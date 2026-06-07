"""Self-play corpus generation (data only — no trainer/arena yet).

Plays games with MCTS and writes one JSONL record per player decision:
  {obs, legal, policy, to_act, value}
where `obs`/`legal` are the raw bridge outputs (re-encoded at train time so the feature
schema can evolve), `policy` is the MCTS root visit distribution (the policy target),
`to_act` is the deciding seat, and `value` is the game's final per-seat outcome vector
(filled in once the game ends). Raw inputs are stored, mirroring SpaceBase's corpus.

Evaluator:
  --evaluator rollout   pure MCTS + random rollouts (no network) — the default
  --evaluator net --ckpt PATH   MCTS guided by a trained net

Examples (run from agent/):
  .venv/Scripts/python -m kdagent.selfplay --games 20 --sims 64 \
      --out data/selfplay/rollout.jsonl
  .venv/Scripts/python -m kdagent.selfplay --games 50 --sims 128 --no-write   # pure timing
"""
from __future__ import annotations

import argparse
import json
import os
import time

import numpy as np

import kingdomino as kd
from kdagent.mcts import MCTS, NetEvaluator, RolloutEvaluator


def _select_move(policy: np.ndarray, temperature: float, rng: np.random.Generator) -> int:
    """Pick a move from the visit distribution: sample ∝ N^(1/T), or argmax when T≈0."""
    if temperature <= 1e-6:
        return int(np.argmax(policy))
    p = np.power(policy, 1.0 / temperature)
    s = p.sum()
    p = p / s if s > 0 else np.full_like(policy, 1.0 / len(policy))
    return int(rng.choice(len(p), p=p))


def play_game(evaluator, n_sims, players, seed, c_puct, temperature, rng,
              harmony=True, middle_kingdom=True):
    """Play one self-play game; return (records_without_value, value_vector, n_decisions)."""
    g = kd.Game(seed, players, harmony, middle_kingdom)
    mcts = MCTS(evaluator, n_sims=n_sims, c_puct=c_puct, seed=seed)
    records = []
    steps = 0
    while not g.is_terminal():
        steps += 1
        assert steps < 100_000, "game failed to terminate"
        if g.is_chance():
            g.apply_chance()
            continue
        obs = json.loads(g.observation())
        legal = json.loads(g.legal_actions())
        policy, _, _ = mcts.run(g, add_noise=True)
        records.append({"obs": obs, "legal": legal, "policy": policy.tolist(),
                        "to_act": g.to_act()})
        g.apply(_select_move(policy, temperature, rng))
    value = [float(x) for x in g.terminal_value()]
    for r in records:
        r["value"] = value
    return records, value, len(records)


def make_evaluator(args):
    if args.evaluator == "net":
        if not args.ckpt:
            raise SystemExit("--evaluator net requires --ckpt PATH")
        from kdagent.net import load_net

        net, _ = load_net(args.ckpt, device=args.device)
        return NetEvaluator(net, device=args.device)
    return RolloutEvaluator(seed=args.seed)


def _summary(games, decisions, sims, elapsed, out, wrote):
    print("--- self-play summary ---")
    print(f"games={games}  decisions={decisions}  sims={sims}  elapsed={elapsed:.2f}s")
    print(f"throughput: {games / elapsed:.2f} games/s, {decisions / elapsed:.1f} decisions/s, "
          f"{sims / elapsed:.0f} sims/s")
    if wrote:
        print(f"wrote {decisions} records to {out}")


def run_python(args):
    """Single-process Python MCTS (rollout or net evaluator)."""
    evaluator = make_evaluator(args)
    rng = np.random.default_rng(args.seed)
    writer = None
    if not args.no_write:
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        writer = open(args.out, "w", encoding="utf-8")
    t0 = time.perf_counter()
    total = 0
    try:
        for gi in range(args.games):
            records, value, n_dec = play_game(
                evaluator, args.sims, args.players, args.seed + gi, args.c_puct,
                args.temperature, rng, args.harmony, args.middle_kingdom)
            total += n_dec
            if writer is not None:
                for r in records:
                    writer.write(json.dumps(r) + "\n")
            print(f"  game {gi + 1}/{args.games}: {n_dec} decisions, value {value}")
    finally:
        if writer is not None:
            writer.close()
    elapsed = time.perf_counter() - t0
    _summary(args.games, total, total * args.sims, elapsed, args.out, not args.no_write)


def run_rust(args):
    """Pure-Rust rollout MCTS, batched across cores (no network). The whole batch runs in one
    call with the GIL released, so this is the fast path for corpus generation / perf testing."""
    t0 = time.perf_counter()
    lines = kd.selfplay_batch(
        n_games=args.games, players=args.players, n_sims=args.sims, c_puct=args.c_puct,
        temp_moves=args.temp_moves, dirichlet_alpha=args.dirichlet_alpha,
        noise_eps=args.noise_eps, seed=args.seed,
        harmony=args.harmony, middle_kingdom=args.middle_kingdom)
    elapsed = time.perf_counter() - t0
    if not args.no_write:
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        with open(args.out, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + ("\n" if lines else ""))
    _summary(args.games, len(lines), len(lines) * args.sims, elapsed, args.out, not args.no_write)


def main():
    ap = argparse.ArgumentParser(description="Generate a Kingdomino self-play corpus.")
    ap.add_argument("--backend", choices=["python", "rust"], default="rust",
                    help="rust = batched pure-Rust rollout MCTS (fast); python = single-process")
    ap.add_argument("--games", type=int, default=10)
    ap.add_argument("--sims", type=int, default=64, help="MCTS simulations per move")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--evaluator", choices=["rollout", "net"], default="rollout",
                    help="python backend only; rust is always rollout")
    ap.add_argument("--ckpt", default=None, help="net checkpoint (python --evaluator net)")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--c-puct", dest="c_puct", type=float, default=1.5)
    ap.add_argument("--temperature", type=float, default=1.0, help="python backend move temp")
    ap.add_argument("--temp-moves", dest="temp_moves", type=int, default=12,
                    help="rust backend: first N moves sampled at temperature 1, then greedy")
    ap.add_argument("--dirichlet-alpha", dest="dirichlet_alpha", type=float, default=0.3)
    ap.add_argument("--noise-eps", dest="noise_eps", type=float, default=0.25)
    ap.add_argument("--harmony", action=argparse.BooleanOptionalAction, default=True)
    ap.add_argument("--middle-kingdom", dest="middle_kingdom",
                    action=argparse.BooleanOptionalAction, default=True)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="data/selfplay/corpus.jsonl")
    ap.add_argument("--no-write", action="store_true", help="skip writing (pure timing)")
    args = ap.parse_args()

    if args.backend == "rust":
        run_rust(args)
    else:
        run_python(args)


if __name__ == "__main__":
    main()
