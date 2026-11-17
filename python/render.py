"""Draw duels recorded with record.py as a video.

    python render.py duel.npz --out duel.mp4
    python render.py vs_bot.npz vs_self.npz --out both.mp4

The picture is drawn from the simulator's own state: positions, view angles,
keys and hits. It is not footage of the game. The main view follows both
players from the side; the inset shows what the first player sees, with the
other player where its client currently shows them (late by ping and
interpolation)."""
import argparse
import math
import subprocess

import numpy as np
from PIL import Image, ImageDraw, ImageFont

FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
SKY_TOP, SKY_BOTTOM = (108, 158, 232), (188, 214, 246)
GRASS_A, GRASS_B = (104, 168, 76), (92, 152, 66)
WALL = (128, 128, 134)
SKIN, HAIR, EYE = (226, 176, 134), (70, 46, 26), (40, 40, 60)
SWORD = (118, 232, 220)
PEARL = (22, 124, 108)
PORTAL = (176, 92, 224)
TEAM = [
    {"body": (58, 118, 224), "legs": (36, 48, 104), "ui": (96, 160, 255)},
    {"body": (214, 62, 52), "legs": (96, 40, 40), "ui": (255, 110, 96)},
]
LIGHT = np.array([0.35, 0.85, 0.4])
LIGHT = LIGHT / np.linalg.norm(LIGHT)
PX = 1.8 / 32.0  # blocks per model pixel


def font(size):
    try:
        return ImageFont.truetype(FONT, size)
    except OSError:
        return ImageFont.load_default(size)


class Camera:
    def __init__(self, pos, target, fov_deg, w, h):
        f = np.asarray(target, float) - np.asarray(pos, float)
        f /= np.linalg.norm(f)
        r = np.cross(f, [0.0, 1.0, 0.0])
        r /= np.linalg.norm(r)
        self.pos, self.f, self.r, self.u = np.asarray(pos, float), f, r, np.cross(r, f)
        self.w, self.h = w, h
        self.focal = (h / 2) / math.tan(math.radians(fov_deg) / 2)

    def to_cam(self, pts):
        d = np.asarray(pts, float) - self.pos
        return np.stack([d @ self.r, d @ self.u, d @ self.f], -1)

    def project(self, cam):
        z = cam[..., 2]
        return np.stack([self.w / 2 + self.focal * cam[..., 0] / z, self.h / 2 - self.focal * cam[..., 1] / z], -1)

    def polygon(self, draw, pts, color, near=0.05):
        """Fill a convex polygon given in world space, cut at the near plane."""
        cam = self.to_cam(pts)
        if (cam[:, 2] < near).any():
            cam = clip_near(cam, near)
            if len(cam) < 3:
                return
        p = self.project(cam)
        if p[:, 0].max() < 0 or p[:, 0].min() > self.w or p[:, 1].max() < 0 or p[:, 1].min() > self.h:
            return
        draw.polygon([tuple(q) for q in p], fill=color)


def clip_near(cam, near):
    out = []
    n = len(cam)
    for i in range(n):
        a, b = cam[i], cam[(i + 1) % n]
        ina, inb = a[2] >= near, b[2] >= near
        if ina:
            out.append(a)
        if ina != inb:
            t = (near - a[2]) / (b[2] - a[2])
            out.append(a + t * (b - a))
    return np.array(out) if out else np.zeros((0, 3))


def shade(color, normal, hurt):
    k = 0.62 + 0.38 * max(float(normal @ LIGHT), 0.0)
    c = np.array(color, float) * k
    if hurt:
        c = 0.62 * c + 0.38 * np.array([255, 40, 32])
    return tuple(int(v) for v in np.clip(c, 0, 255))


# Faces of a unit box: corner indices and outward normal.
FACES = [
    ((0, 1, 3, 2), (-1, 0, 0)), ((4, 6, 7, 5), (1, 0, 0)),
    ((0, 4, 5, 1), (0, -1, 0)), ((2, 3, 7, 6), (0, 1, 0)),
    ((0, 2, 6, 4), (0, 0, -1)), ((1, 5, 7, 3), (0, 0, 1)),
]


