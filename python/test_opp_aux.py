"""Tests of the head that guesses what the opponent is up to
(`pvpai/opp_aux.py`, `train.py --opp-aux-coef`).

    cd python && python test_opp_aux.py

They need the simulator library built (sim/target/release/libpvpsim.so).
`test_off_is_bit_identical` trains with the code of a git revision
(PVPAI_BASE_REF, HEAD by default: the code before uncommitted changes)
and with the code here, with the head off, and asks for the same losses and
the same weights to the bit."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

import numpy as np
import torch

PY = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(PY)
sys.path.insert(0, PY)

from pvpai import opp_aux  # noqa: E402
from pvpai.env import BOT, VecEnv, default_lib_path  # noqa: E402
from pvpai.model import Policy, load_policy  # noqa: E402
from pvpai.ppo import RunningScale, Stream, ppo_update  # noqa: E402

# Duels with real health, potions and pearls, so that every target occurs.
PARAMS = dict(infinite_health=0, health_prob=1, pearl_prob=1, potions_min=6, potions_max=7, bot_level_min=2,
              bot_level_max=4, ep_ticks_min=400, ep_ticks_max=600, bot_potion=1, bot_pearl=1)


def _need_lib():
    if not os.path.exists(default_lib_path()):
        raise unittest.SkipTest("simulator library not built")


def _train_args(out, init=None, extra=()):
    a = [sys.executable, "train.py", "--minutes", "0", "--threads", "1", "--seed", "5", "--n-selfplay", "4",
         "--n-bot", "8", "--n-league", "4", "--league-groups", "2", "--turn-drill", "0.3", "--horizon", "32",
         "--hidden", "64", "--minibatches", "2", "--epochs", "2", "--out", out]
    for k, v in PARAMS.items():
        a += ["--param", f"{k}={v}"]
    if init:
        a += ["--init", init]
    return a + list(extra)


def _run(py_dir, out, init=None, extra=()):
    env = dict(os.environ, OMP_NUM_THREADS="1", MKL_NUM_THREADS="1")
    r = subprocess.run(_train_args(out, init, extra), cwd=py_dir, env=env, capture_output=True, text=True, timeout=600)
    assert r.returncode == 0, r.stdout[-2000:] + r.stderr[-4000:]
    return os.path.join(out, "latest.pt")


def _chain(py_dir, out, iters, extra=()):
    """`iters` iterations, one per run of train.py, each from the last."""
    ck = None
    for i in range(iters):
        ck = _run(py_dir, os.path.join(out, f"r{i}"), ck, extra)
    rows = []
    for i in range(iters):
        with open(os.path.join(out, f"r{i}", "log.jsonl")) as f:
            for line in f:
                row = json.loads(line)
                for k in ("minutes", "sps", "rollout_s", "update_s"):
                    row.pop(k, None)
                rows.append(row)
    return rows, torch.load(ck, map_location="cpu", weights_only=False)


def test_targets_by_hand():
    """The targets of a short made-up sequence, worked out by hand."""
    T, B = 30, 1
    priv = torch.zeros(T, B, 71)
    done = torch.zeros(T, B, dtype=torch.bool)
    priv[..., opp_aux.P_HIT_HEALTH] = 1.0
    priv[:, 0, opp_aux.P_HP] = torch.linspace(1.0, 0.5, T)
    # Our hurt time: counting down, a hit from step 7 to 8.
    hurt = torch.zeros(T)
    hurt[8:] = torch.clamp(1.0 - 0.05 * torch.arange(T - 8), min=0.0)
    priv[:, 0, opp_aux.P_HURT_SELF] = hurt
    # Its potions: 5 until step 3, then 4 (thrown from step 3 to 4).
    priv[:, 0, opp_aux.P_POTIONS] = torch.where(torch.arange(T) <= 3, 5 / 7, 4 / 7)
    # A pearl in flight from step 12.
    priv[12:16, 0, opp_aux.P_PEARL_FLYING] = 1.0
    priv[:, 0, 0] = torch.arange(T) * 0.1
    # A duel ends with step 25.
    done[25, 0] = True
    y, m = opp_aux.targets(priv, done)
    s = opp_aux.SLICES
    hit5, hit5m = y[:, 0, s["hit5"].start], m[:, 0, s["hit5"].start]
    # The hit is the change 7 -> 8: within 5 of steps 3..7.
    assert hit5[:21].tolist() == [0, 0, 0, 1, 1, 1, 1, 1] + [0] * 13
    # Windows that run into the end of the duel (step 25) or of the stored steps are out.
    assert hit5m.tolist() == [1] * 21 + [0] * 9
    hit10m = m[:, 0, s["hit10"].start]
    assert hit10m.tolist() == [1] * 16 + [0] * 14
    pot, potm = y[:, 0, s["pot20"].start], m[:, 0, s["pot20"].start]
    assert pot[:6].tolist() == [1, 1, 1, 1, 0, 0] and potm.tolist() == [1] * 6 + [0] * 24
    pearl = y[:, 0, s["pearl20"].start]
    assert pearl[:6].tolist() == [1] * 6 and pearl[0] == 1  # the change 11 -> 12 is within 20 of step 0
    pos5 = y[:, 0, s["pos5"]]
    assert torch.allclose(pos5[:20, 0], (torch.arange(20) + 5) * 0.1)
    assert m[:, 0, s["pos5"].start].tolist() == [1] * 21 + [0] * 9
    assert torch.allclose(y[:, 0, s["hp"].start], priv[:, 0, opp_aux.P_HP]) and bool((m[:, 0, s["hp"].start] == 1).all())
    # No health to lose: no health target.
    priv[..., opp_aux.P_HIT_HEALTH] = 0.0
    _, m2 = opp_aux.targets(priv, done)
    assert float(m2[..., s["hp"].start].sum()) == 0.0


def test_off_is_bit_identical():
    """With the head off, training is what it was: same losses, same
    weights and optimiser state after a few iterations, same seed."""
    _need_lib()
    ref = os.environ.get("PVPAI_BASE_REF", "HEAD")
    tmp = tempfile.mkdtemp(prefix="oppaux_")
    try:
        base = os.path.join(tmp, "base")
        os.makedirs(base)
        try:
            arch = subprocess.run(["git", "-C", ROOT, "archive", ref, "python"], capture_output=True, check=True).stdout
        except (OSError, subprocess.CalledProcessError) as e:
            raise unittest.SkipTest(f"no git revision {ref} to compare with: {e}")
        subprocess.run(["tar", "-x", "-C", base], input=arch, check=True)
        lib_dir = os.path.join(base, "sim", "target", "release")
        os.makedirs(lib_dir)
        os.symlink(default_lib_path(), os.path.join(lib_dir, "libpvpsim.so"))
        rows0, ck0 = _chain(os.path.join(base, "python"), os.path.join(tmp, "a"), 3)
        rows1, ck1 = _chain(PY, os.path.join(tmp, "b"), 3, ["--opp-aux-coef", "0"])
        assert len(rows0) == len(rows1) == 3
        for r0, r1 in zip(rows0, rows1):
            assert r0 == r1, (r0, r1)
        assert ck0["policy"].keys() == ck1["policy"].keys()
        for k in ck0["policy"]:
            assert torch.equal(ck0["policy"][k], ck1["policy"][k]), k
        s0, s1 = ck0["opt"]["state"], ck1["opt"]["state"]
        assert s0.keys() == s1.keys()
        for i in s0:
            for k in s0[i]:
                assert torch.equal(s0[i][k], s1[i][k]), (i, k)
        assert ck0["vs"] == ck1["vs"]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _collect(policy, env, stream, acts, gamma=0.995):
    """One rollout of `stream.T` steps against the bots, as `train.py`
    plays them: `acts` are pending, and so are the ones returned."""
    while not stream.full():
        env.step(acts)
        stream.close(policy, env.rew, env.done, env.fobs, env.fpriv, gamma, 1.0, env.obs)
        acts = stream.choose(policy, env.obs, env.priv)
    return acts


def _aux_loss_on(policy, batch):
    obs, priv, act, h0, c0, done, y, m, norm = batch
    with torch.no_grad():
        pred = policy.evaluate_seq(obs, priv, act, (h0, c0), done, per_head=True, opp_aux=True)[-1]
    total, parts = opp_aux.loss(pred, y, m, norm)
    return float(total), parts, opp_aux.metrics(pred, y, m)


def test_aux_loss_falls():
    """With a weight above 0 the guesses get better on duels the head never
    learnt from."""
    _need_lib()
    torch.manual_seed(0)
    np.random.seed(0)
    torch.set_num_threads(1)
    n, T, hidden = 24, 64, 64

    def mk_env(seed):
        e = VecEnv(n, BOT, seed)
        for k, v in PARAMS.items():
            e.set_param(k, v)
        return e

    policy = Policy(*_dims(), hidden, opp_aux=opp_aux.DIM)
    # Duels kept apart for judging, played by the untrained policy.
    held_env = mk_env(99)
    hs = Stream("held", n, held_env.obs_dim, held_env.priv_dim, len(held_env.act_dims), T * 4, hidden)
    _collect(policy, held_env, hs, hs.choose(policy, *held_env.reset()))
    y, m = opp_aux.targets(hs.priv, hs.done)
    held = (hs.obs, hs.priv, hs.act, hs.h0, hs.c0, hs.done, y, m, opp_aux.norms(y, m))
    before, parts0, _ = _aux_loss_on(policy, held)

    env = mk_env(7)
    st = Stream("bot", n, env.obs_dim, env.priv_dim, len(env.act_dims), T, hidden)
    opt = torch.optim.Adam(policy.parameters(), lr=1e-3, eps=1e-5)
    vs = RunningScale()
    cfg = dict(gamma=0.995, lam=0.95, clip=0.2, epochs=2, minibatches=2, vf_coef=0.5, ent_coef=0.0015,
               max_grad_norm=0.5, opp_aux_coef=1.0)
    acts = st.choose(policy, *env.reset())
    losses = []
    for _ in range(25):
        acts = _collect(policy, env, st, acts)
        info = ppo_update(policy, opt, [st], vs, cfg)
        losses.append(info["opp_aux_loss"])
    after, parts1, met = _aux_loss_on(policy, held)
    print(f"held-out aux loss {before:.3f} -> {after:.3f}; training {losses[0]:.3f} -> {losses[-1]:.3f}")
    print("  before", {k: round(v, 3) for k, v in parts0.items()})
    print("  after ", {k: round(v, 3) for k, v in parts1.items()})
    print("  held-out", {k: {a: round(b, 3) for a, b in v.items()} for k, v in met.items()})
    # A run this short with a fresh network mostly learns where each target
    # lies on average; telling the opponent's moves apart takes a trained
    # memory and a longer run.
    assert after < 0.7 * before, (before, after)
    assert np.mean(losses[-5:]) < 0.7 * np.mean(losses[:3]), losses


def _dims():
    e = VecEnv(1, BOT, 0)
    d = (e.obs_dim, e.priv_dim, e.act_dims)
    e.close()
    return d


def test_load_and_export_ignore_the_head():
    """Checkpoints with and without the head load; the head is not in what
    the mod reads, and a checkpoint with it exports to the same bytes as
    the same weights without it."""
    _need_lib()
    import export_policy
    obs, priv, acts = _dims()
    tmp = tempfile.mkdtemp(prefix="oppaux_")
    try:
        torch.manual_seed(3)
        pol = Policy(obs, priv, acts, 64, opp_aux=opp_aux.DIM)
        ck = {"policy": pol.state_dict(), "priv_dim": priv, "act_dims": acts, "samples": 7, "vs": (1.0, 1.0),
              "iters": 1, "marks": []}
        with_head, without = os.path.join(tmp, "with.pt"), os.path.join(tmp, "without.pt")
        torch.save(ck, with_head)
        torch.save(dict(ck, policy={k: v for k, v in ck["policy"].items() if not k.startswith("opp_aux.")}), without)
        p1, _, _ = load_policy(with_head, (obs, priv, acts))
        p0, _, _ = load_policy(without, (obs, priv, acts))
        assert p1.opp_aux_dim == opp_aux.DIM and p0.opp_aux_dim == 0
        for k, v in p0.state_dict().items():
            assert torch.equal(v, p1.state_dict()[k]), k
        b1, b0 = os.path.join(tmp, "with.bin"), os.path.join(tmp, "without.bin")
        export_policy.write_policy(load_policy(with_head, (obs, priv, acts), still=True)[0], 7, b1)
        export_policy.write_policy(load_policy(without, (obs, priv, acts), still=True)[0], 7, b0)
        with open(b1, "rb") as f1, open(b0, "rb") as f0:
            assert f1.read() == f0.read()
        old = "/home/claude/runs/w1310/latest.pt"
        if os.path.exists(old):
            p, _, _ = load_policy(old, (obs, priv, acts))
            assert p.opp_aux_dim == 0
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_train_switches_head_on_and_off():
    """train.py continues a checkpoint without the head with it on (the
    head starts afresh, the rest and the optimiser as stored), and one with
    the head with it off (the head is dropped)."""
    _need_lib()
    tmp = tempfile.mkdtemp(prefix="oppaux_")
    try:
        plain = _run(PY, os.path.join(tmp, "plain"))
        on = _run(PY, os.path.join(tmp, "on"), plain, ["--opp-aux-coef", "0.5"])
        ck_on = torch.load(on, map_location="cpu", weights_only=False)
        assert any(k.startswith("opp_aux.") for k in ck_on["policy"])
        assert len(ck_on["opt"]["state"]) == len(ck_on["policy"])
        again = _run(PY, os.path.join(tmp, "again"), on, ["--opp-aux-coef", "0.5"])
        off = _run(PY, os.path.join(tmp, "off"), again)
        ck_off = torch.load(off, map_location="cpu", weights_only=False)
        assert not any(k.startswith("opp_aux.") for k in ck_off["policy"])
        assert len(ck_off["opt"]["state"]) == len(ck_off["policy"])
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    failed = 0
    for name, fn in list(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print("ok  ", name, flush=True)
            except unittest.SkipTest as e:
                print("skip", name, e, flush=True)
            except Exception:
                import traceback
                traceback.print_exc()
                print("FAIL", name, flush=True)
                failed += 1
    sys.exit(1 if failed else 0)
