"""Train `KingdominoNet` on a self-play corpus (AlphaZero-style supervised targets).

Loss = policy cross-entropy (MCTS visit distribution vs the net's masked per-action policy)
     + value_coef · value cross-entropy (seat-relative outcome distribution vs the max-n head).
The corpus stores raw obs/legal, so improving the feature schema never invalidates it — retrain.

    cd agent
    .venv/Scripts/python -m kdagent.train --corpus data/selfplay/rollout.jsonl \
        --epochs 5 --batch-size 256 --out runs/net --device cuda
"""
from __future__ import annotations

import argparse
import time
from pathlib import Path

import numpy as np
import torch

from .dataset import Batch, collate, load_corpus
from .encoder import A_CLAIM, A_PLACE
from .net import KingdominoNet, load_net


def gather_logits(place_map, claim_logits, discard, batch: Batch) -> torch.Tensor:
    """Per-action logits [B, Amax] gathered from the heads, illegal/pad slots set to -inf."""
    b = place_map.size(0)
    place_flat = place_map.reshape(b, -1)  # [B, 4·169]
    pl = torch.gather(place_flat, 1, batch.a_pidx)  # [B, Amax]
    cl = torch.gather(claim_logits, 1, batch.a_ltok)  # [B, Amax]
    ds = discard.unsqueeze(1).expand(-1, batch.a_type.size(1))  # discard broadcast
    logits = torch.where(batch.a_type == A_PLACE, pl,
                         torch.where(batch.a_type == A_CLAIM, cl, ds))
    return logits.masked_fill(~batch.a_mask, float("-inf"))


def losses(net: KingdominoNet, batch: Batch, value_coef: float):
    """Return (total, policy_ce, value_ce, top1_acc)."""
    place_map, claim_logits, discard, value = net.forward_batch(batch.board, batch.lines, batch.glob)
    logits = gather_logits(place_map, claim_logits, discard, batch)
    logp = torch.nan_to_num(torch.log_softmax(logits, dim=1), neginf=0.0)  # -inf·0 -> 0
    ploss = -(batch.policy * logp).sum(dim=1).mean()
    logv = torch.log_softmax(value[:, : batch.pc], dim=1)
    vloss = -(batch.value_rel * logv).sum(dim=1).mean()
    with torch.no_grad():
        acc = (logits.argmax(1) == batch.policy.argmax(1)).float().mean()
    return ploss + value_coef * vloss, ploss.detach(), vloss.detach(), acc


def iter_batches(records, batch_size, pc, rng, shuffle):
    idx = np.arange(len(records))
    if shuffle:
        rng.shuffle(idx)
    for s in range(0, len(idx), batch_size):
        chunk = [records[j] for j in idx[s : s + batch_size]]
        try:
            yield collate(chunk, pc=pc)
        except ValueError:
            continue  # a batch with no matching-pc records (rare); skip


@torch.no_grad()
def evaluate(net, records, batch_size, pc, value_coef, device):
    net.eval()
    tot = pl = vl = ac = 0.0
    n = 0
    for batch in iter_batches(records, batch_size, pc, None, shuffle=False):
        batch = batch.to(device)
        _, p, v, a = losses(net, batch, value_coef)
        bs = len(batch)
        tot += (p.item() + value_coef * v.item()) * bs
        pl += p.item() * bs
        vl += v.item() * bs
        ac += a.item() * bs
        n += bs
    net.train()
    n = max(n, 1)
    return tot / n, pl / n, vl / n, ac / n


def main():
    ap = argparse.ArgumentParser(description="Train KingdominoNet on a self-play corpus.")
    ap.add_argument("--corpus", required=True, help="training JSONL corpus")
    ap.add_argument("--test", default=None, help="eval corpus; if omitted, hold out --val-frac")
    ap.add_argument("--val-frac", type=float, default=0.1)
    ap.add_argument("--epochs", type=int, default=5)
    ap.add_argument("--batch-size", dest="batch_size", type=int, default=256)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--weight-decay", dest="weight_decay", type=float, default=1e-4)
    ap.add_argument("--value-coef", dest="value_coef", type=float, default=1.0)
    ap.add_argument("--players", type=int, default=2, help="net player count (corpus must match)")
    ap.add_argument("--ch", type=int, default=64, help="conv width")
    ap.add_argument("--board-blocks", dest="board_blocks", type=int, default=3)
    ap.add_argument("--init-from", dest="init_from", default=None, help="warm-start checkpoint")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--limit", type=int, default=None, help="cap training records (smoke tests)")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="runs/net", help="checkpoint prefix (.best.pt / .last.pt)")
    ap.add_argument("--no-save-every-epoch", dest="save_every_epoch", action="store_false",
                     help="skip .epochN.pt per-epoch checkpoints (val loss isn't always playing strength)")
    args = ap.parse_args()

    device = torch.device(args.device)
    rng = np.random.default_rng(args.seed)
    torch.manual_seed(args.seed)

    print("loading corpus...", flush=True)
    records = load_corpus(args.corpus, limit=args.limit)
    if args.test:
        train_recs, test_recs = records, load_corpus(args.test)
    else:
        n_val = max(1, int(len(records) * args.val_frac))
        perm = rng.permutation(len(records))
        test_recs = [records[i] for i in perm[:n_val]]
        train_recs = [records[i] for i in perm[n_val:]]
    print(f"train: {len(train_recs):,} | test: {len(test_recs):,} | batch {args.batch_size} "
          f"| device {device}", flush=True)

    cfg = {"player_count": args.players, "ch": args.ch, "board_blocks": args.board_blocks}
    if args.init_from:
        net, ck = load_net(args.init_from, device)
        cfg = ck.get("net_cfg", cfg) if isinstance(ck, dict) else cfg
        print(f"warm-started from {args.init_from}", flush=True)
    else:
        net = KingdominoNet(**cfg).to(device)
    print(f"net: {cfg} | {net.num_params():,} params", flush=True)

    opt = torch.optim.Adam(net.parameters(), lr=args.lr, weight_decay=args.weight_decay)
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)

    best, best_epoch = float("inf"), 0
    for epoch in range(1, args.epochs + 1):
        net.train()
        t0 = time.time()
        run, steps = 0.0, 0
        for batch in iter_batches(train_recs, args.batch_size, args.players, rng, shuffle=True):
            batch = batch.to(device)
            opt.zero_grad()
            loss, p, v, a = losses(net, batch, args.value_coef)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(net.parameters(), 5.0)
            opt.step()
            run += loss.item()
            steps += 1
        train_loss = run / max(steps, 1)

        val, vp, vv, va = evaluate(net, test_recs, args.batch_size, args.players,
                                   args.value_coef, device)
        dt = time.time() - t0
        is_best = val < best
        if is_best:
            best, best_epoch = val, epoch
        print(f"epoch {epoch}: train {train_loss:.4f} | val {val:.4f} "
              f"(policy {vp:.4f} value {vv:.4f} top1 {va:.3f}) | {dt:.1f}s"
              + ("  <- best" if is_best else ""), flush=True)

        ckpt = {"model": net.state_dict(), "net_cfg": cfg, "epoch": epoch, "val": val,
                "args": vars(args)}
        torch.save(ckpt, f"{args.out}.last.pt")
        if is_best:
            torch.save(ckpt, f"{args.out}.best.pt")
        if args.save_every_epoch:
            torch.save(ckpt, f"{args.out}.epoch{epoch}.pt")

    print(f"done. best val {best:.4f} at epoch {best_epoch} -> {args.out}.best.pt", flush=True)


if __name__ == "__main__":
    main()