def box(lo, hi):
    return np.array([[x, y, z] for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])], float)


def rot_x(pts, pivot, ang):
    """Rotate about the model's left-right axis around `pivot` (positive tilts the top forward)."""
    c, s = math.cos(ang), math.sin(ang)
    d = pts - pivot
    y = d[:, 1] * c - d[:, 2] * s
    z = d[:, 1] * s + d[:, 2] * c
    return np.stack([d[:, 0], y, z], 1) + pivot


def cube_quads(centre, size, color):
    """World-space quads of a small cube, for a pearl in flight."""
    c, r = np.asarray(centre, float), size / 2
    w = box(c - r, c + r)
    return [(w[list(idx)], np.array(n, float), shade(color, np.array(n, float), False)) for idx, n in FACES]


def player_quads(pos, yaw, pitch, walk, amp, swing, team, hurt, block=False, pearl=False):
    """World-space quads (corners, normal, colour) of one blocky player.
    With `block` the sword is held upright in front of the body; with
    `pearl` the hand holds a pearl instead of the sword."""
    cy, sy = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))

    def to_world(p):
        p = p * PX
        return np.stack([p[:, 0] * cy - p[:, 2] * sy, p[:, 1], p[:, 0] * sy + p[:, 2] * cy], 1) + pos

    def nrm(n, ang):
        n = rot_x(np.array([n], float), np.zeros(3), ang)[0]
        return np.array([n[0] * cy - n[2] * sy, n[1], n[0] * sy + n[2] * cy])

    leg = math.sin(walk) * amp * 0.9
    arm_swing = -math.sin(swing * math.pi) * 1.7 if swing is not None else 0.0
    head = math.radians(max(-80.0, min(80.0, pitch)))
    arm = -leg * 0.8 - 0.35 + arm_swing
    sword = (box((-6.6, 10.5, 1), (-5.4, 12.5, 17)), (0, 22, 0), arm, [SWORD] * 6)
    if pearl:
        sword = (box((-7.6, 9.2, 0.4), (-4.4, 12.4, 3.6)), (0, 22, 0), arm, [PEARL] * 6)
    elif block:
        arm = -1.15
        sword = (box((-5.2, 13, 7.2), (-0.8, 31, 8.4)), (0, 22, 0), 0.0, [SWORD] * 6)
    parts = [
        (box((0, 0, -2), (4, 12, 2)), (0, 12, 0), -leg, [team["legs"]] * 6),
        (box((-4, 0, -2), (0, 12, 2)), (0, 12, 0), leg, [team["legs"]] * 6),
        (box((-4, 12, -2), (4, 24, 2)), (0, 12, 0), 0.0, [team["body"]] * 6),
        (box((4, 12, -2), (8, 24, 2)), (0, 22, 0), leg * 0.8, [team["body"]] * 6),
        (box((-8, 12, -2), (-4, 24, 2)), (0, 22, 0), arm, [team["body"]] * 6),
        sword,
        (box((-4, 24, -4), (4, 32, 4)), (0, 24, 0), -head, [SKIN, SKIN, SKIN, HAIR, HAIR, SKIN]),
        (box((1, 27.5, 4.05), (3, 29.5, 4.1)), (0, 24, 0), -head, [EYE] * 6),
        (box((-3, 27.5, 4.05), (-1, 29.5, 4.1)), (0, 24, 0), -head, [EYE] * 6),
    ]
    quads = []
    for corners, pivot, ang, colors in parts:
        if ang:
            corners = rot_x(corners, np.array(pivot, float), -ang)
        w = to_world(corners)
        for (idx, n), col in zip(FACES, colors):
            wn = nrm(n, -ang) if ang else np.array([n[0] * cy - n[2] * sy, n[1], n[0] * sy + n[2] * cy])
            quads.append((w[list(idx)], wn, shade(col, wn, hurt)))
    return quads


