"""F3a: give SEARCH a denser leaf value by blending in the score head.

D2 showed search strength is nearly all value head, so the evaluator is the lever. The value
head predicts win/loss -- a coin-flippy label in a game with a hidden draw. The score head
predicts final score margin: dense, already trained, loss 0.033 and flat... and search ignores
it completely.

This needs NO retraining and NO regeneration: the Python driver already computes the leaf value
it hands to pool.apply(), so blending happens there.

  value_used = (1-alpha) * P(win)  +  alpha * sigmoid(a * predicted_margin)

`a` is CALIBRATED on held-out positions (fit margin -> actual outcome) rather than guessed, so
alpha is the only free knob. alpha=0 reproduces today's behaviour and is the control.
"""
import sys, time, glob
import numpy as np, torch
import kingdomino as kd
from kdagent.net import load_net
from kdagent.selfplay import _gather_logits
from kdagent.dataset import Corpora

DEV, CKPT = "cuda", "runs/gen11.best.pt"
SIMS, GAMES = 512, 4000
net, _ = load_net(CKPT, DEV)
net = net.to(memory_format=torch.channels_last)

# ---- calibrate margin -> win probability on held-out positions -------------------------
c = Corpora(sorted(glob.glob("data/d1/*.kdc")))
rng = np.random.default_rng(1)
idx = np.sort(rng.choice(len(c), size=60_000, replace=False))
margins, outs = [], []
for s in range(0, len(idx), 4096):
    b = c.batch(idx[s:s + 4096], pc=2).to(DEV)
    with torch.no_grad():
        *_, score = net.forward_batch(b.board, b.lines, b.glob, with_score=True)
    margins.append((score[:, 0] - score[:, 1]).float().cpu())
    outs.append(b.value_rel[:, 0].float().cpu())
m = torch.cat(margins); y = torch.cat(outs)
a = torch.tensor(4.0, requires_grad=True)
opt = torch.optim.Adam([a], lr=0.1)
for _ in range(400):
    opt.zero_grad()
    loss = torch.nn.functional.binary_cross_entropy(torch.sigmoid(a * m), y)
    loss.backward(); opt.step()
A = float(a.detach())
acc = float(((torch.sigmoid(A * m) > 0.5).float() == (y > 0.5).float()).float().mean())
print(f"calibrated margin->winprob: a={A:.3f}  (score-only winner-acc={acc:.3f}, "
      f"value-head was 0.682)", flush=True)

# ---- arena: blended-value A vs pure-win-prob B ----------------------------------------
def heads(batch):
    board = torch.from_numpy(batch["board"]).to(DEV, non_blocking=True).float()
    board = board.to(memory_format=torch.channels_last)
    lines = torch.from_numpy(batch["lines"]).to(DEV, non_blocking=True)
    glob = torch.from_numpy(batch["glob"]).to(DEV, non_blocking=True)
    with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
        pm, cl, ds, value, score = net.forward_batch(board, lines, glob, with_score=True)
        logits = _gather_logits(pm, cl, ds, batch, DEV)
        pwin = torch.softmax(value.float(), 1)[:, 0]
        pscore = torch.sigmoid(A * (score[:, 0] - score[:, 1]).float())
    return logits.float(), pwin, pscore

def two_col(p):
    return torch.stack([p, 1.0 - p], 1).cpu().numpy()

for alpha in [float(x) for x in sys.argv[1:]] or [0.0, 0.5, 1.0]:
    pool = kd.BatchedArena(n_games=1024, total_games=GAMES, players=2,
                           sims_a=SIMS, sims_b=SIMS, c_puct=1.5, seed=9090)
    t0 = time.perf_counter()
    while not pool.done():
        d = pool.collect(); ba, bb = d["a"], d["b"]
        la = va = lb = vb = None
        if ba["b"] > 0:
            l, pw, ps = heads(ba)
            la, va = l.cpu().numpy(), two_col((1 - alpha) * pw + alpha * ps)
        if bb["b"] > 0:
            l, pw, _ = heads(bb)
            lb, vb = l.cpu().numpy(), two_col(pw)
        z2, z1 = np.zeros((0, 2), np.float32), np.zeros((0, 1), np.float32)
        pool.apply(la if la is not None else z1, va if va is not None else z2,
                   lb if lb is not None else z1, vb if vb is not None else z2)
    wa, wb, ties, _ = pool.stats()
    n = max(wa + wb + ties, 1)
    sc = (wa + 0.5 * ties) / n
    print(f"  alpha={alpha:<4} blended-A {100*sc:5.1f}% +/- {100*1.96*(0.25/n)**0.5:.1f}  "
          f"[{wa}/{wb}/t{ties}]  {time.perf_counter()-t0:.0f}s", flush=True)
