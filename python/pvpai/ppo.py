"""PPO with a recurrent policy over several streams of experience.

A stream is a set of (duel, player) pairs that act on the same call: player
0 of the self-play duels, player 1 of the self-play duels, and player 0 of
the duels against scripted bots. Each stream keeps one action waiting for
its reward; when the reward arrives the step is stored and the next action
is chosen. That keeps the two self-play players, whose turns interleave,
in ordinary fixed-length sequences."""
import numpy as np
import torch

# The pitch of a throw with a turn, and the input that counts down from the
# tick of the turn (1 on that tick).
PITCH_HEAD = 11
TURN_OBS = 152


class Stream:
    def __init__(self, name, n, obs_dim, priv_dim, heads, horizon, hidden, learn=True):
        self.name, self.n, self.T, self.learn = name, n, horizon, learn
        self.obs = torch.zeros(horizon, n, obs_dim)
        self.priv = torch.zeros(horizon, n, priv_dim)
        self.act = torch.zeros(horizon, n, heads, dtype=torch.long)
        self.logp = torch.zeros(horizon, n, heads)
        self.val = torch.zeros(horizon, n)
        self.rew = torch.zeros(horizon, n)
        self.done = torch.zeros(horizon, n, dtype=torch.bool)
        # 1 for each control whose choice stored is the one that was played,
        # 0 where others laid their keys over it or a drill changed it: those
        # choices teach the value, not the policy.
        self.own = torch.ones(horizon, n, heads)
        # What a teacher would have chosen, -1 where it has nothing to say.
        self.teach = torch.full((horizon, n, heads), -1, dtype=torch.long)
        self.pend_teach = None
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
        act, logp, val, self.state = policy.act(obs, priv, before, per_head=True)
        self.pend = (obs, priv, act, logp, val, before)
        self.pend_own = torch.ones(self.n, act.shape[-1])
        self.pend_teach = None
        return act.numpy().astype(np.int32)

    def taught(self, teach_np):
        """What a teacher would have chosen in place of the pending actions."""
        self.pend_teach = torch.from_numpy(teach_np.astype(np.int64))

    def disown(self, overridden_np):
        """Mark the pending actions whose keys others laid over."""
        self.pend_own[torch.from_numpy(overridden_np != 0)] = 0.0

    def drill(self, act_np, prob, rng, head=9, asked=1, played=7):
        """Play some of the pending potions asked for as throws with a turn
        instead, to practise them: the policy still picks the pitch and
        everything around the throw, but the choice of throw on those steps
        is not its own and does not teach it."""
        rows = (act_np[:, head] == asked) & (rng.random(self.n) < prob)
        act_np[rows, head] = played
        self.pend_own[torch.from_numpy(rows), head] = 0.0
        return act_np

    def close(self, policy, rew_np, done_np, fobs_np, fpriv_np, gamma, vscale, nobs_np=None):
        """Store the pending step now that its reward and done flag are known.
        `nobs_np`, the view that follows the step, tells whether it turned
        for a throw: the pitch chosen for such a throw counts on that step
        only, and is left out of what the policy learns on the others."""
        if self.pend is None:
            return
        obs, priv, act, logp, val, before = self.pend
        if nobs_np is not None and self.pend_own.shape[-1] > PITCH_HEAD:
            after = np.where((done_np != 0)[:, None], fobs_np, nobs_np)
            self.pend_own[torch.from_numpy(after[:, TURN_OBS] != 1.0), PITCH_HEAD] = 0.0
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
            self.own[t] = self.pend_own
            self.teach[t] = -1 if self.pend_teach is None else self.pend_teach
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


def choose_together(policy, picks):
    """`Stream.choose` for several streams, [(stream, obs, priv), ...], in
    one pass of the network: the same kind of choices at a fraction of the
    cost of a pass for each. Returns each stream's actions."""
    if not picks:
        return []
    obs = [torch.from_numpy(o).clone() for _, o, _ in picks]
    priv = [torch.from_numpy(p).clone() for _, _, p in picks]
    before = [s.state for s, _, _ in picks]
    state = (torch.cat([b[0] for b in before]), torch.cat([b[1] for b in before]))
    act, logp, val, after = policy.act(torch.cat(obs), torch.cat(priv), state, per_head=True)
    out, i = [], 0
    for (s, _, _), o, p, b in zip(picks, obs, priv, before):
        j = i + s.n
        s.state = (after[0][i:j], after[1][i:j])
        s.pend = (o, p, act[i:j], logp[i:j], val[i:j], b)
        s.pend_own = torch.ones(s.n, act.shape[-1])
        s.pend_teach = None
        out.append(act[i:j].numpy().astype(np.int32))
        i = j
    return out


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
    own = cat([s.own for s, _, _ in parts])
    teach = cat([s.teach for s, _, _ in parts])
    teach_coef = cfg.get("teach_coef", 0.0)
    use_teach = teach_coef > 0 and bool((teach >= 0).any())
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
            got = policy.evaluate_seq(obs[:, mb], priv[:, mb], act[:, mb], (h0[mb], c0[mb]), done[:, mb], per_head=True,
                                      teach=teach[:, mb] if use_teach else None)
            logp_h, ent, v = got[:3]
            o = own[:, mb]
            logp = (logp_h * o).sum(-1)
            ratio = torch.exp(logp - (logp_old[:, mb] * o).sum(-1))
            a, w = adv[:, mb], (o.sum(-1) > 0).float()
            surr = torch.min(ratio * a, torch.clamp(ratio, 1 - cfg["clip"], 1 + cfg["clip"]) * a)
            pi_loss = -(surr * w).sum() / w.sum().clamp(min=1.0)
            v_loss = 0.5 * (v - ret_n[:, mb]).pow(2).mean()
            entropy = ent.mean()
            loss = pi_loss + cfg["vf_coef"] * v_loss - cfg["ent_coef"] * entropy
            if use_teach:
                # Imitation of the teacher on the choices it has an opinion
                # on, on the steps that were the policy's own.
                tm = ((teach[:, mb] >= 0) & (o.sum(-1, keepdim=True) > 0)).float()
                teach_loss = -(got[3] * tm).sum() / tm.sum().clamp(min=1.0)
                loss = loss + teach_coef * teach_loss
                stats["teach_loss"] = stats.get("teach_loss", 0.0) + float(teach_loss)
            opt.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(policy.parameters(), cfg["max_grad_norm"])
            opt.step()
            with torch.no_grad():
                stats["pi_loss"] += float(pi_loss)
                stats["v_loss"] += float(v_loss)
                stats["entropy"] += float(entropy)
                stats["kl"] += float(((logp_old[:, mb] * o).sum(-1) - logp).mean())
                stats["clip_frac"] += float(((ratio - 1).abs() > cfg["clip"]).float().mean())
            n_upd += 1
    out = {k: v / max(n_upd, 1) for k, v in stats.items()}
    out["samples"] = T * B
    out["ret_scale"] = scale
    out["mean_step_reward"] = float(cat([s.rew for s, _, _ in parts]).mean())
    return out
