"""Recurrent policy with one categorical head per control, and a value
head that also sees simulator-only information during training."""
import math
import torch
import torch.nn as nn


# Starting preference of the right mouse button: pressed on about three
# ticks in a hundred.
USE_BIAS = [0.0, -3.5]
# And of the pearl throw: asked for on about one tick in a hundred, which
# with the wait between pearls is a throw every quarter of a minute or so.
PEARL_BIAS = [0.0, -4.5]
# And of the potion throw: about as often, which uses up seven potions
# within a duel, so that some are thrown while hurt and pay off. The same
# control later gained a drink, a meal and a stop, and then a healing
# potion and a potion of Speed from the inventory.
POTION_BIAS = [0.0, -4.5, -5.0, -5.0, -5.0, -4.0, -4.0]
# And of a flick of the mouse: one tick in twenty, a fifth of those for
# each of the five. Rare enough that a fight goes on as it did, often
# enough that every state sees all of them tried within minutes: the turns
# of the yaw control as large as these had gone so far out of use that
# nothing brought them back, and turning round took half a second.
FLICK_BIAS = [0.0, -4.5, -4.5, -4.5, -4.5, -4.5]
# And of the tool of a kit (a rod, snowballs or eggs): used on about one
# tick in fifty-five, a few times a second's worth in a duel, and the sword
# taken back on about one tick in eight with the tool in hand, so that a hook
# has the ticks it takes to reach the opponent before the sword comes back.
TOOL_BIAS = [0.0, -4.0, -2.0]
# Controls that do not start evenly, by their position among the controls.
START_BIAS = {7: USE_BIAS, 8: PEARL_BIAS, 9: POTION_BIAS, 10: FLICK_BIAS, 12: TOOL_BIAS}
# The preference of every choice but the first of a control that an old
# version kept as an opponent never had: it goes on playing as it did.
NEVER = -30.0
# Where a choice added to a control that already exists starts: as a copy
# of the control's first choice (in every one of these controls, doing
# nothing) made rarer by this much, so that the policy plays as before
# until it finds a use for it. A choice with weights of zero would not do:
# the first choice's own preference grows with training, by five and more
# in states the policy is sure of, and beside it a choice that hangs on
# nothing is asked for on one tick in thousands and never tried.
NEW_CHOICE_BIAS = -5.0
# Choices that start as a copy of another choice than the first, with their
# own starting preference, by control: the quarter and half turns of the
# view start as the largest turn there was the same way, one time in twenty
# of those on which that one is asked for.
# And the potions thrown with a turn, behind and to either side, start as
# the plain throw, the three together in its place a little more than one
# time in three: tried in the very states in which potions are thrown, and
# kept where they heal better. The same turns for two potions start as the
# turn for one, about one time in three of those: tried where it pots with a
# turn, and kept where one potion is not enough.
COPY_OF = {5: {27: (0, -3.0), 28: (26, -3.0), 29: (0, -3.0), 30: (26, -3.0)},
           9: {7: (1, -2.0), 8: (1, -2.0), 9: (1, -2.0), 10: (7, -1.0), 11: (8, -1.0), 12: (9, -1.0)}}
# When the potion control is added, the pearl control starts over. With
# infinite health the agent had learnt never to throw a pearl; with health
# to lose, and an opponent that can be knocked away from its own potion, a
# pearl is worth something else, and it has to try them again to find out.
RESTART = {9: (8,)}
# Choices that start over once, by the name of the restart: the choice
# becomes a copy of the control's first choice, rarer by the value given
# (-4 is one tick in fifty-five of those on which nothing is asked). The
# first run with drinks and meals paid for Speed and food only when the
# drink or the meal was finished, and within half an hour the agent had
# stopped trying either.
RESTART_CHOICES = {"drink-eat": ((9, 2, -4.0), (9, 3, -4.0))}
# A restart can start a choice as a copy of another choice than the first,
# named fourth. The potions thrown with a turn broke a little short one time
# in four and healed 7 instead of 8, and the fleet let them die; with that
# mended they start over as the plain throw does in `COPY_OF`.
RESTART_CHOICES["turned-potions"] = ((9, 7, -2.0, 1), (9, 8, -2.0, 1), (9, 9, -2.0, 1))
# The pitch of a potion thrown with a turn and the run after it became the
# policy's own; the pitch is the last control, new, and starts even over its
# choices. The turned throws themselves keep what the fleet had learnt of
# them (two in three on carry servers): drills teach the pitch.


