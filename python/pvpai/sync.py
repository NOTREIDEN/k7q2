"""Sharing weights between training machines.

Every machine trains the same agent on its own duels. Every few minutes it
publishes its weights and replaces them with the average of everybody's
latest weights. There is no coordinator: a machine that stops or restarts
simply drops out of, or rejoins, the average.

Publishing means writing a file into a folder. Between the processes of one
machine that folder is shared directly. Between GitHub Actions machines each
one has a folder it writes to and a folder it reads from, and the steps of
the workflow carry the files across as artifacts of the run
(`.github/actions/round`)."""
import os
import shutil
import tempfile
import time

import torch

from pvpai.model import fit_state


class DirBackend:
    """Files in `path`. With `read_path`, other members' files are looked
    for there instead."""

    def __init__(self, path, read_path=None):
        self.path, self.read_path = path, read_path or path
        os.makedirs(self.path, exist_ok=True)
        os.makedirs(self.read_path, exist_ok=True)

    def put(self, name, src):
        tmp = os.path.join(self.path, f".{name}.{os.getpid()}.tmp")
        shutil.copyfile(src, tmp)
        os.replace(tmp, os.path.join(self.path, name))

    def get(self, name, dst):
        src = os.path.join(self.read_path, name)
        if not os.path.exists(src):
            return False
        shutil.copyfile(src, dst)
        return True

    def mark(self, round_id):
        """Note which round the file just written belongs to, for whoever
        carries it to the other members."""
        tmp = os.path.join(self.path, f".round.{os.getpid()}.tmp")
        with open(tmp, "w") as f:
            f.write(f"{round_id}\n")
        os.replace(tmp, os.path.join(self.path, "round.txt"))


def pack(policy, vs, samples, iters, round_id=0, score=0.0, generation=0.0, fresh=False, origin=0.0):
    """What a member publishes. It is also a checkpoint that `load_policy`
    reads, with the weights stored in half precision."""
    sd = {k: v.detach().to(torch.float16) for k, v in policy.state_dict().items()}
    return {"policy": sd, "vs": (vs.var, vs.count), "samples": samples, "iters": iters, "time": time.time(),
            "round": round_id, "origin": origin, "score": score, "generation": generation, "fresh": fresh,
            "obs_dim": policy.enc[0].in_features, "priv_dim": policy.penc[0].in_features,
            "act_dims": list(policy.act_dims)}