def draw_scene(img, cam, players, half, floor_y, skip=None, pearls=(), marks=()):
    """`pearls` are positions of pearls in flight; `marks` are (position,
    strength 0..1) of places a pearl has just put a player."""
    d = ImageDraw.Draw(img)
    w, h = img.size
    # Sky.
    for i in range(0, h, 4):
        t = i / h
        d.rectangle([0, i, w, i + 4], fill=tuple(int(a + (b - a) * t) for a, b in zip(SKY_TOP, SKY_BOTTOM)))
    # Walls, as panels two blocks wide.
    top = floor_y + 5
    for side in range(4):
        for k in range(int(-half), int(half), 2):
            a, b = float(k), float(k + 2)
            if side == 0:
                q = [(a, floor_y, -half), (b, floor_y, -half), (b, top, -half), (a, top, -half)]
            elif side == 1:
                q = [(a, floor_y, half), (b, floor_y, half), (b, top, half), (a, top, half)]
            elif side == 2:
                q = [(-half, floor_y, a), (-half, floor_y, b), (-half, top, b), (-half, top, a)]
            else:
                q = [(half, floor_y, a), (half, floor_y, b), (half, top, b), (half, top, a)]
            shade_k = 0.86 if (k // 2 + side) % 2 else 1.0
            cam.polygon(d, np.array(q), tuple(int(c * shade_k) for c in WALL))
    # Floor: one base polygon, then the darker tiles of a checkerboard near the action.
    cam.polygon(d, np.array([(-half, floor_y, -half), (half, floor_y, -half), (half, floor_y, half), (-half, floor_y, half)]), GRASS_A)
    cx, cz = cam.pos[0] + cam.f[0] * 8, cam.pos[2] + cam.f[2] * 8
    r = 20
    for ix in range(int(max(-half, cx - r)), int(min(half, cx + r))):
        for iz in range(int(max(-half, cz - r)), int(min(half, cz + r))):
            if (ix + iz) % 2:
                cam.polygon(d, np.array([(ix, floor_y, iz), (ix + 1, floor_y, iz), (ix + 1, floor_y, iz + 1), (ix, floor_y, iz + 1)], float), GRASS_B)
    # Where a pearl has just landed: a ring on the floor that fades.
    for pos, k in marks:
        r = 0.55 + 0.5 * (1 - k)
        col = tuple(int(g + (c - g) * k) for c, g in zip(PORTAL, GRASS_A))
        ring = [(pos[0] + r * math.cos(a), floor_y + 0.012, pos[2] + r * math.sin(a)) for a in np.linspace(0, 2 * math.pi, 18)[:-1]]
        cam.polygon(d, np.array(ring), col)
    # Soft shadows, then the players' faces from far to near.
    for i, p in enumerate(players):
        if i == skip:
            continue
        ring = [(p["pos"][0] + 0.42 * math.cos(a), floor_y + 0.01, p["pos"][2] + 0.42 * math.sin(a)) for a in np.linspace(0, 2 * math.pi, 14)[:-1]]
        cam.polygon(d, np.array(ring), (70, 118, 54))
    for q in pearls:
        ring = [(q[0] + 0.16 * math.cos(a), floor_y + 0.011, q[2] + 0.16 * math.sin(a)) for a in np.linspace(0, 2 * math.pi, 10)[:-1]]
        cam.polygon(d, np.array(ring), (70, 118, 54))
    quads = []
    for i, p in enumerate(players):
        if i == skip:
            continue
        quads += player_quads(p["pos"], p["yaw"], p["pitch"], p["walk"], p["amp"], p["swing"], TEAM[i], p["hurt"],
                              p.get("block", False), p.get("pearl", False))
    for q in pearls:
        # Drawn larger than the quarter block it is, so it can be followed.
        quads += cube_quads(q, 0.34, PEARL)
    order = []
    for corners, n, col in quads:
        centre = corners.mean(0)
        if (centre - cam.pos) @ n >= 0:
            continue
        order.append((float(((centre - cam.pos) ** 2).sum()), corners, col))
    order.sort(key=lambda q: -q[0])
    for _, corners, col in order:
        cam.polygon(d, corners, col)


class Segment:
    def __init__(self, path):
        z = np.load(path, allow_pickle=False)
        self.v = z["views"]
        self.names = [str(n) for n in z["names"]]
        self.samples = float(z["samples"])
        pf = [str(n) for n in z["player_fields"]]
        gf = [str(n) for n in z["global_fields"]]
        self.np = len(pf)
        self.pi = {n: i for i, n in enumerate(pf)}
        self.gi = {n: 2 * len(pf) + i for i, n in enumerate(gf)}
        self.T = len(self.v)
        pos = np.array([[self.col(p, "x"), self.col(p, "z")] for p in (0, 1)])  # [2, 2, T]
        step = np.hypot(np.diff(pos[:, 0], prepend=pos[:, 0, :1]), np.diff(pos[:, 1], prepend=pos[:, 1, :1]))
        # A step of more than three blocks is a pearl landing, not a walk.
        self.jump = step > 3.0
        step = np.where(self.jump, 0.0, step)
        self.dist = np.cumsum(step, 1)
        self.speed = step

    def col(self, p, name):
        return self.v[:, p * self.np + self.pi[name]]

    def g(self, name, t=0):
        return float(self.v[t, self.gi[name]])

    def player(self, p, t, a, seen_by=None):
        """State of player p between ticks t and t+1 (a in 0..1)."""
        t1 = min(t + 1, self.T - 1)
        lerp = lambda n: float(self.col(p, n)[t] * (1 - a) + self.col(p, n)[t1] * a)
        if seen_by is None:
            # Moved by a pearl: there one moment, here the next.
            pos = np.array([lerp("x"), lerp("y"), lerp("z")]) if not self.jump[p, t1] else \
                np.array([float(self.col(p, n)[t1 if a >= 0.5 else t]) for n in ("x", "y", "z")])
        else:
            q0 = np.array([float(self.col(seen_by, n)[t]) for n in ("seen_x", "seen_y", "seen_z")])
            q1 = np.array([float(self.col(seen_by, n)[t1]) for n in ("seen_x", "seen_y", "seen_z")])
            pos = q0 * (1 - a) + q1 * a if np.hypot(q1[0] - q0[0], q1[2] - q0[2]) <= 3.0 else (q1 if a >= 0.5 else q0)
        since = float(self.col(p, "ticks_since_swing")[t1]) + a - 1.0
        return {
            "pos": pos, "yaw": lerp("yaw"), "pitch": lerp("pitch"),
            "walk": float(self.dist[p, t] * (1 - a) + self.dist[p, t1] * a) * 2.4,
            "amp": min(float(self.speed[p, t1]) * 4.0, 1.0),
            "swing": since / 6.0 if 0 <= since < 6 else None,
            "hurt": self.col(p, "hurt_time")[t1] > 0,
            # Shown blocking when the server has the player blocking, which is what others see.
            "block": "blocking" in self.pi and self.col(p, "blocking")[t1] > 0,
            "pearl": "held" in self.pi and self.col(p, "held")[t1] == 1,
        }

    def pearls(self, t, a):
        """Positions of the pearls in flight between ticks t and t+1."""
        out = []
        if "pearl_on" not in self.pi:
            return out
        t1 = min(t + 1, self.T - 1)
        for p in (0, 1):
            at = lambda k: np.array([float(self.col(p, n)[k]) for n in ("pearl_x", "pearl_y", "pearl_z")])
            on0, on1 = self.col(p, "pearl_on")[t] > 0, self.col(p, "pearl_on")[t1] > 0
            if on0 and on1 and np.linalg.norm(at(t1) - at(t)) < 2.5:
                out.append(at(t) * (1 - a) + at(t1) * a)
            elif on0 and a < 0.5:
                out.append(at(t))
            elif on1 and a >= 0.5:
                out.append(at(t1))
        return out

    def marks(self, t, a, ticks=12):
        """Places a pearl put a player within the last `ticks` ticks, with
        how fresh each is (1 just now, 0 gone)."""
        out = []
        for p in (0, 1):
            for k in range(max(1, t - ticks + 1), min(t + 1, self.T - 1) + 1):
                if self.jump[p, k]:
                    age = (t + a) - (k - 0.5)
                    if 0 <= age < ticks:
                        out.append((np.array([float(self.col(p, n)[k]) for n in ("x", "y", "z")]), 1 - age / ticks))
        return out


def text(d, xy, s, f, fill, anchor="la"):
    d.text((xy[0] + 2, xy[1] + 2), s, font=f, fill=(0, 0, 0, 170), anchor=anchor)
    d.text(xy, s, font=f, fill=fill, anchor=anchor)


def hud(img, seg, t, fonts, inset_box):
    d = ImageDraw.Draw(img, "RGBA")
    w, h = img.size
    big, mid, small = fonts
    tick = min(t + 1, seg.T - 1)
    for p in (0, 1):
        hits = int(seg.col(p, "hits")[tick])
        combo = int(seg.col(p, "combo")[tick])
        sprint = int(seg.col(p, "sprint_hits")[tick])
        # The rate this player clicks at while attacking, fixed for the duel.
        cps = seg.g("cps" + str(p))
        x = 28 if p == 0 else w - 28
        anchor = "la" if p == 0 else "ra"
        # What the opponent has over the plain game, if anything.
        edge = []
        if p == 1 and "reach1" in seg.gi:
            if seg.g("reach1") > 3.001:
                edge.append(f"alcance {seg.g('reach1'):.2f}")
            if seg.g("kb_taken1") < 0.999:
                edge.append(f"KB {100 * seg.g('kb_taken1'):.0f} %")
            if seg.g("backtrack1_ms") > 0:
                edge.append(f"backtrack {seg.g('backtrack1_ms'):.0f} ms")
            if "autoblock1" in seg.gi and seg.g("autoblock1") > 0:
                edge.append("autoblock")
        blocks = "blocked" in seg.pi
        low = 158 + (30 if blocks else 0)
        box_w = 330 if not edge else 520
        d.rectangle([x - 14 if p == 0 else x - box_w, 16, x + 330 if p == 0 else x + 14, low + (40 if edge else 10)],
                    fill=(0, 0, 0, 96))
        if blocks:
            # Damage dealt in hearts (two points each) and hits taken on a block.
            text(d, (x, 158), f"Daño: {seg.col(p, 'damage')[tick] / 2:.0f} corazones   Bloqueó: {int(seg.col(p, 'blocked')[tick])}",
                 small, (225, 225, 225), anchor)
        if edge:
            text(d, (x, low), "Ventaja: " + ", ".join(edge), small, (255, 214, 120), anchor)
        text(d, (x, 22), seg.names[p], mid, TEAM[p]["ui"], anchor)
        text(d, (x, 60), f"Golpes: {hits}", mid, (255, 255, 255), anchor)
        text(d, (x, 98), f"Con sprint: {100 * sprint // max(hits, 1)} %   CPS: {cps:.1f}", small, (225, 225, 225), anchor)
        has_pearls = "pearls" in seg.pi and seg.col(p, "pearls")[0] > 0
        text(d, (x, 128), f"Ping: {seg.g('ping' + str(p)):.0f} ms" + (f"   Perlas: {int(seg.col(p, 'pearls')[tick])}" if has_pearls else ""),
             small, (225, 225, 225), anchor)
        if combo >= 3:
            text(d, (w // 2, 30 + 64 * p), f"COMBO x{combo}", big, TEAM[p]["ui"], "ma")
    # Keys of the first player.
    k = lambda n: seg.col(0, n)[tick]
    keys = [("W", 1, 0, k("forward") > 0), ("A", 0, 1, k("strafe") > 0), ("S", 1, 1, k("forward") < 0),
            ("D", 2, 1, k("strafe") < 0)]
    x0, y0, sz = 28, h - 196, 50
    for label, cx, cy, on in keys:
        bx, by = x0 + cx * (sz + 6), y0 + cy * (sz + 6)
        d.rectangle([bx, by, bx + sz, by + sz], fill=(255, 255, 255, 215) if on else (0, 0, 0, 110))
        d.text((bx + sz / 2, by + sz / 2), label, font=mid, fill=(20, 20, 20) if on else (235, 235, 235), anchor="mm")
    wide = [("SALTO", k("jump") > 0), ("SPRINT", seg.col(0, "sprinting")[tick] > 0), ("CLIC", k("attack") > 0)]
    if "using" in seg.pi:
        wide.append(("BLOQ", k("using") > 0))
    if "pearls" in seg.pi and seg.col(0, "pearls")[0] > 0:
        # Lit for a third of a second after a throw, which itself takes one tick.
        wide.append(("PERLA", seg.col(0, "pearls")[max(tick - 7, 0)] > seg.col(0, "pearls")[tick]))
    for i, (label, on) in enumerate(wide):
        bx, by = x0 + i * 112, y0 + 2 * (sz + 6)
        d.rectangle([bx, by, bx + 106, by + 40], fill=(255, 255, 255, 215) if on else (0, 0, 0, 110))
        d.text((bx + 53, by + 20), label, font=small, fill=(20, 20, 20) if on else (235, 235, 235), anchor="mm")
    secs = tick / 20.0
    kb = ""
    if "kb_kind" in seg.gi and seg.g("kb_kind") > 0:
        kb = f", KB de servidor {seg.g('kb_h'):.2f} / {seg.g('kb_v'):.2f}"
        if seg.g("hit_delay") != 20:
            kb += f", hit delay {seg.g('hit_delay'):.0f}"
    if "pearl_cd_s" in seg.gi and seg.col(0, "pearls")[0] + seg.col(1, "pearls")[0] > 0:
        kb += f", perlas cada {seg.g('pearl_cd_s'):.0f} s"
    text(d, (w // 2, h - 40), f"{int(secs) // 60:02d}:{int(secs) % 60:02d}   Simulador de 1.8.9, Speed II, vida infinita{kb}",
         small, (255, 255, 255), "ma")
    x1, y1, x2, y2 = inset_box
    d.rectangle([x1 - 3, y1 - 3, x2 + 3, y2 + 3], outline=(255, 255, 255, 230), width=3)
    text(d, (x1 + 8, y1 + 6), f"Lo que ve la {seg.names[0]}", small, (255, 255, 255))
    cx, cy = (x1 + x2) // 2, (y1 + y2) // 2
    d.line([cx - 9, cy, cx + 9, cy], fill=(255, 255, 255, 235), width=2)
    d.line([cx, cy - 9, cx, cy + 9], fill=(255, 255, 255, 235), width=2)


def look(yaw, pitch):
    y, p = math.radians(yaw), math.radians(pitch)
    return np.array([-math.sin(y) * math.cos(p), -math.sin(p), math.cos(y) * math.cos(p)])


def frames(seg, size, fps, ss, only=None):
    w, h = size
    rate = 60.0 / fps  # the camera smoothing below is tuned per 60 fps frame
    half, floor_y = seg.g("arena_half"), seg.g("floor_y")
    fonts = (font(46), font(30), font(21))
    iw, ih = int(w * 0.31), int(h * 0.31)
    ibox = (w - iw - 24, h - ih - 24, w - 24, h - 24)
    focus, theta = None, None
    for k in range(int((seg.T - 1) * fps / 20)):
        t = int(k * 20 / fps)
        a = k * 20 / fps - t
        pl = [seg.player(0, t, a), seg.player(1, t, a)]
        mid = (pl[0]["pos"] + pl[1]["pos"]) / 2 + np.array([0, 1.0, 0])
        dx, dz = pl[1]["pos"][0] - pl[0]["pos"][0], pl[1]["pos"][2] - pl[0]["pos"][2]
        sep = math.hypot(dx, dz)
        if focus is None:
            focus, theta = mid, math.atan2(dz, dx) + math.pi / 2
        focus = focus + (mid - focus) * min(0.12 * rate, 1.0)
        if sep > 0.6:
            want = math.atan2(dz, dx) + math.pi / 2
            diff = (want - theta + math.pi / 2) % math.pi - math.pi / 2  # either side of the line will do
            theta += diff * min(0.035 * rate, 1.0)
        dist = min(max(5.0 + 0.85 * sep, 6.0), 17.0)
        cpos = focus + np.array([math.cos(theta) * dist, 1.3 + 0.28 * dist, math.sin(theta) * dist])
        lim = half - 0.4
        cpos[0], cpos[2] = min(max(cpos[0], -lim), lim), min(max(cpos[2], -lim), lim)
        if only is not None and k != only:
            continue
        img = Image.new("RGB", (w * ss, h * ss))
        flying, marks = seg.pearls(t, a), seg.marks(t, a)
        draw_scene(img, Camera(cpos, focus, 52, w * ss, h * ss), pl, half, floor_y, pearls=flying, marks=marks)
        if ss > 1:
            img = img.resize((w, h), Image.BOX)
        # What the first player sees.
        eye = pl[0]["pos"] + np.array([0, 1.62, 0])
        seen = [pl[0], dict(pl[1], pos=seg.player(1, t, a, seen_by=0)["pos"])]
        inset = Image.new("RGB", (iw * ss, ih * ss))
        draw_scene(inset, Camera(eye, eye + look(pl[0]["yaw"], pl[0]["pitch"]), 70, iw * ss, ih * ss), seen, half, floor_y, skip=0,
                   pearls=flying, marks=marks)
        if ss > 1:
            inset = inset.resize((iw, ih), Image.BOX)
        img.paste(inset, ibox[:2])
        hud(img, seg, t, fonts, ibox)
        yield img


def title(size, lines, fps, seconds=1.6):
    w, h = size
    img = Image.new("RGB", size, (16, 20, 30))
    d = ImageDraw.Draw(img)
    d.text((w // 2, h // 2 - 40), lines[0], font=font(54), fill=(255, 255, 255), anchor="mm")
    for i, s in enumerate(lines[1:]):
        d.text((w // 2, h // 2 + 30 + 40 * i), s, font=font(26), fill=(190, 200, 215), anchor="mm")
    for _ in range(int(fps * seconds)):
        yield img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("duels", nargs="+")
    ap.add_argument("--out", default="duel.mp4")
    ap.add_argument("--fps", type=int, default=60)
    ap.add_argument("--size", default="1280x720")
    ap.add_argument("--ss", type=int, default=2, help="draw at this multiple of the size and scale down")
    ap.add_argument("--still", type=float, default=None, help="save one frame at this second as a PNG instead")
    args = ap.parse_args()
    size = tuple(int(x) for x in args.size.split("x"))
    segs = [Segment(p) for p in args.duels]
    if args.still is not None:
        for img in frames(segs[0], size, args.fps, args.ss, only=int(args.still * args.fps)):
            img.save(args.out)
            print("saved", args.out)
        return
    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{size[0]}x{size[1]}",
         "-r", str(args.fps), "-i", "-", "-an", "-c:v", "libx264", "-preset", "medium", "-crf", "21",
         "-pix_fmt", "yuv420p", "-movflags", "+faststart", args.out], stdin=subprocess.PIPE)
    n = 0
    for seg in segs:
        server_kb = "kb_kind" in seg.gi and seg.g("kb_kind") > 0
        lines = [f"{seg.names[0]} contra {seg.names[1]}",
                 f"Entrenada con {seg.samples/1e6:.0f} millones de pasos en el simulador de 1.8.9",
                 "Speed II, vida infinita, knockback de " + ("servidor" if server_kb else "vanilla")]
        for img in title(size, lines, args.fps):
            ff.stdin.write(img.tobytes())
        for img in frames(seg, size, args.fps, args.ss):
            ff.stdin.write(img.tobytes())
            n += 1
    ff.stdin.close()
    ff.wait()
    print("saved", args.out, n, "frames")


if __name__ == "__main__":
    main()