def start_bias(head, n):
    """Starting preferences of the `n` choices of a control, or None when
    they start evenly. Choices a control has gained since start rare."""
    b = START_BIAS.get(head)
    return None if b is None else (b + [NEW_CHOICE_BIAS] * n)[:n]


def restart_choices(policy, marks):
    """Start over the choices in `RESTART_CHOICES` that `marks` does not
    list yet, and list them. Returns the names of the restarts made."""
    made = []
    with torch.no_grad():
        for name, choices in RESTART_CHOICES.items():
            if name in marks:
                continue
            for head, choice, bias, *src in choices:
                src = src[0] if src else 0
                if len(policy.act_dims) > head and policy.act_dims[head] > choice:
                    policy.heads[head].weight[choice] = policy.heads[head].weight[src]
                    policy.heads[head].bias[choice] = policy.heads[head].bias[src] + bias
            marks.append(name)
            made.append(name)
    return made


def _init(layer, gain):
    nn.init.orthogonal_(layer.weight, gain)
    nn.init.zeros_(layer.bias)
    return layer


class Policy(nn.Module):
    def __init__(self, obs_dim, priv_dim, act_dims, hidden=256):
        super().__init__()
        self.act_dims = list(act_dims)
        self.hidden = hidden
        g = math.sqrt(2.0)
        self.enc = nn.Sequential(
            _init(nn.Linear(obs_dim, hidden), g), nn.ReLU(),
            _init(nn.Linear(hidden, hidden), g), nn.ReLU(),
        )
        self.lstm = nn.LSTM(hidden, hidden)
        self.heads = nn.ModuleList([_init(nn.Linear(hidden, d), 0.01) for d in self.act_dims])
        self.penc = nn.Sequential(_init(nn.Linear(priv_dim, 64), g), nn.ReLU())
        self.vf = nn.Sequential(
            _init(nn.Linear(hidden + 64, hidden), g), nn.ReLU(),
            _init(nn.Linear(hidden, 1), 1.0),
        )
        # Start with small mouse movements and few jumps, so early play is
        # not a blur of spinning and hopping.
        with torch.no_grad():
            for h in (5, 6):
                n = self.act_dims[h]
                c = (n - 1) / 2
                self.heads[h].bias.copy_(torch.tensor([-0.35 * abs(i - c) for i in range(n)]))
            self.heads[2].bias.copy_(torch.tensor([0.0, -1.7]))
            # Blocking costs clicks and speed and a pearl costs health, so
            # both start rare.
            for h in START_BIAS:
                if len(self.act_dims) > h:
                    self.heads[h].bias.copy_(torch.tensor(start_bias(h, self.act_dims[h])))

    def initial_state(self, n, device=None):
        z = torch.zeros(n, self.hidden, device=device)
        return (z, z.clone())

    def core(self, obs, state):
        """One time step. obs [B, obs_dim], state (h, c) each [B, hidden]."""
        x = self.enc(obs).unsqueeze(0)
        out, (h, c) = self.lstm(x, (state[0].unsqueeze(0), state[1].unsqueeze(0)))
        return out[0], (h[0], c[0])

    def core_seq(self, obs, state, done):
        """A whole sequence. obs [T, B, obs_dim], done [T, B] marks steps
        after which the memory restarts. The sequence is cut wherever any
        member restarts, so the LSTM runs over a few long segments."""
        x = self.enc(obs)
        T = x.shape[0]
        h, c = state[0].unsqueeze(0), state[1].unsqueeze(0)
        cuts = (done.any(dim=1).nonzero().squeeze(-1) + 1).tolist()
        outs, start = [], 0
        for end in cuts + [T]:
            if end > start:
                o, (h, c) = self.lstm(x[start:end], (h, c))
                outs.append(o)
            if end < T or end in cuts:
                keep = (~done[end - 1]).to(x.dtype).view(1, -1, 1)
                h, c = h * keep, c * keep
            start = end
        return torch.cat(outs, 0)

    def logits(self, h):
        return [head(h) for head in self.heads]

    # All the controls at once. The heads stay separate layers (the stored
    # weights and the mod read them so), but their weights are used as one
    # matrix, and the choices of every control are laid side by side in a
    # padded block [..., heads, widest], the padding so unlikely that it is
    # never picked and adds nothing to a sum. One matrix product, one
    # softmax and one draw for all twelve controls cost a tenth of twelve
    # of each through torch.distributions, which was most of the time it
    # took to play a tick.
    PAD = -1.0e9

    def _layout(self, device):
        key = (tuple(self.act_dims), device)
        if getattr(self, "_lay_key", None) != key:
            widest = max(self.act_dims)
            cols = torch.cat([torch.arange(d) + i * widest for i, d in enumerate(self.act_dims)]).to(device)
            self._lay = (widest, cols)
            self._lay_key = key
        return self._lay

    def _head_matrix(self, grad):
        """The weights and biases of every head stacked, for one product.
        Without gradients the stack is kept until any of them changes."""
        if grad:
            return (torch.cat([hd.weight for hd in self.heads], 0), torch.cat([hd.bias for hd in self.heads], 0))
        ver = tuple((hd.weight.data_ptr(), hd.weight._version, hd.bias.data_ptr(), hd.bias._version) for hd in self.heads)
        if getattr(self, "_hm_ver", None) != ver:
            with torch.no_grad():
                self._hm = (torch.cat([hd.weight for hd in self.heads], 0).detach(), torch.cat([hd.bias for hd in self.heads], 0).detach())
            self._hm_ver = ver
        return self._hm

    def padded_logp(self, h):
        """Log-probabilities of every choice of every control, [..., heads,
        widest], the padding at `PAD` or below."""
        w, b = self._head_matrix(torch.is_grad_enabled())
        flat = torch.nn.functional.linear(h, w, b)
        widest, cols = self._layout(flat.device)
        lead = flat.shape[:-1]
        H = len(self.act_dims)
        block = torch.full(lead + (H * widest,), self.PAD, dtype=flat.dtype, device=flat.device)
        block = block.index_copy(-1, cols, flat).view(lead + (H, widest))
        return torch.log_softmax(block, -1)

    def _segments(self, device):
        """For each choice, the control it belongs to; where each control's
        choices start; and the matrix that adds up each control's choices."""
        key = (tuple(self.act_dims), device)
        if getattr(self, "_seg_key", None) != key:
            dims = self.act_dims
            head_of = torch.cat([torch.full((d,), i, dtype=torch.long) for i, d in enumerate(dims)]).to(device)
            start = torch.tensor([sum(dims[:i]) for i in range(len(dims))], dtype=torch.long, device=device)
            sums = torch.nn.functional.one_hot(head_of, len(dims)).float()
            self._seg = (head_of, start, sums)
            self._seg_key = key
        return self._seg

    def flat_logp(self, h):
        """Log-probabilities of every choice of every control side by side,
        [..., all choices], the controls one after another in order: the
        numbers of `padded_logp` without its padding, which costs a third of
        the time to learn from. Each control's choices are shifted by their
        largest before the exponent, so that none overflows or vanishes; the
        shift cancels out and carries no gradient."""
        w, b = self._head_matrix(torch.is_grad_enabled())
        flat = torch.nn.functional.linear(h, w, b)
        head_of, _, sums = self._segments(flat.device)
        lead = flat.shape[:-1]
        x = flat.reshape(-1, flat.shape[-1])
        with torch.no_grad():
            top = torch.full((x.shape[0], len(self.act_dims)), -math.inf, dtype=x.dtype, device=x.device)
            top = top.scatter_reduce(-1, head_of.expand(x.shape[0], -1), x.detach(), "amax")
        shifted = x - top.index_select(-1, head_of)
        lse = torch.log(torch.exp(shifted) @ sums.to(x.dtype))
        return (shifted - lse.index_select(-1, head_of)).view(lead + (flat.shape[-1],))

    def value(self, h, priv):
        return self.vf(torch.cat([h, self.penc(priv)], dim=-1)).squeeze(-1)

    @torch.no_grad()
    def act(self, obs, priv, state, deterministic=False, per_head=False):
        """With `per_head`, the log-prob of each control apart [B, heads]."""
        h, state = self.core(obs, state)
        lp = self.padded_logp(h)
        if deterministic:
            a = lp.argmax(-1)
        else:
            # The largest of the log-probabilities each with a Gumbel draw
            # added is a draw from the categorical distribution.
            u = torch.rand_like(lp).clamp_(1e-20, 1.0)
            a = (lp - torch.log(-torch.log(u))).argmax(-1)
        logp = lp.gather(-1, a.unsqueeze(-1)).squeeze(-1)
        return a, logp if per_head else logp.sum(-1), self.value(h, priv), state

    def evaluate_seq(self, obs, priv, actions, state, done, per_head=False, teach=None):
        """Log-prob, entropy and value for stored sequences [T, B, ...];
        with `per_head`, the log-prob of each control apart [T, B, heads].
        With `teach` (choices like `actions`, -1 for none), the log-prob of
        those choices per control as well, 0 where there is none."""
        hs = self.core_seq(obs, state, done)
        lp = self.flat_logp(hs)
        _, start, sums = self._segments(lp.device)
        logp = lp.gather(-1, actions + start)
        # Entropy per control [T, B, heads] with `per_head`, summed otherwise.
        ent = -(lp.exp() * lp) @ sums.to(lp.dtype)
        if not per_head:
            ent = ent.sum(-1)
        out = (logp if per_head else logp.sum(-1), ent, self.value(hs, priv))
        if teach is None:
            return out
        tlogp = lp.gather(-1, teach.clamp(min=0) + start) * (teach >= 0)
        return out + (tlogp,)


