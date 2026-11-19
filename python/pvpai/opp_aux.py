"""What the policy's memory is asked to foresee of the opponent, beside
playing (`train.py --opp-aux-coef`, off at 0).

A small head on the memory's output (`Policy.opp_aux`) guesses, on every
step, things a client is never shown but the value function's inputs
(`env.rs` `write_priv`) hold, now or a few steps later in the same duel.
The head itself plays no part in choosing actions and is not exported to
the mod; what it is for is the push its loss gives the memory to keep
track of the opponent: its true health on a server that hides it, and what
it is about to do.

The targets, all from the stored inputs of the value function and the
stored ends of duels, so the simulator gives nothing new for them. A step
is t; "within k" means on one of the k steps from t on (the change from the
inputs of step j to those of step j + 1, for j = t .. t + k - 1):

    hp        the opponent's health now / 20 (priv 55), where hits cost
              health (priv 59 > 0)
    hit5      we take a hit that starts our invulnerable ticks within 5
    hit10     ... within 10 (priv 9, our hurt-resistant time, goes up: a
              melee hit, a rod or snowball, rarely a fall)
    pot20     the opponent throws a healing potion within 20 (priv 57, its
              potions in the hotbar, goes down, or priv 58, a potion of its
              in flight, comes up)
    pearl20   the opponent throws a pearl within 20 (priv 50, its pearls,
              goes down, or priv 52, a pearl of its in flight, comes up)
    pos5      where the opponent is 5 steps on: priv 0-2 then (ahead and to
              the right of where we look then, / 8 blocks; above, / 3)
    pos10     ... 10 steps on

A target is left out where its steps run past the end of the stored
sequence or past the end of a duel (`done`), never on what it turned out
to be, so leaving it out does not tilt what is learnt.

The losses: binary cross entropy for the events, divided by that of
always guessing their share in the batch (1 for a head that has learnt
nothing, 0 for a perfect one); smooth L1 for the rest, on the error divided
by the target's spread in the batch. Each group above counts the same, and
the loss is their mean."""
import math

import torch
import torch.nn.functional as F

# Inputs of the value function (`write_priv`).
P_POS = (0, 1, 2)
P_HURT_SELF = 9
P_PEARLS = 50
P_PEARL_FLYING = 52
P_HP = 55
P_POTIONS = 57
P_POTION_FLYING = 58
P_HIT_HEALTH = 59

# (group, outputs, kind)
GROUPS = [("hp", 1, "reg"), ("hit5", 1, "bce"), ("hit10", 1, "bce"), ("pot20", 1, "bce"), ("pearl20", 1, "bce"),
          ("pos5", 3, "reg"), ("pos10", 3, "reg")]
DIM = sum(n for _, n, _ in GROUPS)
# The first output of each group.
SLICES = {}
_i = 0
for _name, _n, _kind in GROUPS:
    SLICES[_name] = slice(_i, _i + _n)
    _i += _n
BCE_COLS = [SLICES[g].start for g, _, k in GROUPS if k == "bce"]
REG_COLS = [c for g, _, k in GROUPS if k == "reg" for c in range(SLICES[g].start, SLICES[g].stop)]


def _window(x, k):
    """[T, B] -> [T, B]: the sum of x[t .. t + k - 1], and whether the
    steps up to t + k are stored (t + k <= T - 1), so that the k changes
    from t on, and the inputs k steps on, are known."""
    T = x.shape[0]
    cs = torch.cat([torch.zeros_like(x[:1]), x.cumsum(0)], 0)
    end = torch.arange(T, device=x.device) + k
    inside = end <= T - 1
    s = cs[end.clamp(max=T)] - cs[:T]
    return s, inside.view(-1, 1).expand_as(x)


@torch.no_grad()
def targets(priv, done):
    """priv [T, B, P], done [T, B] (the step ended a duel) -> targets and
    masks, each [T, B, DIM] (masks 1.0 where a target counts)."""
    T, B = priv.shape[:2]
    priv = priv.float()
    d = done.float()
    y = torch.zeros(T, B, DIM)
    m = torch.zeros(T, B, DIM)
    # Changes from step j to j + 1 (the last step has none stored).
    nxt = torch.cat([priv[1:], priv[-1:]], 0)
    hurt = (nxt[..., P_HURT_SELF] > priv[..., P_HURT_SELF] + 0.01).float()
    pot = ((nxt[..., P_POTIONS] < priv[..., P_POTIONS] - 0.5 / 7.0)
           | (nxt[..., P_POTION_FLYING] > priv[..., P_POTION_FLYING] + 0.5)).float()
    pearl = ((nxt[..., P_PEARLS] < priv[..., P_PEARLS] - 0.5 / 16.0)
             | (nxt[..., P_PEARL_FLYING] > priv[..., P_PEARL_FLYING] + 0.5)).float()

    def within(ev, k):
        n, inside = _window(ev, k)
        ends, _ = _window(d, k)
        return (n > 0).float(), (inside & (ends == 0)).float()

    c = SLICES["hp"].start
    y[..., c] = priv[..., P_HP]
    m[..., c] = (priv[..., P_HIT_HEALTH] > 0).float()
    for name, ev, k in (("hit5", hurt, 5), ("hit10", hurt, 10), ("pot20", pot, 20), ("pearl20", pearl, 20)):
        c = SLICES[name].start
        y[..., c], m[..., c] = within(ev, k)
    for name, k in (("pos5", 5), ("pos10", 10)):
        sl = SLICES[name]
        ends, inside = _window(d, k)
        ok = (inside & (ends == 0)).float()
        if k < T:
            y[: T - k, :, sl] = priv[k:][..., list(P_POS)]
        m[..., sl] = ok.unsqueeze(-1)
    return y, m


