"""Recurrent policy with one categorical head per control, and a value
head that also sees simulator-only information during training."""
import math
import torch
import torch.nn as nn


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


def load_policy(path, obs_dim=None):
    """The policy stored in a checkpoint, with the checkpoint itself and
    whether inputs were added. If the simulator now gives more observation
    values than the checkpoint was trained with, the new ones start with
    zero weight: the policy plays as before until it learns to use them."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    sd = {k: v.float() for k, v in ck["policy"].items()}
    hidden = sd["lstm.weight_hh_l0"].shape[1]
    old = sd["enc.0.weight"].shape[1]
    n = max(obs_dim or old, old)
    if n > old:
        w = torch.zeros(hidden, n)
        w[:, :old] = sd["enc.0.weight"]
        sd["enc.0.weight"] = w
    policy = Policy(n, ck["priv_dim"], ck["act_dims"], hidden)
    policy.load_state_dict(sd)
    return policy, ck, n > old
