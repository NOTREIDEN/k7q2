"""Write input scripts for the parity rig.

    python gen_script.py NAME SEED OUT [speed=N] [sensitivity=X]

speed is the amplifier of the Speed effect (1 = Speed II, -1 = none);
sensitivity is the mouse slider from 0 to 1.

A script holds one line per client tick (C forward strafe jump sprint attack
dx dy, optionally followed by the right mouse button and by a number key, 1
to 9) and, when a second player is present, its actions keyed by server tick
(F tick ACTION ...)."""
import math, random, sys

def hold(rng, n, lo, hi, choices, weights=None):
    """A sequence of n values where each value is held for lo..hi ticks."""
    out = []
    while len(out) < n:
        v = rng.choices(choices, weights)[0]
        out += [v] * rng.randint(lo, hi)
    return out[:n]

def movement(rng, n, mouse=True, clicks=False, jumps=0.25):
    fwd = hold(rng, n, 3, 40, [1, 0, -1], [6, 2, 1])
    strafe = hold(rng, n, 3, 30, [0, 1, -1], [4, 1, 1])
    jump = hold(rng, n, 1, 25, [0, 1], [1 - jumps, jumps])
    sprint = hold(rng, n, 2, 50, [1, 0], [3, 1])
    att = [1 if clicks and rng.random() < 0.5 else 0 for _ in range(n)]
    dx = hold(rng, n, 1, 12, [0, 3, -3, 20, -20, 1, -1, 90, -90, 377, -377]) if mouse else [0] * n
    dy = hold(rng, n, 1, 12, [0, 0, 2, -2, 9, -9, 40, -40]) if mouse else [0] * n
    return [(fwd[i], strafe[i], jump[i], sprint[i], att[i], dx[i], dy[i]) for i in range(n)]

def jump_arc():
    """Heights above the floor on the airborne ticks of one jump."""
    ys, y, vy = [], 0.0, 0.42
    while True:
        y += vy
        if y <= 0:
            return ys
        ys.append(y)
        vy = (vy - 0.08) * 0.98

def pen(lines, half=4, height=4, roof=False):
    """A walled square so that nobody wanders off: inside is [-half, half) on both axes.
    With a roof nothing that is thrown leaves it either."""
    for y in range(4, 4 + height):
        for k in range(-half - 1, half + 1):
            for b in ((half, y, k), (-half - 1, y, k), (k, y, half), (k, y, -half - 1)):
                line = "block %d %d %d" % b
                if line not in lines:
                    lines.append(line)
    if roof:
        for x in range(-half - 1, half + 1):
            for z in range(-half - 1, half + 1):
                lines.append("block %d %d %d" % (x, 4 + height, z))
    lines.append("pen %.2f %.2f" % (-half + 0.4, half - 0.4))

def fight_inputs(rng, n, jumps=True, wild=True, per_count=0.15):
    """Keys, clicks and mouse of the real client in a fight. The pitch is steered so that the
    crosshair is on the other player much of the time (it stands in front of this one)."""
    seq, pitch = [], 0.0
    while len(seq) < n:
        mode = rng.choices(["calm", "rush", "stand", "wild"], [3, 4, 2, 1 if wild else 0])[0]
        length = rng.randint(20, 80)
        base = movement(rng, length, mouse=False, jumps=0.2 if jumps else 0.0)
        tap_every, tap_len = rng.randint(4, 14), rng.randint(1, 3)
        strafe = hold(rng, length, 5, 30, [0, 1, -1], [3, 1, 1])
        dxs = hold(rng, length, 1, 10, [0, 0, 1, -1, 3, -3, 8, -8, 25, -25])
        want = rng.choice([0.0, 8.0, 15.0, 25.0, -30.0 if wild else 5.0])
        for i in range(length):
            if mode == "stand":
                fwd, st, jump, sprint = 0, 0, 0, 0
                att = rng.random() < 0.6
            elif mode == "rush":
                fwd = 0 if i % tap_every < tap_len else 1
                st, sprint = strafe[i], 1 if rng.random() < 0.95 else 0
                jump = 1 if jumps and rng.random() < 0.08 else 0
                att = rng.random() < 0.8
            else:
                fwd, st, jump, sprint = base[i][0], base[i][1], base[i][2], base[i][3]
                att = rng.random() < 0.5
            dx = dxs[i]
            if mode == "wild" and rng.random() < 0.15:
                dx = rng.choice([90, -90, 377, -377, 600, -600])
            # Each mouse count lowers the pitch by per_count degrees.
            dy = max(-40, min(40, int(round((pitch - want) / per_count))))
            if rng.random() < 0.7:
                dy = max(-6, min(6, dy))
            pitch = max(-90.0, min(90.0, pitch - dy * per_count))
            seq.append((fwd, st, jump, sprint, 1 if att else 0, dx, dy))
    return seq[:n]

