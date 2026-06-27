"""Trace KingdominoNet's forward to a TorchScript module the Rust self-play loop (tch) loads.

Also dumps a sample input + expected output (.npy) so the Rust side can parity-check that its
libtorch forward matches PyTorch. Run (from agent/):
  .venv/Scripts/python -m kdagent.trace --ckpt runs/gen0.best.pt --out runs/gen0.ts.pt
  .venv/Scripts/python -m kdagent.trace --out runs/random.ts.pt   # random net (for perf)
"""
from __future__ import annotations

import argparse
import os

import numpy as np
import torch

from kdagent.encoder import LINE_FEATS, N_PLANES, global_dim
from kdagent.net import KingdominoNet, load_net


class Wrap(torch.nn.Module):
    """forward(board, lines, glob) -> (place_map, claim_logits, discard, value). The per-action
    gather stays in Rust (dynamic action count); this traces just the net heads."""

    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, board, lines, glob):
        return self.net.forward_batch(board, lines, glob)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ckpt", default=None, help="checkpoint to trace; omit for a random net")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--out", default="runs/traced.ts.pt")
    ap.add_argument("--sample-dir", default="runs/parity", help="where to write parity .npy")
    ap.add_argument("--batch", type=int, default=256)
    ap.add_argument("--bf16", action="store_true", help="trace a bf16 model (for fast inference)")
    args = ap.parse_args()

    if args.ckpt:
        net, _ = load_net(args.ckpt, "cpu")
    else:
        torch.manual_seed(0)
        net = KingdominoNet(player_count=args.players)
    net.eval()
    wrap = Wrap(net).eval()

    pc, b = args.players, args.batch
    torch.manual_seed(1)
    board = torch.rand(b, pc * N_PLANES, 13, 13)
    lines = torch.rand(b, 8, LINE_FEATS)
    glob = torch.rand(b, global_dim(pc))
    if args.bf16:
        wrap = wrap.to(torch.bfloat16)
        board, lines, glob = (x.to(torch.bfloat16) for x in (board, lines, glob))

    with torch.no_grad():
        traced = torch.jit.trace(wrap, (board, lines, glob))
        pm, cl, dc, v = wrap(board, lines, glob)
    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    traced.save(args.out)

    os.makedirs(args.sample_dir, exist_ok=True)
    for name, t in [("board", board), ("lines", lines), ("glob", glob),
                    ("place_map", pm), ("claim_logits", cl), ("discard", dc), ("value", v)]:
        np.save(os.path.join(args.sample_dir, f"{name}.npy"), t.float().detach().numpy())
    print(f"traced -> {args.out}{' (bf16)' if args.bf16 else ''}  |  parity sample (B={b}) -> {args.sample_dir}")


if __name__ == "__main__":
    main()
