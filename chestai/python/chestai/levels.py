"""Prioritized Level Replay (Jiang et al., 2021) over the seeds of the
simulator's levels: a level is the seed a map, its chests, the mobs and
the server settings are drawn from. Levels where the agent's value was
furthest below what it then got (positive value loss) are played again
more often; staleness makes sure old scores get looked at again."""
import numpy as np


class LevelBuffer:
    def __init__(self, capacity=4000, beta=0.3, rho=0.3, replay_prob=0.5, min_fill=400, seed=0):
        self.cap, self.beta, self.rho, self.p_replay, self.min_fill = capacity, beta, rho, replay_prob, min_fill
        self.rng = np.random.default_rng(seed)
        self.seeds = np.zeros(capacity, np.uint64)
        self.score = np.zeros(capacity)
        self.last = np.zeros(capacity)
        self.n = 0
        self.index = {}
        self.clock = 0
        self.replays = 0
        self.news = 0

    def new_seed(self):
        return int(self.rng.integers(1, 2 ** 62))

    def sample(self):
        """The seed of a level to play next."""
        self.clock += 1
        if self.n < self.min_fill or self.rng.random() >= self.p_replay:
            self.news += 1
            return self.new_seed()
        n = self.n
        order = np.argsort(-self.score[:n])
        rank = np.empty(n)
        rank[order] = np.arange(1, n + 1)
        ps = (1.0 / rank) ** (1.0 / self.beta)
        ps /= ps.sum()
        st = self.clock - self.last[:n]
        pc = st / st.sum() if st.sum() > 0 else np.full(n, 1.0 / n)
        p = (1 - self.rho) * ps + self.rho * pc
        i = int(self.rng.choice(n, p=p / p.sum()))
        self.last[i] = self.clock
        self.replays += 1
        return int(self.seeds[i])

    def update(self, seed, score):
        i = self.index.get(seed)
        if i is None:
            if self.n < self.cap:
                i = self.n
                self.n += 1
            else:
                # The level with the lowest score makes room.
                i = int(np.argmin(self.score[:self.n]))
                del self.index[int(self.seeds[i])]
            self.index[seed] = i
            self.seeds[i] = seed
        self.score[i] = score
        self.last[i] = self.clock

    def report(self):
        r = {"plr_levels": self.n, "plr_mean": round(float(self.score[:self.n].mean()) if self.n else 0.0, 4),
             "plr_replay_share": round(self.replays / max(self.replays + self.news, 1), 3)}
        self.replays = self.news = 0
        return r
