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
# within a duel, so that some are thrown while hurt and pay off.
POTION_BIAS = [0.0, -4.5]
# Controls that do not start evenly, by their position among the controls.
START_BIAS = {7: USE_BIAS, 8: PEARL_BIAS, 9: POTION_BIAS}
# Where a choice added to a control that already exists starts: rare, so
# that the policy plays as before until it finds a use for it.
NEW_CHOICE_BIAS = -5.0
# When the potion control is added, the pearl control starts over. With
# infinite health the agent had learnt never to throw a pearl; with health
# to lose, and an opponent that can be knocked away from its own potion, a
# pearl is worth something else, and it has to try them again to find out.
RESTART = {9: (8,)}


def start_bias(head, n):
    """Starting preferences of the `n` choices of a control, or None when
    they start evenly. Choices a control has gained since start rare."""
    b = START_BIAS.get(head)
    return None if b is None else (b + [NEW_CHOICE_BIAS] * n)[:n]


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

    def value(self, h, priv):
        return self.vf(torch.cat([h, self.penc(priv)], dim=-1)).squeeze(-1)

    @torch.no_grad()
    def act(self, obs, priv, state, deterministic=False):
        h, state = self.core(obs, state)
        acts, logp = [], 0.0
        for lg in self.logits(h):
            d = torch.distributions.Categorical(logits=lg)
            a = lg.argmax(-1) if deterministic else d.sample()
            acts.append(a)
            logp = logp + d.log_prob(a)
        return torch.stack(acts, -1), logp, self.value(h, priv), state

    def evaluate_seq(self, obs, priv, actions, state, done):
        """Log-prob, entropy and value for stored sequences [T, B, ...]."""
        hs = self.core_seq(obs, state, done)
        logp, ent = 0.0, 0.0
        for i, lg in enumerate(self.logits(hs)):
            d = torch.distributions.Categorical(logits=lg)
            logp = logp + d.log_prob(actions[..., i])
            ent = ent + d.entropy()
        return logp, ent, self.value(hs, priv)


def fit_state(sd, policy):
    """Stored weights made to fit a policy that has since gained inputs,
    controls or choices of a control. New inputs get zero weight, a new
    control depends on nothing yet and has its starting preference, and a
    new choice of an old control starts rare, so the policy plays as before
    until it learns to use them. A control listed in `RESTART` for a
    control that is being added starts over as well. Returns the weights
    and whether anything was added."""
    out, grown = {}, False
    again = {h for new, hs in RESTART.items() if f"heads.{new}.bias" not in sd for h in hs}
    for k, ref in policy.state_dict().items():
        part = k.split(".")
        head = int(part[1]) if part[0] == "heads" else None
        if k not in sd or head in again:
            start = start_bias(head, ref.shape[0]) if head is not None and part[2] == "bias" else None
            out[k] = torch.tensor(start, dtype=ref.dtype) if start is not None else torch.zeros_like(ref)
            grown = True
        elif sd[k].shape != ref.shape:
            w = torch.zeros_like(ref)
            if head is not None and part[2] == "bias":
                w.fill_(NEW_CHOICE_BIAS)
            old = sd[k].float()
            w[tuple(slice(0, n) for n in old.shape)] = old
            out[k] = w
            grown = True
        else:
            out[k] = sd[k].float()
    return out, grown


def load_policy(path, dims=None):
    """The policy stored in a checkpoint, with the checkpoint itself and
    whether inputs or controls were added. `dims` is what the simulator
    now gives and takes (`pvpai.env.sim_dims()`); a checkpoint trained
    with less is grown to fit, see `fit_state`."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    sd = {k: v.float() for k, v in ck["policy"].items()}
    hidden = sd["lstm.weight_hh_l0"].shape[1]
    obs, priv, acts = sd["enc.0.weight"].shape[1], ck["priv_dim"], list(ck["act_dims"])
    if dims is not None:
        obs, priv = max(obs, dims[0]), max(priv, dims[1])
        acts = [max(a, d) for a, d in zip(acts, dims[2])] + list(dims[2][len(acts):])
    policy = Policy(obs, priv, acts, hidden)
    sd, grown = fit_state(sd, policy)
    policy.load_state_dict(sd)
    return policy, ck, grown
