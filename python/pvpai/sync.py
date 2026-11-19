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
import math
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


def pack(policy, vs, samples, iters, round_id=0, score=0.0, generation=0.0, fresh=False, origin=0.0, marks=()):
    """What a member publishes. It is also a checkpoint that `load_policy`
    reads, with the weights stored in half precision."""
    sd = {k: v.detach().to(torch.float16) for k, v in policy.state_dict().items()}
    return {"policy": sd, "vs": (vs.var, vs.count), "samples": samples, "iters": iters, "time": time.time(),
            "round": round_id, "origin": origin, "score": score, "generation": generation, "fresh": fresh,
            "obs_dim": policy.enc[0].in_features, "priv_dim": policy.penc[0].in_features,
            "act_dims": list(policy.act_dims), "marks": list(marks)}


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
    unrelated ones.

    `island_dir`: another fleet trains the same agent on its own machines
    and publishes its averaged network every round or two; the round steps
    of the workflow fetch it into this folder. After averaging, a member
    moves `island_beta` of the way towards the newest of those networks
    that was published before the previous round began, so that every
    member of this group has it and picks the same one, and each such
    network once. The two fleets pull each other together this way without
    waiting for each other. A network further than `island_max_dist` away
    (relative to the size of the weights) is left out: that fleet has gone
    elsewhere, or broke."""

    def __init__(self, backend, worker_id, n_workers, every_minutes, wait_seconds=30.0, mode="average",
                 margin=5.0, origin=0.0, outer_lr=1.0, outer_mom=0.0, island_dir=None, island_beta=0.0,
                 island_max_dist=0.08):
        self.b, self.id, self.n = backend, worker_id, n_workers
        # The step an averaging round takes (mode "average"): from the
        # weights all members started the round with (`anchor`) towards the
        # mean, `outer_lr` times as far, with Nesterov momentum `outer_mom`
        # over the rounds (DiLoCo, arXiv 2311.08105). 1 and 0: the plain mean.
        self.outer_lr, self.outer_mom = outer_lr, outer_mom
        self.anchor, self.mom = None, None
        self.outer_log = ""
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
        # The one-time restarts the weights have had (see `model.RESTART_CHOICES`).
        self.marks = []
        self.adopted_score = 0.0
        self.seen = {}
        self.tmp = tempfile.mkdtemp(prefix="pvpsync")
        self.peer_samples = {}
        self.island_dir, self.island_beta, self.island_max_dist = island_dir, island_beta, island_max_dist
        self.island_time = 0.0

    def clock_round(self):
        # No rounds before the origin.
        return max(int((time.time() - self.origin) // self.every), -1)

    def name(self, i):
        return f"w{i:02d}.pt"

    def publish(self, policy, vs, samples, iters, score=0.0):
        """Write this member's weights without waiting for anybody."""
        own_path = os.path.join(self.tmp, "own.pt")
        torch.save(pack(policy, vs, samples, iters, self.round, score, self.generation, self.fresh, self.origin, self.marks),
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
        self.anchor, self.mom = None, None
        self.generation = max(c.get("generation", 0.0) for c in src)
        # A restart every one of them has had is not made again.
        self.marks = sorted(set.intersection(*[set(c.get("marks", ())) for c in src]))
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
            self.anchor, self.mom = None, None
            self.generation = gen
            self.fresh = any(c.get("fresh", False) for c in src)
            return f"took over the network of {len(src)} others", ("replaced" if self.fresh else "moved")
        same = [c for c in current if c.get("generation", 0.0) == self.generation]
        if not same:
            return "the others have not caught up yet", ""
        own = {k: v.detach() for k, v in policy.state_dict().items()}
        if self.outer:
            # Rounded as the others receive it, so that every member that
            # hears from the same ones computes the same mean.
            own = {k: (v.half().float() if v.is_floating_point() else v) for k, v in own.items()}
        self._load_mean(policy, vs, same, own=own)
        if self.outer:
            self._outer_step(policy)
        island = self._island(policy, vs, rnd)
        return f"averaged with {len(same)} others{self.outer_log}{island}", ""

    def _island(self, policy, vs, rnd):
        """After averaging: move towards the other fleet's network (see `island_dir`)."""
        if not self.island_dir or self.island_beta <= 0 or self.mode != "average":
            return ""
        try:
            names = sorted(n for n in os.listdir(self.island_dir) if n.endswith(".pt"))
        except OSError:
            return ""
        start = self.origin + rnd * self.every
        best = None
        for n in names:
            try:
                ck = torch.load(os.path.join(self.island_dir, n), map_location="cpu", weights_only=False)
            except Exception:
                continue
            t = ck.get("time", 0.0)
            # Published before the previous round began, a minute to spare:
            # every member fetched it during that round.
            if t > start - self.every - 60 or t < start - 6 * self.every or t <= self.island_time:
                continue
            if best is None or (t, n) > (best[0], best[2]):
                best = (t, ck, n)
        if best is None:
            return ""
        t, ck, _ = best
        self.island_time = t
        other, _ = fit_state({k: v.float() for k, v in ck["policy"].items()}, policy)
        own = {k: v.detach().clone() for k, v in policy.state_dict().items()}
        keys = [k for k, v in own.items() if v.is_floating_point()]
        if any(k not in other or other[k].shape != own[k].shape for k in keys):
            return ", the other fleet's network does not fit"
        d = math.sqrt(sum(float(((own[k] - other[k]) ** 2).sum()) for k in keys))
        size = math.sqrt(sum(float((own[k] ** 2).sum()) for k in keys))
        rel = d / max(size, 1e-12)
        if rel > self.island_max_dist:
            return f", the other fleet's network is {rel:.3f} away, left out"
        b = self.island_beta
        new = dict(own)
        for k in keys:
            new[k] = (1 - b) * own[k] + b * other[k]
        policy.load_state_dict(new)
        vs.var = (1 - b) * vs.var + b * ck["vs"][0]
        if self.anchor is not None:
            self.anchor = {k: new[k].detach().clone() for k in self.anchor}
        return f", moved {b:.2f} towards the other fleet ({(start - t) / 60:.1f} min old, {rel:.3f} away)"

    @property
    def outer(self):
        return self.mode == "average" and (self.outer_lr != 1.0 or self.outer_mom != 0.0)

    def _outer_step(self, policy):
        """Policy holds the round's mean; move it to the outer step from the anchor."""
        mean = {k: v.detach().clone() for k, v in policy.state_dict().items()}
        keys = [k for k, v in mean.items() if v.is_floating_point()]
        if (self.anchor is None or any(k not in self.anchor or self.anchor[k].shape != mean[k].shape for k in keys)):
            # First round of this network here: the plain mean, which the next round starts from.
            self.anchor, self.mom = {k: mean[k] for k in keys}, {k: torch.zeros_like(mean[k]) for k in keys}
            self.outer_log = ", outer step from the next round"
            return
        g = {k: self.anchor[k] - mean[k] for k in keys}
        mu, lr = self.outer_mom, self.outer_lr
        for k in keys:
            self.mom[k].mul_(mu).add_(g[k])
        step = {k: lr * (g[k] + mu * self.mom[k]) for k in keys}
        gn = math.sqrt(sum(float((x * x).sum()) for x in g.values()))
        sn = math.sqrt(sum(float((x * x).sum()) for x in step.values()))
        mn = math.sqrt(sum(float((x * x).sum()) for x in self.mom.values()))
        dot = sum(float((g[k] * self.mom[k]).sum()) for k in keys)
        # Never more than three times the plain mean's own step.
        scale = min(1.0, 3.0 * gn / sn) if sn > 0 else 1.0
        new = dict(mean)
        for k in keys:
            new[k] = self.anchor[k] - scale * step[k]
        policy.load_state_dict(new)
        self.anchor = {k: new[k].detach().clone() for k in keys}
        self.outer_log = (f", outer step |mean-anchor| {gn:.3f} -> {sn * scale:.3f}"
                          f" (cos with momentum {dot / max(gn * mn, 1e-12):+.2f})")

    def total_samples(self, own):
        return own + sum(self.peer_samples.values())
