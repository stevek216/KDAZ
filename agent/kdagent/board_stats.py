"""Final-board quality analysis across checkpoints: self-play a batch of games per agent and
aggregate terminal-board statistics — score totals, Harmony / Middle Kingdom rates, board
fill, largest territory. The direct read on whether generations are building better kingdoms
(win-rate arenas only show *relative* strength).

Each net plays greedy self-play (temp-moves 0, no Dirichlet noise by default) so the only
variance is the hidden deck; the rollout arm (`--rollout`) is the no-net MCTS baseline.
Summaries come from the engine's terminal observation (single source of truth for scoring).

    cd agent
    .venv/Scripts/python -m kdagent.board_stats --nets runs/gen0.best.pt,runs/gen1.best.pt \
        --rollout --games 1000 --sims 256 --device cuda
"""
from __future__ import annotations

import argparse
import json
import os
import time

import numpy as np

import kingdomino as kd


def net_selfplay_summaries(ckpt, args):
    """Greedy batched self-play with one net on both seats; returns terminal-obs JSON lines."""
    import torch

    from kdagent.net import load_net
    from kdagent.selfplay import _gpu_forward

    device = args.device
    net, _ = load_net(ckpt, device)
    use_amp = str(device).startswith("cuda")
    if use_amp:
        net = net.to(memory_format=torch.channels_last)
    pool = kd.BatchedNetSelfPlay(
        n_games=args.concurrent, total_games=args.games, players=2, n_sims=args.sims,
        c_puct=args.c_puct, temp_moves=args.temp_moves, dirichlet_alpha=args.dirichlet_alpha,
        noise_eps=args.noise_eps, seed=args.seed, harmony=True, middle_kingdom=True,
        summaries=True)
    summaries = []
    t0, last = time.perf_counter(), time.perf_counter()
    while not pool.done():
        batch = pool.collect()
        if batch["b"] > 0:
            logits, value_rel = _gpu_forward(net, batch, device, use_amp)
            pool.apply(logits.cpu().numpy(), value_rel.cpu().numpy())
        pool.drain_packed()  # corpus not needed here — drained only to free the buffers
        summaries += pool.drain_summaries()
        if time.perf_counter() - last > 2.0:
            done, tot = pool.stats()
            el = time.perf_counter() - t0
            print(f"  {done}/{tot} games | {done / el:.1f} games/s", flush=True, end="\r")
            last = time.perf_counter()
    summaries += pool.drain_summaries()
    print(flush=True)
    return summaries


def rollout_summaries(args):
    print("  rollout MCTS (all cores)...", flush=True)
    return kd.rollout_summaries(
        n_games=args.games, players=2, n_sims=args.sims, c_puct=args.c_puct,
        temp_moves=args.temp_moves, dirichlet_alpha=args.dirichlet_alpha,
        noise_eps=args.noise_eps, seed=args.seed, harmony=True, middle_kingdom=True)


def aggregate(summaries):
    """Per-board and per-game stats from terminal observations."""
    totals, crowns, largest, filled = [], [], [], []
    harmony_hits = middle_hits = complete = 0
    margins, ties = [], 0
    for line in summaries:
        obs = json.loads(line)
        game_totals = []
        for s in range(obs["player_count"]):
            sc, seat = obs["scores"][s], obs["seats"][s]
            totals.append(sc["total"])
            crowns.append(sc["crown_score"])
            largest.append(sc["largest_territory"])
            filled.append(seat["filled"])
            harmony_hits += sc["harmony"] > 0
            middle_hits += sc["middle_kingdom"] > 0
            complete += seat["filled"] == 48
            game_totals.append(sc["total"])
        a, b = sorted(game_totals, reverse=True)[:2]
        margins.append(a - b)
        ties += a == b
    n = len(totals)
    return {
        "games": len(summaries),
        "boards": n,
        "score_mean": float(np.mean(totals)),
        "score_std": float(np.std(totals)),
        "score_median": float(np.median(totals)),
        "score_max": int(np.max(totals)),
        "crown_mean": float(np.mean(crowns)),
        "harmony_pct": 100.0 * harmony_hits / n,
        "middle_pct": 100.0 * middle_hits / n,
        "incomplete_pct": 100.0 * (n - complete) / n,
        "filled_mean": float(np.mean(filled)),
        "discards_mean": float(np.mean((48 - np.asarray(filled)) / 2)),
        "largest_mean": float(np.mean(largest)),
        "margin_mean": float(np.mean(margins)),
        "tie_pct": 100.0 * ties / len(summaries),
    }


