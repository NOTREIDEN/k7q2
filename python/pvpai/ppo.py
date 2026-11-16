"""PPO with a recurrent policy over several streams of experience.

A stream is a set of (duel, player) pairs that act on the same call: player
0 of the self-play duels, player 1 of the self-play duels, and player 0 of
the duels against scripted bots. Each stream keeps one action waiting for
its reward; when the reward arrives the step is stored and the next action
is chosen. That keeps the two self-play players, whose turns interleave,
in ordinary fixed-length sequences."""
import numpy as np
import torch


class Stream:
    def __init__(self, name, n, obs_dim, priv_dim, heads, horizon, hidden, learn=True):
        self.name, self.n, self.T, self.learn = name, n, horizon, learn
        self.obs = torch.zeros(horizon, n, obs_dim)
        self.priv = torch.zeros(horizon, n, priv_dim)
        self.act = torch.zeros(horizon, n, heads, dtype=torch.long)
        self.logp = torch.zeros(horizon, n)
        self.val = torch.zeros(horizon, n)
        self.rew = torch.zeros(horizon, n)
        self.done = torch.zeros(horizon, n, dtype=torch.bool)
        self.h0 = torch.zeros(n, hidden)
        self.c0 = torch.zeros(n, hidden)
        self.state = (torch.zeros(n, hidden), torch.zeros(n, hidden))
        self.pend = None
        self.t = 0

    def choose(self, policy, obs_np, priv_np):
        """Pick the next action for every member and keep it pending."""
        obs = torch.from_numpy(obs_np).clone()
        priv = torch.from_numpy(priv_np).clone()
        before = self.state
        act, logp, val, self.state = policy.act(obs, priv, before)
        self.pend = (obs, priv, act, logp, val, before)
        return act.numpy().astype(np.int32)

    def close(self, policy, rew_np, done_np, fobs_np, fpriv_np, gamma, vscale):
        """Store the pending step now that its reward and done flag are known."""
        if self.pend is None:
            return
        obs, priv, act, logp, val, before = self.pend
        rew = torch.from_numpy(rew_np).clone()
        done = torch.from_numpy(done_np != 0)
        if done.any():
            idx = done.nonzero().squeeze(-1)
            h, c = self.state
            trunc = torch.from_numpy(done_np[idx.numpy()] == 2)
            if trunc.any():
                # The episode was cut by the clock, not ended by the game:
                # count what the value function expects from there on.
                with torch.no_grad():
                    hh, _ = policy.core(torch.from_numpy(fobs_np[idx.numpy()]), (h[idx], c[idx]))
                    v = policy.value(hh, torch.from_numpy(fpriv_np[idx.numpy()])) * vscale
                rew[idx] += gamma * v * trunc
            h, c = h.clone(), c.clone()
            h[idx] = 0
            c[idx] = 0
            self.state = (h, c)
        if self.t < self.T:
            t = self.t
            if t == 0:
                self.h0, self.c0 = before[0].clone(), before[1].clone()
            self.obs[t], self.priv[t], self.act[t] = obs, priv, act
            self.logp[t], self.val[t], self.rew[t], self.done[t] = logp, val, rew, done
            self.t += 1
        self.pend = None

    def full(self):
        return self.t >= self.T

    def gae(self, gamma, lam, vscale):
        """Advantages and returns; the pending step supplies the last value."""
        T = self.T
        val = self.val * vscale
        last = self.pend[4] * vscale
        adv = torch.zeros(T, self.n)
        run = torch.zeros(self.n)
        for t in reversed(range(T)):
            nxt = last if t == T - 1 else val[t + 1]
            live = (~self.done[t]).float()
            delta = self.rew[t] + gamma * nxt * live - val[t]
            run = delta + gamma * lam * live * run
            adv[t] = run
        self.t = 0
        return adv, adv + val


class RunningScale:
    """Running standard deviation of returns, used to keep value targets
    near unit size."""

    def __init__(self):
        self.var, self.count = 1.0, 1e-4

    def update(self, x):
        b = x.numel()
        v = float(x.var()) + float(x.mean()) ** 2
        tot = self.count + b
        self.var = (self.var * self.count + v * b) / tot
        self.count = min(tot, 5e6)

    @property
    def scale(self):
        return max(self.var ** 0.5, 0.05)


def ppo_update(policy, opt, streams, vs, cfg):
    gamma, lam = cfg["gamma"], cfg["lam"]
    scale = vs.scale
    parts = [(s, *s.gae(gamma, lam, scale)) for s in streams if s.learn]
    cat = lambda xs, d=1: torch.cat(xs, d)
    obs = cat([s.obs for s, _, _ in parts])
    priv = cat([s.priv for s, _, _ in parts])
    act = cat([s.act for s, _, _ in parts])
    logp_old = cat([s.logp for s, _, _ in parts])
    done = cat([s.done for s, _, _ in parts])
    adv = cat([a for _, a, _ in parts])
    ret = cat([r for _, _, r in parts])
    h0 = cat([s.h0 for s, _, _ in parts], 0)
    c0 = cat([s.c0 for s, _, _ in parts], 0)
    vs.update(ret)
    scale = vs.scale
    ret_n = ret / scale
    adv = (adv - adv.mean()) / (adv.std() + 1e-8)
    T, B = obs.shape[0], obs.shape[1]
    stats = {"pi_loss": 0.0, "v_loss": 0.0, "entropy": 0.0, "kl": 0.0, "clip_frac": 0.0}
    n_upd = 0
    for _ in range(cfg["epochs"]):
        perm = torch.randperm(B)
        for mb in perm.chunk(cfg["minibatches"]):
            logp, ent, v = policy.evaluate_seq(obs[:, mb], priv[:, mb], act[:, mb], (h0[mb], c0[mb]), done[:, mb])
            ratio = torch.exp(logp - logp_old[:, mb])
            a = adv[:, mb]
            pi_loss = -torch.min(ratio * a, torch.clamp(ratio, 1 - cfg["clip"], 1 + cfg["clip"]) * a).mean()
            v_loss = 0.5 * (v - ret_n[:, mb]).pow(2).mean()
            entropy = ent.mean()
            loss = pi_loss + cfg["vf_coef"] * v_loss - cfg["ent_coef"] * entropy
            opt.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(policy.parameters(), cfg["max_grad_norm"])
            opt.step()
            with torch.no_grad():
                stats["pi_loss"] += float(pi_loss)
                stats["v_loss"] += float(v_loss)
                stats["entropy"] += float(entropy)
                stats["kl"] += float((logp_old[:, mb] - logp).mean())
                stats["clip_frac"] += float(((ratio - 1).abs() > cfg["clip"]).float().mean())
            n_upd += 1
    out = {k: v / max(n_upd, 1) for k, v in stats.items()}
    out["samples"] = T * B
    out["ret_scale"] = scale
    out["mean_step_reward"] = float(cat([s.rew for s, _, _ in parts]).mean())
    return out
