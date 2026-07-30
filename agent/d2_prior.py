"""D2: does the policy prior contribute anything to search strength?

Agent A gets its policy logits ZEROED (uniform prior); agent B gets gen11's real policy. Both
use gen11's value head, so the ONLY difference is whether the prior carries information.

  A loses badly   -> the prior matters a lot; policy improvements should translate into search
                     strength, and the plateau is a training/extraction problem.
  A ~ 50%         -> search strength comes almost entirely from the value head. Improving the
                     policy can never improve search, and the 2 points of policy gain we
                     measured were always going to be invisible.

Driven directly against BatchedArena so no engine change is needed.
"""
import sys, time
import numpy as np
import torch
import kingdomino as kd
from kdagent.net import load_net
from kdagent.selfplay import _gather_logits

CKPT = "runs/gen11.best.pt"
SIMS = int(sys.argv[1]) if len(sys.argv) > 1 else 512
GAMES = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
UNIFORM_SIDE = "a"

dev = "cuda"
net, _ = load_net(CKPT, dev)
net = net.to(memory_format=torch.channels_last)

pool = kd.BatchedArena(n_games=1024, total_games=GAMES, players=2,
                       sims_a=SIMS, sims_b=SIMS, c_puct=1.5, seed=4242)

def forward(batch):
    """Per-action logits + seat-relative value for one collected leaf batch."""
    board = torch.from_numpy(batch["board"]).to(dev, non_blocking=True).float()
    board = board.to(memory_format=torch.channels_last)
    lines = torch.from_numpy(batch["lines"]).to(dev, non_blocking=True)
    glob = torch.from_numpy(batch["glob"]).to(dev, non_blocking=True)
    with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
        pm, cl, ds, value = net.forward_batch(board, lines, glob)
        logits = _gather_logits(pm, cl, ds, batch, dev)
        value_rel = torch.softmax(value.float(), dim=1)
    return logits.float().cpu().numpy(), value_rel.cpu().numpy()

t0 = time.perf_counter()
while not pool.done():
    d = pool.collect()
    ba, bb = d["a"], d["b"]
    la = va = lb = vb = None
    if ba["b"] > 0:
        la, va = forward(ba)
        if UNIFORM_SIDE == "a":
            # Uniform prior: equal logits over the real actions. The pool masks pad slots, so
            # zeroing only the live entries keeps the mask semantics intact.
            la = np.where(np.isfinite(la), 0.0, la).astype(np.float32)
    if bb["b"] > 0:
        lb, vb = forward(bb)
    pool.apply(
        la if la is not None else np.zeros((0, 1), np.float32),
        va if va is not None else np.zeros((0, 2), np.float32),
        lb if lb is not None else np.zeros((0, 1), np.float32),
        vb if vb is not None else np.zeros((0, 2), np.float32),
    )
el = time.perf_counter() - t0
wa, wb, ties, done = pool.stats()
n = max(wa + wb + ties, 1)
score = (wa + 0.5 * ties) / n
ci = 1.96 * (0.25 / n) ** 0.5
print(f"UNIFORM-prior A vs REAL-prior B  (both gen11 value head, {SIMS} sims)")
print(f"  A(uniform) {100*score:.1f}% +/- {100*ci:.1f}   [{wa} / {wb} / tie {ties}]  "
      f"{n} games in {el:.0f}s")