@torch.no_grad()
def norms(y, m):
    """Per output, what its loss is divided by: for an event, the cross
    entropy of guessing its share in this batch; otherwise the target's
    spread in this batch."""
    out = torch.ones(DIM)
    cnt = m.sum((0, 1)).clamp(min=1.0)
    mean = (y * m).sum((0, 1)) / cnt
    var = ((y - mean) ** 2 * m).sum((0, 1)) / cnt
    for c in BCE_COLS:
        p = float(mean[c]) if float(m[..., c].sum()) > 0 else 0.5
        p = min(max(p, 1e-4), 1 - 1e-4)
        out[c] = max(-(p * math.log(p) + (1 - p) * math.log(1 - p)), 0.05)
    for c in REG_COLS:
        out[c] = max(float(var[c]) ** 0.5, 0.02)
    return out


_ORDER = BCE_COLS + REG_COLS


def loss(pred, y, m, norm):
    """pred, y, m [..., DIM]; norm [DIM]. Returns the loss (mean over the
    groups with any target) and each group's loss as a float."""
    bc, rc = BCE_COLS, REG_COLS
    per = torch.cat([F.binary_cross_entropy_with_logits(pred[..., bc], y[..., bc], reduction="none") / norm[bc],
                     F.smooth_l1_loss(pred[..., rc] / norm[rc], y[..., rc] / norm[rc], reduction="none", beta=1.0)], -1)
    mm = m[..., _ORDER]
    lead = tuple(range(per.dim() - 1))
    cnt_o = mm.sum(lead)
    col_o = (per * mm).sum(lead) / cnt_o.clamp(min=1.0)
    col, cnt = torch.zeros(DIM, dtype=per.dtype), torch.zeros(DIM, dtype=per.dtype)
    col = col.index_put((torch.tensor(_ORDER),), col_o)
    cnt = cnt.index_put((torch.tensor(_ORDER),), cnt_o)
    total, n, parts = 0.0, 0, {}
    for name, _, _ in GROUPS:
        sl = SLICES[name]
        have = cnt[sl] > 0
        if not bool(have.any()):
            continue
        g = col[sl][have].mean()
        total = total + g
        n += 1
        parts[name] = float(g.detach())
    if n == 0:
        return pred.sum() * 0.0, parts
    return total / n, parts


@torch.no_grad()
def metrics(pred, y, m):
    """How good the guesses are, per group: for an event its share, the
    accuracy at one half and of always guessing the commoner answer, the
    area under the ROC curve, and the cross entropy divided by that of
    guessing the share; for the rest the mean absolute error, that of
    guessing the mean, and the share of the variance explained."""
    out = {}
    for name, _, kind in GROUPS:
        sl = SLICES[name]
        mm = m[..., sl].reshape(-1) > 0
        if not bool(mm.any()):
            continue
        p = pred[..., sl].reshape(-1)[mm].double()
        t = y[..., sl].reshape(-1)[mm].double()
        if kind == "bce":
            share = float(t.mean())
            acc = float(((p > 0) == (t > 0.5)).double().mean())
            q = min(max(share, 1e-6), 1 - 1e-6)
            h0 = -(q * math.log(q) + (1 - q) * math.log(1 - q))
            ce = float(F.binary_cross_entropy_with_logits(p, t))
            out[name] = dict(n=int(mm.sum()), share=share, acc=acc, acc_base=max(share, 1 - share),
                             auc=_auc(p, t), ce_rel=ce / h0)
        else:
            k = sl.stop - sl.start
            p = pred[..., sl].reshape(-1, k)[m[..., sl.start].reshape(-1) > 0].double()
            t = y[..., sl].reshape(-1, k)[m[..., sl.start].reshape(-1) > 0].double()
            mae = float((p - t).abs().mean())
            mae0 = float((t - t.mean(0)).abs().mean())
            r2 = float(1 - ((p - t) ** 2).sum() / ((t - t.mean(0)) ** 2).sum().clamp(min=1e-12))
            out[name] = dict(n=int(t.shape[0]), mae=mae, mae_base=mae0, r2=r2)
    return out


def _auc(score, label):
    """Area under the ROC curve (ties counted half)."""
    pos = label > 0.5
    n1, n0 = int(pos.sum()), int((~pos).sum())
    if n1 == 0 or n0 == 0:
        return float("nan")
    order = torch.argsort(score)
    _, inv, counts = torch.unique_consecutive(score[order], return_inverse=True, return_counts=True)
    # Ranks from 1, the mean rank of a run of ties for each of them.
    avg = counts.cumsum(0).double() - (counts.double() - 1) / 2
    r = torch.empty(score.shape[0], dtype=torch.double)
    r[order] = avg[inv]
    return float((r[pos].sum() - n1 * (n1 + 1) / 2) / (n1 * n0))