class Syncer:
    """One group of machines (or processes) that share weights.

    Rounds happen at fixed clock times, the same for everybody. After the
    first iteration that ends in a new round a member publishes its weights
    and waits a little for the others to publish theirs for that round.
    Waiting matters: averaging with weights from an earlier round throws
    away the progress made since.

    mode "average": the member replaces its weights with the mean of
    everybody's.
    mode "best": every member trains its own network; a member whose score
    is in the bottom quarter, and clearly below the best, adopts the best
    member's weights.

    `generation` says which network a member is carrying. A member that
    takes on a network from outside the group raises it, and the others in
    an averaging group then adopt that network instead of averaging two
    unrelated ones."""

    def __init__(self, backend, worker_id, n_workers, every_minutes, wait_seconds=30.0, mode="average",
                 margin=5.0, origin=0.0):
        self.b, self.id, self.n = backend, worker_id, n_workers
        self.every = every_minutes * 60.0
        self.wait = wait_seconds
        self.mode = mode
        self.margin = margin
        # Round k ends at origin + k * every, on everybody's clock. Files
        # published under another origin belong to an earlier run.
        self.origin = origin
        self.round = self.clock_round()
        self.first_round = self.round
        self.generation, self.fresh = 0.0, False
        self.adopted_score = 0.0
        self.seen = {}
        self.tmp = tempfile.mkdtemp(prefix="pvpsync")
        self.peer_samples = {}

    def clock_round(self):
        # No rounds before the origin.
        return max(int((time.time() - self.origin) // self.every), -1)

    def name(self, i):
        return f"w{i:02d}.pt"

    def publish(self, policy, vs, samples, iters, score=0.0):
        """Write this member's weights without waiting for anybody."""
        own_path = os.path.join(self.tmp, "own.pt")
        torch.save(pack(policy, vs, samples, iters, self.round, score, self.generation, self.fresh, self.origin),
                   own_path)
        self.b.put(self.name(self.id), own_path)

    def new_generation(self, fresh):
        """Call after loading weights that did not come from this group."""
        self.generation, self.fresh = time.time(), fresh

    def fetch(self, max_age, only=None, include_self=False):
        out = []
        now = time.time()
        for i in (range(self.n) if only is None else only):
            if i == self.id and not include_self:
                continue
            dst = os.path.join(self.tmp, self.name(i))
            if not self.b.get(self.name(i), dst):
                continue
            try:
                ck = torch.load(dst, map_location="cpu", weights_only=False)
            except Exception:
                continue
            if i != self.id:
                self.peer_samples[i] = ck.get("samples", 0)
            ck["worker"] = i
            if max_age is None or now - ck.get("time", 0) <= max_age:
                out.append(ck)
        return out

    def restore(self, policy, vs):
        """At start-up, continue from what was published before. Returns
        (samples, iters, loaded)."""
        cks = self.fetch(None, include_self=True)
        own = next((c for c in cks if c["worker"] == self.id), None)
        samples, iters = (own.get("samples", 0), own.get("iters", 0)) if own else (0, 0)
        if not cks:
            return samples, iters, False
        if self.mode == "best":
            src = [own if own is not None else max(cks, key=lambda c: c.get("score", 0.0))]
        else:
            newest = max(c.get("time", 0) for c in cks)
            gen = max(c.get("generation", 0.0) for c in cks)
            src = [c for c in cks if newest - c.get("time", 0) < 4 * self.every and c.get("generation", 0.0) == gen]
            src = src or [max(cks, key=lambda c: c.get("time", 0))]
        self._load_mean(policy, vs, src)
        self.generation = max(c.get("generation", 0.0) for c in src)
        return samples, iters, True

    @staticmethod
    def _load_mean(policy, vs, cks, own=None):
        sds = [c["policy"] for c in cks]
        if own is not None:
            sds.append(own)
        mean = {k: torch.stack([sd[k].float() for sd in sds]).mean(0) for k in sds[0]}
        # Weights from before the simulator gained inputs or controls are
        # grown to fit, the same way on every machine.
        mean, _ = fit_state(mean, policy)
        policy.load_state_dict(mean)
        vars_ = [c["vs"][0] for c in cks] + ([vs.var] if own is not None else [])
        vs.var = sum(vars_) / len(vars_)
        if own is None:
            vs.count = max(c["vs"][1] for c in cks)

    def maybe_sync(self, policy, vs, samples, iters, score=0.0):
        """Returns None when it is not time yet, otherwise (what, changed):
        a short description, and "" / "moved" / "replaced" for whether the
        weights now come from outside this member's own training
        ("replaced" means an unrelated network: reset the optimiser)."""
        rnd = self.clock_round()
        if rnd <= self.round:
            return None
        self.round = rnd
        self.publish(policy, vs, samples, iters, score)
        self.b.mark(rnd)
        deadline = time.time() + self.wait
        current, missing = [], [i for i in range(self.n) if i != self.id]
        while True:
            for c in self.fetch(3 * self.every, missing):
                if c.get("round", 0) >= rnd and c.get("origin", 0.0) == self.origin:
                    current.append(c)
                    missing.remove(c["worker"])
                    self.seen[c["worker"]] = rnd
            # Only wait for members that took part recently, so that one
            # that stopped does not hold up every round.
            waiting_for = [i for i in missing
                           if self.seen.get(i, -1) >= rnd - 3 or rnd - self.first_round <= 2]
            if not waiting_for or time.time() >= deadline:
                break
            time.sleep(max(0.2, min(5.0, self.wait / 10)))
        if not current:
            return "nobody else published in time", ""
        if self.mode == "best":
            scores = sorted([c.get("score", 0.0) for c in current] + [score])
            cut = scores[max(len(scores) // 4 - 1, 0)]
            best = max(current, key=lambda c: c.get("score", 0.0))
            if score <= cut and best.get("score", 0.0) > score + self.margin:
                self._load_mean(policy, vs, [best])
                self.adopted_score = best.get("score", 0.0)
                return (f"adopted the best of {len(current)} others (score {best.get('score', 0.0):+.1f} "
                        f"against {score:+.1f})"), "replaced"
            return f"kept own weights (score {score:+.1f}, {len(current)} others)", ""
        gen = max(c.get("generation", 0.0) for c in current)
        if gen > self.generation:
            src = [c for c in current if c.get("generation", 0.0) == gen]
            self._load_mean(policy, vs, src)
            self.generation = gen
            self.fresh = any(c.get("fresh", False) for c in src)
            return f"took over the network of {len(src)} others", ("replaced" if self.fresh else "moved")
        same = [c for c in current if c.get("generation", 0.0) == self.generation]
        if not same:
            return "the others have not caught up yet", ""
        self._load_mean(policy, vs, same, own={k: v.detach() for k, v in policy.state_dict().items()})
        return f"averaged with {len(same)} others", ""

    def total_samples(self, own):
        return own + sum(self.peer_samples.values())
