"""Average checkpoints into one compact checkpoint.

    python merge.py weights/w*.pt --out merged.pt

The machines of a training run each publish their own weights; this writes
the one network they stand for. Files much older than the newest are left
out, because their machine stopped earlier. With a single input it only
rewrites the checkpoint in the compact form (half precision, no optimiser
state), which is how `seed/` is made."""
import argparse
from types import SimpleNamespace

import torch

from pvpai.model import load_policy
from pvpai.sync import pack


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoints", nargs="+")
    ap.add_argument("--out", required=True)
    ap.add_argument("--max-age-minutes", type=float, default=20.0)
    args = ap.parse_args()

    loaded = [load_policy(p) for p in args.checkpoints]
    newest = max(ck.get("time", 0.0) for _, ck, _ in loaded)
    keep = [(pol, ck) for pol, ck, _ in loaded if newest - ck.get("time", newest) <= args.max_age_minutes * 60]
    policy = keep[0][0]
    sds = [pol.state_dict() for pol, _ in keep]
    policy.load_state_dict({k: torch.stack([sd[k] for sd in sds if k in sd]).mean(0) for k in sds[0]})
    vs = SimpleNamespace(var=sum(ck["vs"][0] for _, ck in keep) / len(keep), count=max(ck["vs"][1] for _, ck in keep))
    samples = sum(ck.get("samples", 0) for _, ck in keep)
    marks = sorted(set.intersection(*[set(ck.get("marks", ())) for _, ck in keep]))
    torch.save(pack(policy, vs, samples, max(ck.get("iters", 0) for _, ck in keep), marks=marks), args.out)
    print(f"{len(keep)} of {len(loaded)} checkpoints averaged, {samples/1e6:.1f}M samples, written to {args.out}")


if __name__ == "__main__":
    main()