def with_use(rng, seq, holds=False):
    """Add the right mouse button to fight inputs: quick taps together with a click (the
    block hit), holds of every length with clicks going on underneath, bursts of presses, and
    stretches without any. With `holds`, most blocks start on the tick of a click and are
    kept up past the moment the server's answer to the hit comes back, which is when the
    client drops its own block."""
    out, i, n = [], 0, len(seq)
    while i < n:
        if holds:
            mode = rng.choices(["none", "hithold", "hold", "after"], [1, 6, 1, 1])[0]
        else:
            mode = rng.choices(["none", "tap", "hold", "spam", "after"], [3, 4, 4, 1, 3])[0]
        length = rng.randint(15, 70)
        every, hold_len, gap = rng.randint(2, 12), rng.randint(2, 15), rng.randint(1, 10)
        for k in range(length):
            if i >= n:
                break
            f, st, j, spr, att, dx, dy = seq[i]
            use = 0
            if mode == "tap":
                if k % every == 0:
                    use = 1
                    att = 1 if rng.random() < 0.8 else att
            elif mode == "hold":
                use = 1 if k % (hold_len + gap) < hold_len else 0
            elif mode == "spam":
                use = 1 if rng.random() < 0.5 else 0
            elif mode == "hithold":
                # Click and press on the same tick, then keep the button down for a while.
                ph = k % (every + 9)
                if ph == 0:
                    keep = rng.choice([2, 2, 3, 3, 4, 5, 6, 8, 12])
                att = 1 if ph == 0 or rng.random() < 0.5 else 0
                use = 1 if ph < keep else 0
            elif mode == "after":
                # Click, then press on the next tick and keep it down a little while.
                ph = k % (every + 4)
                att = 1 if ph == 0 else att
                use = 1 if 1 <= ph <= rng.choice([1, 2, 3, 3, 4, 6]) else 0
            out.append((f, st, j, spr, att, dx, dy, use))
            i += 1
    return out