COLS = [
    ("score_mean", "score", "{:.1f}"),
    ("score_median", "med", "{:.0f}"),
    ("score_max", "max", "{:d}"),
    ("crown_mean", "crowns", "{:.1f}"),
    ("harmony_pct", "harm%", "{:.1f}"),
    ("middle_pct", "midk%", "{:.1f}"),
    ("incomplete_pct", "gap%", "{:.1f}"),
    ("discards_mean", "disc", "{:.2f}"),
    ("largest_mean", "large", "{:.1f}"),
    ("margin_mean", "margin", "{:.1f}"),
    ("tie_pct", "tie%", "{:.1f}"),
]


def main():
    ap = argparse.ArgumentParser(description="Final-board quality stats across checkpoints.")
    ap.add_argument("--nets", default="", help="comma-separated checkpoint paths")
    ap.add_argument("--rollout", action="store_true", help="include a no-net rollout-MCTS arm")
    ap.add_argument("--games", type=int, default=1000)
    ap.add_argument("--sims", type=int, default=256)
    ap.add_argument("--concurrent", type=int, default=512)
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--c-puct", dest="c_puct", type=float, default=1.5)
    ap.add_argument("--temp-moves", dest="temp_moves", type=int, default=0,
                    help="sampled moves before greedy (0 = fully greedy analysis play)")
    ap.add_argument("--dirichlet-alpha", dest="dirichlet_alpha", type=float, default=0.3)
    ap.add_argument("--noise-eps", dest="noise_eps", type=float, default=0.0,
                    help="root noise weight (0 = off for analysis play)")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--dump", default=None,
                    help="optional dir to dump each arm's raw summary JSONL")
    ap.add_argument("--value-blend", dest="value_blend", type=float, default=0.0,
                    help="blend the score head into the search's leaf value: "
                         "(1-a)*P(win) + a*sigmoid(cal*margin). 0 = pure value head. "
                         "gen11 @512 sims: a=0.7 scores 52.0%% +/- 1.0 over 10k games vs a=0.")
    ap.add_argument("--score-cal", dest="score_cal", type=float, default=None,
                    help="margin->winprob scale for --value-blend (default 6.139)")
    args = ap.parse_args()
    from kdagent.selfplay import set_value_blend
    set_value_blend(args.value_blend, args.score_cal)

    arms = []
    if args.rollout:
        arms.append(("rollout", None))
    arms += [(os.path.basename(p), p) for p in args.nets.split(",") if p.strip()]
    if not arms:
        raise SystemExit("nothing to run: pass --nets and/or --rollout")

    results = []
    for name, ckpt in arms:
        print(f"=== {name}  ({args.games} games, {args.sims} sims, greedy) ===", flush=True)
        t0 = time.perf_counter()
        summaries = rollout_summaries(args) if ckpt is None else net_selfplay_summaries(ckpt, args)
        print(f"  done in {time.perf_counter() - t0:.1f}s ({len(summaries)} games)", flush=True)
        if args.dump:
            os.makedirs(args.dump, exist_ok=True)
            with open(os.path.join(args.dump, f"{name}.jsonl"), "w", encoding="utf-8") as f:
                f.write("\n".join(summaries) + "\n")
        results.append((name, aggregate(summaries)))

    name_w = max(len(n) for n, _ in results)
    print(f"\n{'agent':<{name_w}}  " + "  ".join(f"{h:>6}" for _, h, _ in COLS))
    for name, r in results:
        print(f"{name:<{name_w}}  " + "  ".join(f.format(r[k]).rjust(6) for k, _, f in COLS))
    print(f"\n(per-board stats over {results[0][1]['boards']} boards/agent; score incl. bonuses; "
          f"harm%=Harmony +5, midk%=Middle Kingdom +10, gap%=boards not fully filled, "
          f"disc=mean discarded dominoes, large=mean largest territory)")


if __name__ == "__main__":
    main()
