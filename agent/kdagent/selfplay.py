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


def _gather_logits(place_map, claim_logits, discard, batch, device):
    """Per-action logits [B, Amax] from the net heads + the pool's action descriptors
    (mirrors kdagent.dataset.gather_logits / net.policy_value)."""
    import torch

    from kdagent.encoder import A_CLAIM, A_PLACE
    b = place_map.size(0)
    a_type = torch.from_numpy(batch["a_type"]).to(device)
    a_pidx = torch.from_numpy(batch["a_pidx"]).to(device).clamp(min=0)
    a_ltok = torch.from_numpy(batch["a_ltok"]).to(device).clamp(min=0)
    a_mask = torch.from_numpy(batch["a_mask"]).to(device).bool()
    pl = torch.gather(place_map.reshape(b, -1), 1, a_pidx)
    cl = torch.gather(claim_logits, 1, a_ltok)
    ds = discard.unsqueeze(1).expand(-1, a_type.size(1))
    logits = torch.where(a_type == A_PLACE, pl, torch.where(a_type == A_CLAIM, cl, ds))
    return logits.masked_fill(~a_mask, float("-inf"))


def run_netbatch(args):
    """Batched net self-play: a Rust pool of games (encoding + trees), one bf16 GPU forward
    per round. Net from --ckpt, or a fresh (random) net for perf measurement."""
    import torch

    from kdagent.net import KingdominoNet, load_net
    device = args.device
    if args.ckpt:
        net, _ = load_net(args.ckpt, device)
    else:
        net = KingdominoNet(player_count=args.players).to(device).eval()
    use_amp = str(device).startswith("cuda")
    if use_amp:  # channels_last speeds the small-conv forward at large batch (~27% @2048)
        net = net.to(memory_format=torch.channels_last)

    pool = kd.BatchedNetSelfPlay(
        n_games=args.concurrent, total_games=args.games, players=args.players, n_sims=args.sims,
        c_puct=args.c_puct, temp_moves=args.temp_moves, dirichlet_alpha=args.dirichlet_alpha,
        noise_eps=args.noise_eps, seed=args.seed,
        harmony=args.harmony, middle_kingdom=args.middle_kingdom)
    writer = None
    if not args.no_write:
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        writer = open(args.out, "w", encoding="utf-8")

    prof = args.profile
    cuda = str(device).startswith("cuda")
    sync = (lambda: torch.cuda.synchronize()) if (prof and cuda) else (lambda: None)
    ph = {"collect": 0.0, "to_dev": 0.0, "forward": 0.0, "apply": 0.0}
    rounds = 0

    t0, total_dec, total_leaves = time.perf_counter(), 0, 0
    last = t0
    while not pool.done():
        sync(); ta = time.perf_counter()
        batch = pool.collect()
        b = batch["b"]
        if prof:
            ph["collect"] += time.perf_counter() - ta
        if b > 0:
            total_leaves += b
            rounds += 1
            sync(); ta = time.perf_counter()
            board = torch.from_numpy(batch["board"]).to(device, non_blocking=True).float()
            if use_amp:  # match the channels_last net
                board = board.to(memory_format=torch.channels_last)
            lines = torch.from_numpy(batch["lines"]).to(device, non_blocking=True)
            glob = torch.from_numpy(batch["glob"]).to(device, non_blocking=True)
            if prof:
                sync(); ph["to_dev"] += time.perf_counter() - ta; ta = time.perf_counter()
            with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16, enabled=use_amp):
                place_map, claim_logits, discard, value = net.forward_batch(board, lines, glob)
                logits = _gather_logits(place_map, claim_logits, discard, batch, device)
                value_rel = torch.softmax(value.float(), dim=1)
            if prof:
                sync(); ph["forward"] += time.perf_counter() - ta; ta = time.perf_counter()
            pool.apply(logits.float().cpu().numpy(), value_rel.cpu().numpy())
            if prof:
                ph["apply"] += time.perf_counter() - ta
        out_lines = pool.drain()
        if out_lines:
            total_dec += len(out_lines)
            if writer is not None:
                writer.write("\n".join(out_lines) + "\n")
        if time.perf_counter() - last > 1.0:
            done, tot = pool.stats()
            el = time.perf_counter() - t0
            print(f"  {done}/{tot} games | {done / el:.1f} games/s | "
                  f"{total_leaves / el:,.0f} leaf-evals/s", flush=True, end="\r")
            last = time.perf_counter()
    if writer is not None:
        writer.close()
    el = time.perf_counter() - t0
    print()
    _summary(args.games, total_dec, total_leaves, el, args.out, not args.no_write)
    print(f"  leaf-evals: {total_leaves:,} ({total_leaves / el:,.0f}/sec)")
    if prof and rounds:
        tot_ms = sum(ph.values()) / rounds * 1000
        print(f"  per-round breakdown over {rounds} rounds ({tot_ms:.2f} ms/round):")
        for k, v in ph.items():
            ms = v / rounds * 1000
            print(f"    {k:>8}: {ms:6.2f} ms ({100 * v / sum(ph.values()):4.1f}%)")


