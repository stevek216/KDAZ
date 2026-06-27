"""Self-play inference benchmark — decide the net-MCTS evaluation backend on evidence.

Measures the three costs that dominate batched net self-play for our (tiny) net:
  1. net forward throughput vs batch size  (GPU resident / GPU incl. transfer / CPU / bf16)
  2. host->device transfer cost            (f32 planes vs compact int8)
  3. the current Python encode rate        (the JSON->numpy path we'd replace with Rust)

Run (from agent/):  .venv/Scripts/python -m kdagent.bench
"""
from __future__ import annotations

import json
import time

import numpy as np
import torch

import kingdomino as kd
from kdagent.encoder import LINE_FEATS, N_PLANES, encode_obs, global_dim

PC = 2
C = N_PLANES
F = LINE_FEATS
G = global_dim(PC)
BATCHES = [1, 8, 32, 128, 512, 1024, 2048, 4096]


def _run_for(fn, seconds=0.4, warmup=8):
    """Call fn() warmup times, then repeatedly for ~`seconds`; return calls/sec."""
    for _ in range(warmup):
        fn()
    if torch.cuda.is_available():
        torch.cuda.synchronize()
    t0 = time.perf_counter()
    n = 0
    while time.perf_counter() - t0 < seconds:
        fn()
        n += 1
    if torch.cuda.is_available():
        torch.cuda.synchronize()
    return n / (time.perf_counter() - t0)


def make_net(device, dtype=torch.float32):
    from kdagent.net import KingdominoNet
    return KingdominoNet(player_count=PC).to(device).to(dtype).eval()


def dev_inputs(b, device, dtype=torch.float32):
    return (torch.rand(b, PC * C, 13, 13, device=device, dtype=dtype),
            torch.rand(b, 8, F, device=device, dtype=dtype),
            torch.rand(b, G, device=device, dtype=dtype))


@torch.no_grad()
def bench_forward():
    print("\n=== net forward throughput (samples/sec) ===")
    print(f"{'batch':>6} | {'cpu':>12} | {'cuda resident':>14} | {'cuda+xfer f32':>14} | {'cuda bf16 res':>14}")
    cuda = torch.cuda.is_available()
    cpu_net = make_net("cpu")
    gpu_net = make_net("cuda") if cuda else None
    gpu_net_bf = make_net("cuda", torch.bfloat16) if cuda else None
    for b in BATCHES:
        # CPU (skip the largest batches — they're slow and not the candidate region)
        cpu_sps = ""
        if b <= 1024:
            ci = dev_inputs(b, "cpu")
            cpu_sps = f"{b * _run_for(lambda: cpu_net.forward_batch(*ci)):>12,.0f}"
        res = xfer = bf = ""
        if cuda:
            gi = dev_inputs(b, "cuda")
            res = f"{b * _run_for(lambda: gpu_net.forward_batch(*gi)):>14,.0f}"
            # realistic: build on CPU (numpy), upload, forward
            host = [a.cpu().numpy() for a in dev_inputs(b, "cpu")]

            def withxfer():
                t = [torch.from_numpy(a).to("cuda", non_blocking=True) for a in host]
                gpu_net.forward_batch(*t)
            xfer = f"{b * _run_for(withxfer):>14,.0f}"
            gbi = dev_inputs(b, "cuda", torch.bfloat16)
            bf = f"{b * _run_for(lambda: gpu_net_bf.forward_batch(*gbi)):>14,.0f}"
        print(f"{b:>6} | {cpu_sps:>12} | {res:>14} | {xfer:>14} | {bf:>14}")


def bench_transfer():
    if not torch.cuda.is_available():
        return
    print("\n=== host->device transfer (ms per batch) — f32 planes vs compact int8 ===")
    f32_bytes = PC * C * 13 * 13 * 4
    i8_bytes = 13 * 13 * 2  # per leaf: per-cell (terrain, crowns) int8, both seats ~ x2
    print(f"  per-leaf: f32 planes = {f32_bytes:,} B   compact int8 = {i8_bytes * 2:,} B   "
          f"({f32_bytes / (i8_bytes * 2):.0f}x)")
    print(f"{'batch':>6} | {'f32 upload ms':>14} | {'int8 upload ms':>15}")
    for b in BATCHES:
        f32 = np.random.rand(b, PC * C, 13, 13).astype(np.float32)
        i8 = np.random.randint(0, 8, (b, 2, 13, 13), dtype=np.uint8)
        f32_ms = 1000.0 / _run_for(lambda: torch.from_numpy(f32).to("cuda", non_blocking=True))
        i8_ms = 1000.0 / _run_for(lambda: torch.from_numpy(i8).to("cuda", non_blocking=True))
        print(f"{b:>6} | {f32_ms:>14.3f} | {i8_ms:>15.3f}")


def bench_encode():
    print("\n=== current Python encode rate (encode_obs, the JSON->numpy path) ===")
    recs = [json.loads(l) for l in kd.selfplay_batch(n_games=20, players=2, n_sims=8)]
    recs = recs[:512]
    t0 = time.perf_counter()
    reps = 0
    while time.perf_counter() - t0 < 1.0:
        for r in recs:
            encode_obs(r["obs"], r["legal"])
        reps += len(recs)
    rate = reps / (time.perf_counter() - t0)
    print(f"  encode_obs: {rate:,.0f} states/sec  ({1e6 / rate:.1f} us/state) over {len(recs)} distinct states")


def main():
    print(f"torch {torch.__version__}  cuda={torch.cuda.is_available()}"
          + (f" ({torch.cuda.get_device_name(0)})" if torch.cuda.is_available() else ""))
    print(f"net: KingdominoNet(player_count={PC})  inputs: board[B,{PC * C},13,13] lines[B,8,{F}] glob[B,{G}]")
    bench_forward()
    bench_transfer()
    bench_encode()


if __name__ == "__main__":
    main()
