"""Sharing weights between training machines.

Every machine trains the same agent on its own duels. Every few minutes it
publishes its weights and replaces them with the average of everybody's
latest weights. There is no coordinator: a machine that stops or restarts
simply drops out of, or rejoins, the average.

Two places to publish to: a folder (for tests on one machine) and the
assets of a GitHub release (for GitHub Actions)."""
import os
import shutil
import subprocess
import tempfile
import time
import urllib.request

import torch


class DirBackend:
    def __init__(self, path):
        self.path = path
        os.makedirs(path, exist_ok=True)

    def put(self, name, src):
        tmp = os.path.join(self.path, f".{name}.{os.getpid()}.tmp")
        shutil.copyfile(src, tmp)
        os.replace(tmp, os.path.join(self.path, name))

    def get(self, name, dst):
        src = os.path.join(self.path, name)
        if not os.path.exists(src):
            return False
        shutil.copyfile(src, dst)
        return True


class ReleaseBackend:
    """Assets of one release of a public repository. Uploads go through the
    `gh` command (needs GH_TOKEN); downloads use the public asset URL, which
    does not count against the API rate limit."""

    def __init__(self, repo, tag):
        self.repo, self.tag = repo, tag
        r = subprocess.run(["gh", "release", "view", tag, "--repo", repo], capture_output=True)
        if r.returncode != 0:
            subprocess.run(["gh", "release", "create", tag, "--repo", repo, "--title", tag,
                            "--notes", "Training checkpoints, replaced as training runs."], check=False)

    def put(self, name, src):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, name)
            shutil.copyfile(src, p)
            for attempt in range(3):
                r = subprocess.run(["gh", "release", "upload", self.tag, p, "--clobber", "--repo", self.repo],
                                   capture_output=True)
                if r.returncode == 0:
                    return
                time.sleep(5 * (attempt + 1))

    def get(self, name, dst):
        url = f"https://github.com/{self.repo}/releases/download/{self.tag}/{name}"
        try:
            with urllib.request.urlopen(url, timeout=60) as r, open(dst, "wb") as f:
                shutil.copyfileobj(r, f)
            return True
        except Exception:
            return False


def pack(policy, vs, samples, iters):
    sd = {k: v.detach().to(torch.float16) for k, v in policy.state_dict().items()}
    return {"policy": sd, "vs": (vs.var, vs.count), "samples": samples, "iters": iters, "time": time.time()}


class Syncer:
    def __init__(self, backend, worker_id, n_workers, every_minutes):
        self.b, self.id, self.n = backend, worker_id, n_workers
        self.every = every_minutes * 60.0
        self.last = time.time()
        self.tmp = tempfile.mkdtemp(prefix="pvpsync")
        self.peer_samples = {}

    def name(self, i):
        return f"w{i:02d}.pt"

    def fetch_peers(self, max_age):
        out = []
        now = time.time()
        for i in range(self.n):
            if i == self.id:
                continue
            dst = os.path.join(self.tmp, self.name(i))
            if not self.b.get(self.name(i), dst):
                continue
            try:
                ck = torch.load(dst, map_location="cpu", weights_only=False)
            except Exception:
                continue
            self.peer_samples[i] = ck.get("samples", 0)
            if max_age is None or now - ck.get("time", 0) <= max_age:
                out.append(ck)
        return out

    def restore(self, policy, vs):
        """At start-up: continue from this worker's own file if there is one,
        then pull towards whatever the others have. Returns (samples, iters)."""
        samples, iters = 0, 0
        own = os.path.join(self.tmp, self.name(self.id))
        peers = self.fetch_peers(None)
        if self.b.get(self.name(self.id), own):
            ck = torch.load(own, map_location="cpu", weights_only=False)
            samples, iters = ck.get("samples", 0), ck.get("iters", 0)
            if not peers:
                policy.load_state_dict({k: v.float() for k, v in ck["policy"].items()})
                vs.var, vs.count = ck["vs"]
        if peers:
            newest = max(peers, key=lambda c: c.get("time", 0))
            fresh = [c for c in peers if newest["time"] - c.get("time", 0) < 4 * self.every]
            self._load_mean(policy, vs, fresh)
        return samples, iters

    @staticmethod
    def _load_mean(policy, vs, cks, own=None):
        sds = [c["policy"] for c in cks]
        if own is not None:
            sds.append(own)
        mean = {k: torch.stack([sd[k].float() for sd in sds]).mean(0) for k in sds[0]}
        policy.load_state_dict(mean)
        vars_ = [c["vs"][0] for c in cks] + ([vs.var] if own is not None else [])
        vs.var = sum(vars_) / len(vars_)
        if own is None:
            vs.count = max(c["vs"][1] for c in cks)

    def maybe_sync(self, policy, vs, samples, iters):
        """Publish and average if it is time. Returns the number of peers
        averaged with, or None when nothing was done."""
        if time.time() - self.last < self.every:
            return None
        self.last = time.time()
        own_path = os.path.join(self.tmp, "own.pt")
        torch.save(pack(policy, vs, samples, iters), own_path)
        self.b.put(self.name(self.id), own_path)
        peers = self.fetch_peers(3 * self.every)
        if peers:
            self._load_mean(policy, vs, peers, own={k: v.detach() for k, v in policy.state_dict().items()})
        return len(peers)

    def total_samples(self, own):
        return own + sum(self.peer_samples.values())