def _gpu_forward(net, batch, device, use_amp):
    """H2D + forward + per-action gather + value softmax, all on GPU. Returns
    `(logits, value_rel)` still *on the GPU* and un-synced — the caller forces the sync later
    by moving them to CPU, which is what lets a CPU `collect()` run concurrently with this."""
    import torch

    board = torch.from_numpy(batch["board"]).to(device, non_blocking=True).float()
    if use_amp:  # match the channels_last net (set in the caller)
        board = board.to(memory_format=torch.channels_last)
    lines = torch.from_numpy(batch["lines"]).to(device, non_blocking=True)
    glob = torch.from_numpy(batch["glob"]).to(device, non_blocking=True)
    with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16, enabled=use_amp):
        place_map, claim_logits, discard, value = net.forward_batch(board, lines, glob)
        logits = _gather_logits(place_map, claim_logits, discard, batch, device)
        value_rel = torch.softmax(value.float(), dim=1)
    return logits.float(), value_rel


def run_netbatch_overlap(args):
    """Pipelined net self-play: two independent game pools ping-pong so the CPU `collect()` of
    one pool overlaps the GPU forward of the other (the phases live on independent hardware and
    are otherwise serialized). Each pool plays half of `--games` from a distinct seed stream, so
    the union is a deterministic, statistically-equivalent corpus. ~1.5x over `run_netbatch`."""
    import torch

    from kdagent.net import KingdominoNet, load_net
    device = args.device
    if args.ckpt:
        net, _ = load_net(args.ckpt, device)
    else:
        net = KingdominoNet(player_count=args.players).to(device).eval()
    use_amp = str(device).startswith("cuda")
    if use_amp:  # channels_last speeds the small-conv forward at large batch (~27% @2048)
        net = net.to(memory_format=torch.channels_last)

    def make_pool(total, seed):
        return kd.BatchedNetSelfPlay(
            n_games=args.concurrent, total_games=total, players=args.players, n_sims=args.sims,
            c_puct=args.c_puct, temp_moves=args.temp_moves, dirichlet_alpha=args.dirichlet_alpha,
            noise_eps=args.noise_eps, seed=seed,
            harmony=args.harmony, middle_kingdom=args.middle_kingdom)

    games_a = (args.games + 1) // 2
    games_b = args.games // 2
    pools = [make_pool(games_a, args.seed),
             make_pool(games_b, args.seed ^ 0x9E37_79B9_7F4A_7C15)]

    writer = None
    if not args.no_write:
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        writer = open(args.out, "w", encoding="utf-8")

    def drain(pi):
        nonlocal total_dec
        out = pools[pi].drain()
        if out:
            total_dec += len(out)
            if writer is not None:
                writer.write("\n".join(out) + "\n")

    from concurrent.futures import ThreadPoolExecutor

    prof = args.profile
    ph = {"gpu": 0.0, "drain": 0.0, "collect_wait": 0.0}
    rounds = 0
    worker = ThreadPoolExecutor(max_workers=1)

    t0, total_dec, total_leaves = time.perf_counter(), 0, 0
    last = t0
    exhausted = [False, False]

    # Prime: collect pool 0's first batch on the main thread.
    cur = 0
    batch = pools[cur].collect()
    exhausted[cur] = batch["b"] == 0
    total_leaves += batch["b"]

    while True:
        other = 1 - cur
        rounds += 1
        # Hand the OTHER pool's collect to the worker thread (Rust's pump/encode release the GIL),
        # so it runs concurrently with this thread's full GPU path on `cur`.
        fut = worker.submit(pools[other].collect)
        ta = time.perf_counter()
        # Main thread: H2D + forward + sync + backup for `cur`. The synchronous H2D releases the
        # GIL, so the worker's collect genuinely overlaps it (that is the whole point).
        if batch["b"] > 0:
            gpu = _gpu_forward(net, batch, device, use_amp)
            pools[cur].apply(gpu[0].cpu().numpy(), gpu[1].cpu().numpy())
        if prof:
            ph["gpu"] += time.perf_counter() - ta; ta = time.perf_counter()
        drain(cur)
        if prof:
            ph["drain"] += time.perf_counter() - ta; ta = time.perf_counter()
        # Join the worker (collect_wait ≈ 0 means the collect was fully hidden behind the GPU).
        obatch = fut.result()
        if prof:
            ph["collect_wait"] += time.perf_counter() - ta
        exhausted[other] = obatch["b"] == 0
        total_leaves += obatch["b"]
        if exhausted[0] and exhausted[1]:
            break
        batch, cur = obatch, other

        if time.perf_counter() - last > 1.0:
            done = sum(p.stats()[0] for p in pools)
            el = time.perf_counter() - t0
            print(f"  {done}/{args.games} games | {done / el:.1f} games/s | "
                  f"{total_leaves / el:,.0f} leaf-evals/s", flush=True, end="\r")
            last = time.perf_counter()

    worker.shutdown()
    drain(0)
    drain(1)
    if writer is not None:
        writer.close()
    el = time.perf_counter() - t0
    print()
    _summary(args.games, total_dec, total_leaves, el, args.out, not args.no_write)
    print(f"  leaf-evals: {total_leaves:,} ({total_leaves / el:,.0f}/sec)")
    if prof and rounds:
        tot_ms = sum(ph.values()) / rounds * 1000
        print(f"  per-round breakdown over {rounds} rounds ({tot_ms:.2f} ms/round):")
        for k, v in ph.items():
            print(f"    {k:>8}: {v / rounds * 1000:6.2f} ms ({100 * v / sum(ph.values()):4.1f}%)")


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
    ap.add_argument("--backend", choices=["python", "rust", "netbatch"], default="rust",
                    help="rust = batched rollout MCTS; netbatch = batched net self-play "
                         "(Rust pool + one GPU forward/round); python = single-process")
    ap.add_argument("--games", type=int, default=10)
    ap.add_argument("--sims", type=int, default=64, help="MCTS simulations per move")
    ap.add_argument("--concurrent", type=int, default=1024, help="netbatch: games in flight (batch)")
    ap.add_argument("--profile", action="store_true", help="netbatch: per-round phase timing breakdown")
    ap.add_argument("--overlap", action="store_true",
                    help="netbatch: two-pool pipeline (CPU collect overlaps GPU forward)")
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
    elif args.backend == "netbatch":
        if args.overlap:
            run_netbatch_overlap(args)
        else:
            run_netbatch(args)
    else:
        run_python(args)


if __name__ == "__main__":
    main()