def with_pearls(rng, seq, per_count, gap=(25, 80), blocking=False, solo=False, potions=0, pearls=True, again=False):
    """Add ender pearl throws to the inputs: the number key of the pearls, a right click and
    the number key of the sword. Thrown at once or after carrying the pearl for a while
    (clicks go on meanwhile), with the button kept down so that more follow, out of a sword
    block, and with the view flicked up, down or aside just before. With potions, some of
    the throws are potions instead, each from a hotbar slot of its own (number keys 3 on),
    mostly thrown at the feet; now and then the key of a slot that is already empty."""
    n = len(seq)
    out = [list(s) + [0] * (9 - len(s)) for s in seq]
    full = list(range(3, 3 + potions))
    rng.shuffle(full)
    empty = []

    def pitch_at(i):
        p = 0.0
        for k in range(i + 1):
            p = max(-90.0, min(90.0, p - out[k][6] * per_count))
        return p

    def flick(i, want_pitch, turn):
        """Look somewhere else from tick i on and come back two ticks later."""
        d = int(round((pitch_at(i - 1) - want_pitch) / per_count))
        out[i][6] += d
        out[i][5] += turn
        if i + 2 < n:
            out[i + 2][6] -= d
            out[i + 2][5] -= turn

    t = rng.randint(14, 40)
    while t < n - 45:
        kind = rng.choices(["quick", "carry", "held", "block", "again"], [5, 3, 2, 3 if blocking else 0, 6 if again else 0])[0]
        aim = rng.choices(["as is", "up", "down", "side", "high"], [3, 3, 2, 2, 1] if not solo else [1, 3, 2, 3, 2])[0]
        key = 2
        if potions and (not pearls or rng.random() < 0.6):
            if empty and (not full or rng.random() < 0.1):
                key = rng.choice(empty)
            elif full:
                key = full.pop()
                empty.append(key)
            aim = rng.choices(["as is", "up", "down", "side", "high"], [2, 2, 6, 1, 1])[0]
        throw = t
        if kind == "quick":
            out[t - 1][7] = 0
            out[t][8], out[t][7] = key, 1
            out[t + 1][7] = 0
            back = t + rng.choice([1, 1, 1, 2, 4])
        elif kind == "carry":
            out[t][8] = key
            throw = t + rng.randint(1, 8)
            for k in range(t, throw):
                out[k][7] = 0
            out[throw][7] = 1
            out[throw + 1][7] = 0
            back = throw + rng.choice([0, 1, 2, 6])
        elif kind == "again":
            # Clicked again on the very next tick, once or twice: the button
            # is let go and pressed between two ticks (2). With potions each
            # click takes another slot in hand first, or stays on the one
            # just emptied.
            out[t - 1][7] = 0
            out[t][8], out[t][7] = key, 1
            more = rng.choice([1, 1, 2])
            for k in range(1, more + 1):
                out[t + k][7] = 2
                if potions and full and rng.random() < 0.8:
                    out[t + k][8] = full.pop()
                    empty.append(out[t + k][8])
            out[t + more + 1][7] = rng.choice([0, 0, 1])
            out[t + more + 2][7] = 0
            back = t + more + rng.choice([1, 1, 2, 3])
        elif kind == "held":
            # The button stays down: another pearl every fourth tick.
            out[t - 1][7] = 0
            out[t][8] = key
            keep = rng.choice([3, 5, 6, 9, 10])
            for k in range(t, t + keep):
                out[k][7] = 1
            out[t + keep][7] = 0
            back = t + keep + rng.choice([0, 1, 3])
        else:
            # Out of a block with the sword, the button never let go.
            for k in range(t - 4, t + 1):
                out[k][7] = 1
            out[t][8] = key
            keep = rng.choice([1, 2, 5, 9])
            for k in range(t, t + keep):
                out[k][7] = 1
            out[t + keep][7] = 0
            throw = t + 1
            back = t + keep + rng.choice([0, 1, 2])
        if back < n:
            out[back][8] = 1 if out[back][8] == 0 else out[back][8]
        if aim != "as is" and throw - 1 > 0:
            want = {"up": rng.uniform(-50, -15), "down": rng.uniform(40, 90), "side": rng.uniform(-30, 20),
                    "high": rng.uniform(-90, -60)}[aim]
            turn = rng.choice([0, 0, 200, -200, 600, -600, 1200]) if aim in ("side", "high") or solo else 0
            flick(throw - 1, want, turn)
        t = back + rng.randint(*gap)
    return [tuple(o) for o in out]

