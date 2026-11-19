"""Pinned versions in the league (pvpai.league.PIN, League(pin_share=...))
and in the pool kept on the branch "weights" (pool.py).

    cd python && python test_league_pin.py"""
import os
import subprocess
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from pvpai.league import PIN, League, Member  # noqa: E402


class Still:
    def initial_state(self, n):
        return (np.zeros(n), np.zeros(n))


def league(names, groups, pin_share, seed=3, record=None):
    lg = League(None, None, groups, 2, seed=seed, pin_share=pin_share)
    for nm in names:
        m = Member(nm, Still())
        if record and nm in record:
            m.won, m.lost = record[nm]
        lg.members[nm] = m
    return lg


def old_assign(lg):
    """The draw as it was before pins (for the off-by-default check)."""
    ms = list(lg.members.values())
    w = np.array([(1.0 - m.beaten) ** 2 + 0.05 for m in ms])
    for m in ms:
        m.won *= 0.9
        m.lost *= 0.9
    out = []
    for _ in range(len(lg.playing)):
        out.append(ms[lg.rng.choice(len(ms), p=w / w.sum())].name)
    return out


def test_off_by_default_draws_as_before():
    names = ["001000", "002000", PIN + "w2340", "003000"]
    rec = {"001000": (5, 1), "002000": (1, 3), PIN + "w2340": (0, 4), "003000": (2, 2)}
    a, b = league(names, 4, 0.0, record=rec), league(names, 4, 0.0, record=rec)
    for _ in range(50):
        a.assign()
        assert [m.name for m in a.playing] == old_assign(b)


def test_pin_share_floor():
    names = ["001000", "002000", PIN + "w2340", PIN + "w0100", "003000"]
    for share, groups, want in ((0.5, 4, 2), (0.25, 4, 1), (0.1, 4, 1), (1.0, 4, 4), (0.5, 3, 2)):
        lg = league(names, groups, share)
        pinned = np.zeros(groups)
        for _ in range(200):
            lg.assign()
            for g, m in enumerate(lg.playing):
                pinned[g] += m.name.startswith(PIN)
        assert np.all(pinned[:want] == 200), (share, groups, pinned)
        # The other groups draw from everybody, pins included.
        if want < groups:
            assert 0 < pinned[want:].sum() < 200 * (groups - want), (share, groups, pinned)


def test_pins_drawn_by_record():
    # Among pins the one the agent loses to is drawn more often.
    names = [PIN + "hard", PIN + "easy", "001000"]
    lg = league(names, 1, 1.0, record={PIN + "hard": (0, 40), PIN + "easy": (40, 0)})
    hard = 0
    for _ in range(400):
        lg.members[PIN + "hard"].won, lg.members[PIN + "hard"].lost = 0, 40
        lg.members[PIN + "easy"].won, lg.members[PIN + "easy"].lost = 40, 0
        lg.assign()
        hard += lg.playing[0].name == PIN + "hard"
    assert hard > 300, hard


def test_no_pins_no_change():
    names = ["001000", "002000"]
    a, b = league(names, 4, 0.5), league(names, 4, 0.0)
    for _ in range(20):
        a.assign()
        b.assign()
        assert [m.name for m in a.playing] == [m.name for m in b.playing]


def test_pool_keeps_pins():
    tree = "\n".join([
        "100644 blob aaaa\tpool/001000.pt",
        "100644 blob bbbb\tpool/001400.pt",
        "100644 blob cccc\tpool/pin_w2340.pt",
        "100644 blob dddd\tpool/notes.txt",
        "100644 blob eeee\tpool/other.pt",
    ]) + "\n"
    out = subprocess.run([sys.executable, os.path.join(HERE, "pool.py"), "--samples", "1800", "--blob", "ffff"],
                         input=tree, capture_output=True, text=True, cwd=HERE, check=True).stdout.splitlines()
    assert out == ["100644 blob aaaa\t001000.pt", "100644 blob bbbb\t001400.pt", "100644 blob ffff\t001800.pt",
                   "100644 blob cccc\tpin_w2340.pt"], out
    # Without pins the output is what it always was.
    plain = "\n".join(tree.splitlines()[:2]) + "\n"
    out = subprocess.run([sys.executable, os.path.join(HERE, "pool.py"), "--samples", "1800", "--blob", "ffff"],
                         input=plain, capture_output=True, text=True, cwd=HERE, check=True).stdout.splitlines()
    assert out == ["100644 blob aaaa\t001000.pt", "100644 blob bbbb\t001400.pt", "100644 blob ffff\t001800.pt"], out


if __name__ == "__main__":
    for k, f in sorted(globals().items()):
        if k.startswith("test_"):
            f()
            print("ok", k)
