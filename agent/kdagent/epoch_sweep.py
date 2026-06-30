"""Sweep the batched arena over every per-epoch checkpoint of a training run, vs a fixed
opponent. Pairs with `train.py --save-every-epoch` (now the default): once a run has left
behind `PREFIX.epochN.pt` files, this answers "which epoch actually plays best?" instead of
trusting val loss (see CLAUDE.md discussion — val loss and playing strength can diverge).

By default each epoch is played as `netmcts:SIMS:CKPT` at the opponent's own sims (`net:CKPT`
if the opponent is the raw net) — only the checkpoint differs. Pass `--sims` to use different
candidate sims instead (a single value, or a comma list to grid-sweep epoch x sims); `--sims 0`
means the candidate plays its raw net policy, no search.

    cd agent
    .venv/Scripts/python -m kdagent.epoch_sweep --prefix runs/gen0 \
        --opponent netmcts:32:runs/gen0.best.pt --games 1500 --device cuda
    .venv/Scripts/python -m kdagent.epoch_sweep --prefix runs/gen0 --sims 16,32,64 \
        --opponent netmcts:32:runs/gen0.best.pt --games 1500 --device cuda
"""
from __future__ import annotations

import argparse
import re
from pathlib import Path

from .arena import run_batched_arena

EPOCH_RE = re.compile(r"\.epoch(\d+)\.pt$")


def find_epoch_checkpoints(prefix):
    """All `{prefix}.epochN.pt` next to `prefix`, sorted numerically by N."""
    prefix_path = Path(prefix)
    found = []
    for p in prefix_path.parent.glob(f"{prefix_path.name}.epoch*.pt"):
        m = EPOCH_RE.search(p.name)
        if m:
            found.append((int(m.group(1)), p))
    found.sort(key=lambda t: t[0])
    return found


def opponent_sims(opponent_spec):
    """Sims encoded in the opponent spec (0 for a raw `net:CKPT`)."""
    if opponent_spec.startswith("netmcts:"):
        return int(opponent_spec.split(":", 2)[1])
    if opponent_spec.startswith("net:"):
        return 0
    raise SystemExit(f"--opponent must be netmcts:SIMS:CKPT or net:CKPT, got {opponent_spec!r}")


def hero_spec(ckpt_path, sims):
    """`sims=0` plays the raw net policy (no search); otherwise netmcts at this sim count."""
    return f"net:{ckpt_path}" if sims == 0 else f"netmcts:{sims}:{ckpt_path}"


def main():
    ap = argparse.ArgumentParser(
        description="Batched arena sweep: every epoch checkpoint of a run vs a fixed opponent.")
    ap.add_argument("--prefix", required=True,
                    help="checkpoint prefix (same value as train.py's --out), e.g. runs/gen0")
    ap.add_argument("--opponent", required=True,
                    help="fixed field agent: netmcts:SIMS:CKPT or net:CKPT")
    ap.add_argument("--sims", default=None,
                    help="candidate sim count(s), comma-separated (e.g. 16,32,64); "
                         "0 = raw net policy; default: match --opponent's sims")
    ap.add_argument("--games", type=int, default=1000)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--device", default="cpu", help="torch device for net agents (e.g. cuda)")
    ap.add_argument("--concurrent", type=int, default=256, help="games in flight")
    ap.add_argument("--c-puct", dest="c_puct", type=float, default=1.5)
    ap.add_argument("--harmony", action=argparse.BooleanOptionalAction, default=True)
    ap.add_argument("--middle-kingdom", dest="middle_kingdom",
                    action=argparse.BooleanOptionalAction, default=True)
    args = ap.parse_args()

    checkpoints = find_epoch_checkpoints(args.prefix)
    if not checkpoints:
        raise SystemExit(f"no {args.prefix}.epochN.pt checkpoints found")
    sims_list = ([opponent_sims(args.opponent)] if args.sims is None
                 else [int(s) for s in args.sims.split(",")])

    results = []
    for epoch, ckpt in checkpoints:
        for sims in sims_list:
            args.a = hero_spec(ckpt, sims)
            args.b = args.opponent
            print(f"\n=== epoch {epoch} ({ckpt.name}), sims {sims} ===", flush=True)
            mean, ci, n, verdict = run_batched_arena(args)
            results.append((epoch, sims, mean, ci, n, verdict))

    print("\nepoch   sims   win%    +/-      n  verdict")
    for epoch, sims, mean, ci, n, verdict in results:
        print(f"{epoch:5d}  {sims:5d}  {mean * 100:5.1f}  {ci * 100:5.1f}  {n:5d}  {verdict}")
    best_epoch, best_sims, best_mean, *_ = max(results, key=lambda r: r[2])
    print(f"\nbest: epoch {best_epoch}, sims {best_sims} ({best_mean * 100:.1f}%)")


if __name__ == "__main__":
    main()
