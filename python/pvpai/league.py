"""Earlier versions of the agent as opponents.

An agent that only ever meets its newest self can go round in circles: it
drops a habit its opponent no longer punishes, the opponent (itself) drops
the answer, and the habit comes back. So part of the duels are played
against a pool of frozen earlier versions, the newest half an hour old and
each one before it about twice as old. They play the second player, the
one with the better connection, the faster clicks and at times the unfair
edges, and they are not trained. How often the agent beats each of them
also says whether it is still getting better, which the scripted bots
stopped saying when it started winning nearly every duel against them.

The pool is a folder of checkpoints named by the samples (in millions) the
network had seen, `007784.pt`. `ladder` is the rule for what stays in it."""
import copy
import os

import numpy as np
import torch

from pvpai.model import load_policy


def ladder(have, new, keep=10, gap=300):
    """The pool after a network with `new` million samples comes by, as a
    sorted list of sample counts. It joins when it is at least `gap` ahead
    of the newest version in the pool. Above `keep` versions the one that
    leaves is the one whose neighbours are closest in age, counted as a
    ratio, so that ages stay spread evenly from half an hour to the whole
    history; the oldest and the newest always stay."""
    out = sorted(set(have))
    if out and new - out[-1] < gap:
        return out
    out.append(new)
    while len(out) > max(keep, 2):
        age = [new - s + gap for s in out]
        i = min(range(1, len(out) - 1), key=lambda i: age[i - 1] / age[i + 1])
        del out[i]
    return out


class Member:
    def __init__(self, name, policy, stamp=None):
        self.name, self.policy, self.stamp = name, policy, stamp
        # Duels the agent won and lost against it, fading with time.
        self.won, self.lost = 0.0, 0.0

    @property
    def beaten(self):
        """The share of the duels against it that the agent won; one half
        before any was played."""
        return (self.won + 2.0) / (self.won + self.lost + 4.0)


class League:
    """`groups` batches of `n` duels, each against one version of the pool
    in `folder`."""

    # The name of the stand-in while the folder is empty.
    NOW = "now"

    def __init__(self, folder, dims, groups, n, seed=0):
        self.folder, self.dims, self.n = folder, dims, n
        self.members = {}
        self.playing = [None] * groups
        self.state = [None] * groups
        self.rng = np.random.default_rng(seed)

    def refresh(self, current):
        """Load the versions that appeared in the folder and forget the ones
        that left it. While it holds none, the agent as it is right now
        stands in for the pool."""
        found = {}
        if self.folder and os.path.isdir(self.folder):
            for f in sorted(os.listdir(self.folder)):
                if f.endswith(".pt") and not f.startswith("."):
                    path = os.path.join(self.folder, f)
                    found[f[:-3]] = (path, os.path.getmtime(path))
        for name in [k for k in self.members if k not in found]:
            del self.members[name]
        for name, (path, stamp) in found.items():
            m = self.members.get(name)
            if m is not None and m.stamp == stamp:
                continue
            try:
                policy, _, _ = load_policy(path, self.dims, still=True)
            except Exception:
                # Half written, or not a checkpoint: look again next time.
                continue
            policy.eval()
            self.members[name] = Member(name, policy, stamp)
        if not self.members:
            self.members[self.NOW] = Member(self.NOW, copy.deepcopy(current).eval())

    def assign(self):
        """Draw the version each group plays next. A version the agent
        loses to is drawn more often than one it beats, and every one of
        them now and then."""
        ms = list(self.members.values())
        w = np.array([(1.0 - m.beaten) ** 2 + 0.05 for m in ms])
        for m in ms:
            m.won *= 0.9
            m.lost *= 0.9
        for g in range(len(self.playing)):
            m = ms[self.rng.choice(len(ms), p=w / w.sum())]
            if self.playing[g] is not m:
                self.playing[g] = m
                self.state[g] = m.policy.initial_state(self.n)

    @torch.no_grad()
    def act(self, g, obs, priv, done):
        """The actions of the version group `g` plays. `done` marks the
        duels that have just started over."""
        m = self.playing[g]
        h, c = self.state[g]
        if done.any():
            keep = torch.from_numpy(done == 0).to(h.dtype).unsqueeze(1)
            h, c = h * keep, c * keep
        act, _, _, self.state[g] = m.policy.act(torch.from_numpy(obs).clone(), torch.from_numpy(priv).clone(), (h, c))
        return act.numpy().astype(np.int32)

    def result(self, g, won, lost):
        """Duels the agent won and lost in group `g` since the last call."""
        m = self.playing[g]
        if m is not None:
            m.won += won
            m.lost += lost

    def table(self):
        """Name and share of duels won against it, oldest version first."""
        return [(m.name, m.beaten) for _, m in sorted(self.members.items())]
