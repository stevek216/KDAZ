"""D1: has the value head improved at all since gen0?

D2 showed search strength is nearly all value head (the whole policy prior is worth 4.7 pts),
so this is now the load-bearing question. Scores gen0 and gen11 on the SAME held-out positions:
  winner accuracy  -- does argmax(value) match who actually won?
  correlation      -- does the predicted win-prob track the outcome?
gen0 measured 90% / 0.87 earlier in the project. If gen11 is no better, the search has been
running on an evaluator that stopped improving many generations ago.
"""
import sys, glob
import numpy as np, torch
from kdagent.corpus import PackedCorpus
from kdagent.dataset import Corpora
from kdagent.net import load_net

files = sorted(glob.glob("data/d1/*.kdc"))
c = Corpora(files)
print(f"eval set: {len(files)} shard(s), {len(c):,} records, "
      f"{len(set(c.game_ids.tolist())):,} distinct games")

rng = np.random.default_rng(0)
idx = np.sort(rng.choice(len(c), size=min(120_000, len(c)), replace=False))

for name in ["runs/gen0.best.pt", "runs/gen11.best.pt"]:
    net, _ = load_net(name, "cuda")
    accs, preds, outs = [], [], []
    for s in range(0, len(idx), 4096):
        b = c.batch(idx[s:s + 4096], pc=2).to("cuda")
        with torch.no_grad():
            *_, value = net.forward_batch(b.board, b.lines, b.glob)
            vp = torch.softmax(value[:, :2].float(), 1)
        tgt = b.value_rel
        accs.append((vp.argmax(1) == tgt.argmax(1)).float().cpu().numpy())
        preds.append(vp[:, 0].cpu().numpy())
        outs.append(tgt[:, 0].cpu().numpy())
    acc = float(np.concatenate(accs).mean())
    p, o = np.concatenate(preds), np.concatenate(outs)
    corr = float(np.corrcoef(p, o)[0, 1])
    params = sum(x.numel() for x in net.parameters())
    print(f"  {name.split('/')[-1]:18} params={params:>9,}  winner-acc={acc:.3f}  corr={corr:.3f}")
