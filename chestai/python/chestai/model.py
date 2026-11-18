"""Policy of the chest farmer. Not the PvP network: the mobs, the chests and
the projectiles are lists of things, so each kind gets its own encoder,
shared by every member of the list, followed by attention from the
player's own state over the list and a pool (as the unit encoders of
AlphaStar and OpenAI Five do). The terrain patch goes through a small
convolution. A recurrent core keeps the memory between ticks."""
import math
import torch
import torch.nn as nn
import torch.nn.functional as F

from .env import K_MOBS, K_CHESTS, K_PROJ, SELF_DIM, TERRAIN_DIM, MOB_DIM, CHEST_DIM, PROJ_DIM, OWN_DIM

GRID = 9


def _init(layer, gain):
    nn.init.orthogonal_(layer.weight, gain)
    nn.init.zeros_(layer.bias)
    return layer


def split(obs):
    """The parts of a batch of observations [..., obs_dim]."""
    i = 0
    out = {}
    for name, w in [("self", SELF_DIM), ("terrain", TERRAIN_DIM), ("mobs", K_MOBS * MOB_DIM), ("chests", K_CHESTS * CHEST_DIM), ("projs", K_PROJ * PROJ_DIM), ("own", OWN_DIM)]:
        out[name] = obs[..., i:i + w]
        i += w
    lead = obs.shape[:-1]
    out["mobs"] = out["mobs"].reshape(*lead, K_MOBS, MOB_DIM)
    out["chests"] = out["chests"].reshape(*lead, K_CHESTS, CHEST_DIM)
    out["projs"] = out["projs"].reshape(*lead, K_PROJ, PROJ_DIM)
    return out


class Entities(nn.Module):
    """Shared encoder of the members of a list, attention from a query, and
    a masked max pool. Absent members (first feature 0) are left out."""

    def __init__(self, d_in, d, d_q):
        super().__init__()
        g = math.sqrt(2.0)
        self.enc = nn.Sequential(_init(nn.Linear(d_in, d), g), nn.ReLU(), _init(nn.Linear(d, d), g), nn.ReLU())
        self.q = _init(nn.Linear(d_q, d), 1.0)
        self.k = _init(nn.Linear(d, d), 1.0)
        self.v = _init(nn.Linear(d, d), 1.0)
        self.d = d

    def forward(self, x, query):
        present = x[..., 0] > 0.5
        e = self.enc(x)
        q = self.q(query).unsqueeze(-2)
        att = (q * self.k(e)).sum(-1) / math.sqrt(self.d)
        att = att.masked_fill(~present, -1e9)
        w = torch.softmax(att, -1) * present.float()
        attended = (w.unsqueeze(-1) * self.v(e)).sum(-2)
        pooled = e.masked_fill(~present.unsqueeze(-1), -1e4).max(-2).values
        pooled = torch.where(present.any(-1, keepdim=True), pooled, torch.zeros_like(pooled))
        return torch.cat([attended, pooled], -1)


class Policy(nn.Module):
    def __init__(self, obs_dim, priv_dim, act_dims, hidden=256):
        super().__init__()
        self.act_dims = list(act_dims)
        self.hidden = hidden
        self.obs_dim = obs_dim
        g = math.sqrt(2.0)
        self.self_enc = nn.Sequential(_init(nn.Linear(SELF_DIM + OWN_DIM, 128), g), nn.ReLU())
        self.terrain = nn.Sequential(
            nn.Conv2d(2, 16, 3, padding=1), nn.ReLU(),
            nn.Conv2d(16, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Flatten(), _init(nn.Linear(16 * 5 * 5, 64), g), nn.ReLU(),
        )
        self.mobs = Entities(MOB_DIM, 64, 128)
        self.chests = Entities(CHEST_DIM, 64, 128)
        self.projs = Entities(PROJ_DIM, 32, 128)
        d = 128 + 64 + 128 + 128 + 64
        self.trunk = nn.Sequential(_init(nn.Linear(d, hidden), g), nn.ReLU())
        self.lstm = nn.LSTM(hidden, hidden)
        self.post = nn.Sequential(_init(nn.Linear(hidden + hidden, hidden), g), nn.ReLU())
        self.heads = nn.ModuleList([_init(nn.Linear(hidden, n), 0.01) for n in self.act_dims])
        self.penc = nn.Sequential(_init(nn.Linear(priv_dim, 64), g), nn.ReLU())
        self.vf = nn.Sequential(_init(nn.Linear(hidden + 64, hidden), g), nn.ReLU(), _init(nn.Linear(hidden, 1), 1.0))
        with torch.no_grad():
            # Small mouse moves to start with, and rare item use.
            for h in (4, 5):
                n = self.act_dims[h]
                c = (n - 1) / 2
                self.heads[h].bias.copy_(torch.tensor([-0.3 * abs(i - c) for i in range(n)]))
            self.heads[0].bias.copy_(torch.tensor([0.0, 0.3, 0.8]))
            self.heads[3].bias.copy_(torch.tensor([0.0, 1.0]))
            self.heads[7].bias.copy_(torch.tensor([0.0, -2.0]))
            self.heads[8].bias.copy_(torch.tensor([2.0, 0.0, -1.0, -1.0, -3.0, -3.0]))

    def initial_state(self, n, device=None):
        z = torch.zeros(n, self.hidden, device=device)
        return (z, z.clone())

    def encode(self, obs):
        p = split(obs)
        lead = obs.shape[:-1]
        s = self.self_enc(torch.cat([p["self"], p["own"]], -1))
        t = p["terrain"].reshape(-1, GRID, GRID, 2).permute(0, 3, 1, 2)
        t = self.terrain(t).reshape(*lead, -1)
        m = self.mobs(p["mobs"], s)
        c = self.chests(p["chests"], s)
        pr = self.projs(p["projs"], s)
        return self.trunk(torch.cat([s, t, m, c, pr], -1))

    def core(self, obs, state):
        x = self.encode(obs)
        out, (h, c) = self.lstm(x.unsqueeze(0), (state[0].unsqueeze(0), state[1].unsqueeze(0)))
        return self.post(torch.cat([out[0], x], -1)), (h[0], c[0])

    def core_seq(self, obs, state, done):
        x = self.encode(obs)
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
        return self.post(torch.cat([torch.cat(outs, 0), x], -1))

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

    def evaluate_seq(self, obs, priv, actions, state, done, with_logits=False):
        hs = self.core_seq(obs, state, done)
        logp, ent = 0.0, 0.0
        lgs = self.logits(hs)
        for i, lg in enumerate(lgs):
            d = torch.distributions.Categorical(logits=lg)
            logp = logp + d.log_prob(actions[..., i])
            ent = ent + d.entropy()
        if with_logits:
            return logp, ent, self.value(hs, priv), lgs
        return logp, ent, self.value(hs, priv)


def load_policy(path):
    ck = torch.load(path, map_location="cpu", weights_only=False)
    pol = Policy(ck["obs_dim"], ck["priv_dim"], ck["act_dims"], ck.get("hidden", 256))
    pol.load_state_dict({k: v.float() for k, v in ck["policy"].items()})
    return pol, ck
