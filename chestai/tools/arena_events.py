"""Waves, claims and resets of each seed of an arena round, in seconds, from the round's job logs.

    gh run view <run id> --log > all.log
    python3 arena_events.py all.log

w = a wave starts, c = a claim, r = a reset; the number after the letter is the chest.
"""
import re
import sys


def main(path):
    events = {}
    for line in open(path, errors="replace"):
        parts = line.split("\t")
        m = re.search(r"\((\d+)(?:, (\w+))?\)", parts[0]) if len(parts) > 1 else None
        if not m:
            continue
        key = m.group(1) + ("/" + m.group(2) if m.group(2) else "")
        d = events.setdefault(key, {"t": 0, "ev": []})
        t = re.search(r"ARENA t=(\d+)", line)
        if t:
            d["t"] = int(t.group(1))
            continue
        for word in ("wave at chest", "claim of chest", "reset of chest"):
            if "ARENA " + word in line:
                n = re.search(word + r" (\d+)", line).group(1)
                d["ev"].append("%d%s%s" % (d["t"] // 20, word[0], n))
    for key in sorted(events, key=lambda k: (int(k.split("/")[0]), k)):
        print(key, " ".join(events[key]["ev"]))


if __name__ == "__main__":
    main(sys.argv[1])