def fit_state(sd, policy, still=False):
    """Stored weights made to fit a policy that has since gained inputs,
    controls or choices of a control. New inputs get zero weight, a new
    control depends on nothing yet and has its starting preference, and a
    new choice of an old control starts as a rare copy of the control's
    first choice, so the policy plays as before until it learns to use
    them. A control listed in `RESTART` for a
    control that is being added starts over as well. With `still`, for a
    version that is not trained any more, a new control stays at its first
    choice instead and new choices are never asked for. Returns the weights
    and whether anything was added."""
    out, grown = {}, False
    again = {h for new, hs in RESTART.items() if f"heads.{new}.bias" not in sd for h in hs}
    for k, ref in policy.state_dict().items():
        part = k.split(".")
        head = int(part[1]) if part[0] == "heads" else None
        if k not in sd or head in again:
            start = start_bias(head, ref.shape[0]) if head is not None and part[2] == "bias" else None
            if still and k not in sd and head is not None and part[2] == "bias":
                start = [0.0] + [NEVER] * (ref.shape[0] - 1)
            out[k] = torch.tensor(start, dtype=ref.dtype) if start is not None else torch.zeros_like(ref)
            grown = True
        elif sd[k].shape != ref.shape:
            w = torch.zeros_like(ref)
            old = sd[k].float()
            if head is not None and old.shape[0] < ref.shape[0] and still:
                # A version that is not trained any more never asks for them.
                if part[2] == "bias":
                    w.fill_(NEVER)
            elif head is not None and old.shape[0] < ref.shape[0]:
                # New choices of an old control: copies of its first choice,
                # rarer by their starting preference.
                if part[2] == "bias":
                    start = start_bias(head, ref.shape[0]) or [0.0] + [NEW_CHOICE_BIAS] * (ref.shape[0] - 1)
                    w.copy_(old[0] + torch.tensor(start, dtype=ref.dtype) - start[0])
                else:
                    w[:, : old.shape[1]] = old[0]
                for new, (src, bias) in COPY_OF.get(head, {}).items():
                    if old.shape[0] <= new < ref.shape[0] and src < old.shape[0]:
                        if part[2] == "bias":
                            w[new] = old[src] + bias
                        else:
                            w[new, : old.shape[1]] = old[src]
            w[tuple(slice(0, n) for n in old.shape)] = old
            out[k] = w
            grown = True
        else:
            out[k] = sd[k].float()
    return out, grown


def load_policy(path, dims=None, still=False):
    """The policy stored in a checkpoint, with the checkpoint itself and
    whether inputs or controls were added. `dims` is what the simulator
    now gives and takes (`pvpai.env.sim_dims()`); a checkpoint trained
    with less is grown to fit, see `fit_state`, which `still` is for."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    sd = {k: v.float() for k, v in ck["policy"].items()}
    hidden = sd["lstm.weight_hh_l0"].shape[1]
    obs, priv, acts = sd["enc.0.weight"].shape[1], ck["priv_dim"], list(ck["act_dims"])
    if dims is not None:
        obs, priv = max(obs, dims[0]), max(priv, dims[1])
        acts = [max(a, d) for a, d in zip(acts, dims[2])] + list(dims[2][len(acts):])
    policy = Policy(obs, priv, acts, hidden)
    sd, grown = fit_state(sd, policy, still)
    policy.load_state_dict(sd)
    return policy, ck, grown
