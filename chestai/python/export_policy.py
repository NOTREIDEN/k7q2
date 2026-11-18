"""Write a checkpoint's policy in the form the Java mod reads, and a check
file with the network's answers to a few observations so the mod can test
that it computes the same thing.

    python export_policy.py runs/a/last.pt mod/forge/src/main/resources/chestai/policy.bin [--check check.bin]

Format (little endian): "CHAI", int 1, obs_dim, hidden, heads, dims...,
long samples, int count, then per tensor: int name length, the name in
UTF-8, int rank, the sizes, the floats. Only the actor's tensors go in.
"""
import argparse
import os
import struct
import sys

import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chestai.model import load_policy  # noqa: E402


def write(pol, ck, path):
    sd = {k: v.float().contiguous() for k, v in pol.state_dict().items() if not (k.startswith("vf.") or k.startswith("penc."))}
    with open(path, "wb") as f:
        f.write(b"CHAI")
        f.write(struct.pack("<iiii", 1, pol.obs_dim, pol.hidden, len(pol.act_dims)))
        f.write(struct.pack("<%di" % len(pol.act_dims), *pol.act_dims))
        f.write(struct.pack("<q", int(ck.get("samples", 0))))
        f.write(struct.pack("<i", len(sd)))
        for k, v in sd.items():
            name = k.encode()
            f.write(struct.pack("<i", len(name)))
            f.write(name)
            f.write(struct.pack("<i", v.dim()))
            f.write(struct.pack("<%di" % v.dim(), *v.shape))
            f.write(v.numpy().astype("<f4").tobytes())


def check(pol, path, obs):
    """Ticks of observations and the logits of every head, memory carried."""
    state = pol.initial_state(1)
    with open(path, "wb") as f:
        f.write(struct.pack("<ii", obs.shape[0], obs.shape[1]))
        with torch.no_grad():
            for t in range(obs.shape[0]):
                o = obs[t:t + 1]
                h, state = pol.core(o, state)
                lg = torch.cat(pol.logits(h), -1)[0]
                f.write(o[0].numpy().astype("<f4").tobytes())
                f.write(struct.pack("<i", lg.shape[0]))
                f.write(lg.numpy().astype("<f4").tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ckpt")
    ap.add_argument("out")
    ap.add_argument("--check", default="")
    ap.add_argument("--dump", default="", help="a dump of chestsim's dump example to take observations from")
    args = ap.parse_args()
    pol, ck = load_policy(args.ckpt)
    pol.eval()
    write(pol, ck, args.out)
    print("wrote", args.out, os.path.getsize(args.out), "bytes,", ck.get("samples", 0) / 1e6, "M samples")
    if args.check:
        obs = []
        if args.dump:
            for line in open(args.dump):
                if line.startswith("F"):
                    t = line.split()
                    n = pol.obs_dim
                    obs.append([float(x) for x in t[-n:]])
                    if len(obs) >= 300:
                        break
            obs = torch.tensor(obs, dtype=torch.float32)
        else:
            g = torch.Generator().manual_seed(0)
            obs = torch.rand(64, pol.obs_dim, generator=g)
            obs[:, ::7] = (obs[:, ::7] > 0.5).float()
        check(pol, args.check, obs)
        print("wrote", args.check)


if __name__ == "__main__":
    main()
