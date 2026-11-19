"""Keep the pool of earlier versions that the league plays against.

    git ls-tree weights pool/ | python pool.py --latest latest.pt --blob SHA | git mktree

Reads the entries of the pool as `git ls-tree` prints them and writes the
entries it should hold after the network in `--latest`, stored in git as
`--blob`, has come by (see `pvpai.league.ladder`). `--samples` gives the
count in millions directly instead of reading it from a checkpoint.

Pinned versions (`pin_*.pt`, see `pvpai.league.PIN`) are passed through as
they are: the ladder never drops them."""
import argparse
import sys

from pvpai.league import PIN, ladder


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--latest", default=None)
    ap.add_argument("--samples", type=float, default=None)
    ap.add_argument("--blob", required=True)
    ap.add_argument("--keep", type=int, default=10)
    ap.add_argument("--gap", type=float, default=300.0, help="millions of samples between the two newest versions")
    args = ap.parse_args()

    if args.samples is None:
        import torch
        args.samples = torch.load(args.latest, map_location="cpu", weights_only=False).get("samples", 0) / 1e6
    new = int(round(args.samples))
    blobs, pinned = {}, {}
    for line in sys.stdin:
        meta, _, path = line.rstrip("\n").partition("\t")
        name = path.rsplit("/", 1)[-1]
        if name.endswith(".pt") and name[:-3].isdigit():
            blobs[int(name[:-3])] = meta.split()[2]
        elif name.endswith(".pt") and name.startswith(PIN) and "/" not in name and len(meta.split()) == 3:
            pinned[name] = meta.split()[2]
    kept = ladder(list(blobs), new, args.keep, args.gap)
    blobs.setdefault(new, args.blob)
    for s in kept:
        print(f"100644 blob {blobs[s]}\t{s:06d}.pt")
    for name in sorted(pinned):
        print(f"100644 blob {pinned[name]}\t{name}")


if __name__ == "__main__":
    main()