def fight_fake(rng, n, lines, hops=True, attacks=True, blocks=False, pearls=0.0, potions=0, potion_rate=0.0):
    """What the scripted player does on each server tick."""
    arc, air = jump_arc(), []
    full = list(range(2, 2 + potions))
    rng.shuffle(full)
    empty = []
    dist, angle = 2.5, 0.0
    t = 0
    later = {}
    while t < n:
        mode = rng.choices(["front", "free", "still"], [6, 2, 1])[0]
        length = rng.randint(15, 90)
        want_d = rng.choice([0.2, 0.6, 1.5, 2.0, 2.5, 2.8, 3.0, 3.2, 3.6, 4.4])
        want_a = rng.choice([0, 0, 0, 5, -5, 12, -12, 30, -30])
        goal = (rng.uniform(-3.5, 3.5), rng.uniform(-3.5, 3.5))
        noise = rng.choice([0, 0, 3, -3, 40, 180])
        fpitch = rng.choice([0, 0, 10, -20, 60])
        for _ in range(length):
            if attacks:
                r = rng.random()
                if r < 0.25:
                    lines.append("F %d SWING" % t)
                    lines.append("F %d ATTACK" % t)
                elif r < 0.28:
                    lines.append("F %d ATTACK" % t)
                elif r < 0.31:
                    lines.append("F %d SWING" % t)
            if blocks:
                r = rng.random()
                if r < 0.07:
                    lines.append("F %d USE" % t)
                elif r < 0.14:
                    lines.append("F %d RELEASE" % t)
            r = rng.random()
            if r < 0.10:
                lines.append("F %d SPRINT 1" % t)
            elif r < 0.14:
                lines.append("F %d SPRINT 0" % t)
            if pearls and not later and rng.random() < pearls:
                # Take the pearls in hand, throw (now or a little later, sometimes after
                # turning the head), take the sword again.
                lines.append("F %d SLOT 1" % t)
                k = t + rng.choice([0, 0, 0, 1, 3])
                if rng.random() < 0.5:
                    later.setdefault(k, []).append("LOOK %d %d" % (rng.randint(-180, 180), rng.choice([-80, -40, -20, -5, 10, 30, 70, 90])))
                later.setdefault(k, []).append("USE")
                if rng.random() < 0.2:
                    later.setdefault(k + 1, []).append("USE")
                later.setdefault(k + rng.choice([0, 1, 1, 2, 5]), []).append("SLOT 0")
            if potion_rate and not later and rng.random() < potion_rate and (full or empty):
                # The same with a potion: mostly at its own feet, sometimes ahead or up.
                if empty and (not full or rng.random() < 0.1):
                    slot = rng.choice(empty)
                else:
                    slot = full.pop()
                    empty.append(slot)
                lines.append("F %d SLOT %d" % (t, slot))
                k = t + rng.choice([0, 0, 0, 1, 3])
                if rng.random() < 0.8:
                    later.setdefault(k, []).append("LOOK %d %d" % (rng.randint(-180, 180), rng.choice([90, 90, 80, 70, 55, 40, 20, 0, -30, -60, -90])))
                later.setdefault(k, []).append("USE")
                if rng.random() < 0.2:
                    later.setdefault(k + 1, []).append("USE")
                later.setdefault(k + rng.choice([0, 1, 1, 2, 5]), []).append("SLOT 0")
            for a in later.pop(t, []):
                lines.append("F %d %s" % (t, a))
            if hops and not air and rng.random() < 0.04:
                air = list(arc)
            up = air.pop(0) if air else 0.0
            if mode == "front":
                dist += max(-0.25, min(0.25, want_d - dist))
                angle += max(-6.0, min(6.0, want_a - angle))
                lines.append("F %d FRONT %r %r %r %d %d" % (t, dist, angle, up, noise, fpitch))
            elif mode == "free":
                lines.append("F %d GOTO %r %r %r %r %d %d" % (t, goal[0], goal[1], rng.choice([0.1, 0.2806, 0.45]), up, noise, fpitch))
            elif up > 0.0 or rng.random() < 0.5:
                lines.append("F %d GOTO %r %r 0.0 %r %d %d" % (t, goal[0], goal[1], up, noise, fpitch))
            t += 1

def main():
    name, seed, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    opts = dict(a.split("=") for a in sys.argv[4:])
    rng = random.Random(seed)
    lines, idle = [], [(0, 0, 0, 0, 0, 0, 0)] * 12
    idle8 = [(0, 0, 0, 0, 0, 0, 0, 0)] * 12
    per_count = 0.15
    if "speed" in opts:
        lines.append("speed " + opts["speed"])
    if "sensitivity" in opts:
        lines.append("sensitivity " + opts["sensitivity"])
        f = float(opts["sensitivity"]) * 0.6 + 0.2
        per_count = 0.15 * f * f * f * 8.0
    # The kit, for any scene: health=1 makes hits cost health (no Resistance, no
    # Saturation), armor=1 is a diamond set that does not wear, prot and ff are
    # the levels of Protection on all four pieces and of Feather Falling on the
    # boots, sharp is Sharpness on the sword, food=LEVEL,SATURATION is what the
    # players start with, refill is the health below which the rig gives a
    # player everything back.
    for k in ("health", "armor", "prot", "ff", "sharp", "refill", "eseed"):
        if k in opts:
            lines.append("%s %s" % (k, opts[k]))
    if "food" in opts:
        lines.append("food " + opts["food"].replace(",", " "))
    if name == "basic":
        seq = idle + [(1, 0, 0, 0, 0, 0, 0)] * 25 + [(1, 0, 0, 1, 0, 0, 0)] * 40 + [(1, 0, 1, 1, 0, 0, 0)] * 40 \
            + [(0, 0, 0, 0, 0, 0, 0)] * 15 + [(1, 1, 0, 1, 0, 4, 0)] * 30 + [(0, -1, 1, 0, 0, -10, 3)] * 30 \
            + [(-1, 0, 0, 0, 0, 0, -5)] * 20 + [(0, 0, 0, 0, 0, 0, 0)] * 20
    elif name == "move":
        seq = idle + movement(rng, 600)
    elif name == "nospeed":
        lines.append("speed -1")
        seq = idle + movement(rng, 400)
    elif name == "walls":
        # A stone corner next to the start, to run and jump into.
        for y in (4, 5, 6):
            for k in range(-3, 6):
                lines.append(f"block {k} {y} 5")
                lines.append(f"block 4 {y} {k}")
        seq = idle + movement(rng, 600, jumps=0.4)
    elif name == "fight":
        pen(lines)
        lines.append("fake 0.5 4 3.0 180 0")
        seq = idle + fight_inputs(rng, 1000, per_count=per_count)
        fight_fake(rng, len(seq) + 60, lines)
    elif name == "block":
        # A fight in which both players block with the sword.
        pen(lines)
        lines.append("fake 0.5 4 3.0 180 0")
        if opts.get("unbreakable", "0") != "0":
            lines.append("unbreakable 1")
        seq = idle8 + with_use(rng, fight_inputs(rng, 1000, per_count=per_count), holds=opts.get("holds", "0") != "0")
        fight_fake(rng, len(seq) + 60, lines, blocks=True)
    elif name == "pearl":
        # A fight with ender pearls on both sides, under a roof so that they stay inside.
        pen(lines, half=6, height=9, roof=True)
        lines.append("fake 0.5 4 3.0 180 0")
        lines.append("pearls %s" % opts.get("pearls", "16"))
        if opts.get("unbreakable", "0") != "0":
            lines.append("unbreakable 1")
        seq = fight_inputs(rng, 1000, per_count=per_count)
        blocking = opts.get("blocks", "0") != "0"
        seq = with_use(rng, seq) if blocking else seq
        gap = (8, 30) if opts.get("many", "0") != "0" else (25, 80)
        seq = [(0,) * 9] * 12 + with_pearls(rng, seq, per_count, gap=gap, blocking=blocking)
        fight_fake(rng, len(seq) + 60, lines, blocks=blocking, pearls=float(opts.get("fake", "0.02")))
    elif name == "pot":
        # A fight with splash potions of healing on both sides, with or without pearls.
        roof = opts.get("roof", "1") != "0"
        pen(lines, half=6, height=9, roof=True) if roof else pen(lines)
        lines.append("fake 0.5 4 3.0 180 0")
        n_pearls, n_pots = int(opts.get("pearls", "0")), int(opts.get("potions", "7"))
        if n_pearls:
            lines.append("pearls %d" % n_pearls)
        lines.append("potions %d" % n_pots)
        if opts.get("unbreakable", "0") != "0":
            lines.append("unbreakable 1")
        seq = fight_inputs(rng, 1000, per_count=per_count)
        blocking = opts.get("blocks", "0") != "0"
        seq = with_use(rng, seq) if blocking else seq
        gap = (8, 30) if opts.get("many", "0") != "0" else (25, 80)
        seq = [(0,) * 9] * 12 + with_pearls(rng, seq, per_count, gap=gap, blocking=blocking, potions=n_pots, pearls=n_pearls > 0,
                                            again=opts.get("again", "0") != "0")
        fight_fake(rng, len(seq) + 60, lines, blocks=blocking, pearls=float(opts.get("fake", "0.02")) if n_pearls else 0.0,
                   potions=n_pots, potion_rate=float(opts.get("fakepot", "0.02")))
    elif name == "potsolo":
        # Alone under the roof: potions (and pearls) in every direction.
        pen(lines, half=6, height=9, roof=True)
        n_pearls, n_pots = int(opts.get("pearls", "0")), int(opts.get("potions", "7"))
        if n_pearls:
            lines.append("pearls %d" % n_pearls)
        lines.append("potions %d" % n_pots)
        seq = [(0,) * 9] * 12 + with_pearls(rng, movement(rng, 500, clicks=True), per_count, gap=(10, 45), solo=True,
                                            potions=n_pots, pearls=n_pearls > 0)
    elif name == "pearlsolo":
        # Alone under the roof: pearls in every direction, at the walls, the roof and the floor.
        pen(lines, half=6, height=9, roof=True)
        lines.append("pearls %s" % opts.get("pearls", "16"))
        seq = [(0,) * 9] * 12 + with_pearls(rng, movement(rng, 800, clicks=True), per_count, gap=(10, 45), solo=True)
    elif name == "long":
        # Nobody leaves the ground and the scripted player never hits back, so that the
        # entity tracker reaches its periodic full position update.
        pen(lines)
        lines.append("fake 0.5 4 3.0 180 0")
        seq = idle + fight_inputs(rng, 1000, jumps=False, wild=False, per_count=per_count)
        fight_fake(rng, len(seq) + 60, lines, hops=False, attacks=False)
    elif name == "wall":
        # The real player leans on the east wall while the other one knocks it along the wall,
        # north and south in turns: fast slides along a wall are what the server objects to.
        # It steps back and returns now and then, and looks at the wall from changing angles,
        # so that it comes to rest against the wall in many slightly different ways.
        pen(lines)
        lines.append("start 3.0 4 -3.0 %d 0" % rng.randint(-120, -60))
        lines.append("fake 3.0 4 -1.0 180 0")
        n = 1000
        strafe = hold(rng, n, 5, 40, [0, 1, -1], [2, 1, 1])
        sprint = hold(rng, n, 5, 60, [1, 0], [2, 1])
        jump = hold(rng, n, 1, 12, [0, 1], [5, 1])
        fwd = hold(rng, n, 6, 60, [1, -1], [5, 1])
        dx = hold(rng, n, 1, 20, [0, 0, 0, 7, -7, 40, -40])
        yaw, seq = 0.0, list(idle)
        for i in range(n):
            d = dx[i] if abs(yaw + dx[i] * per_count) < 50 else 0
            yaw += d * per_count
            seq.append((fwd[i], strafe[i], jump[i], sprint[i], 0, d, 0))
        t = 0
        while t < len(seq) + 60:
            angle = rng.choice([250, 270, 270, 290, 70, 90, 90, 110])
            for _ in range(rng.randint(30, 70)):
                if rng.random() < 0.3:
                    lines.append("F %d SPRINT 1" % t)
                    lines.append("F %d SWING" % t)
                    lines.append("F %d ATTACK" % t)
                lines.append("F %d FRONT %r %r 0.0 0 0" % (t, rng.choice([1.2, 1.5, 2.0]), float(angle)))
                t += 1
    else:
        raise SystemExit("unknown script " + name)
    lines += ["C " + " ".join("%d" % v for v in s) for s in seq]
    open(out, "w").write("\n".join(lines) + "\n")
    print(out, len(seq), "ticks")

main()
