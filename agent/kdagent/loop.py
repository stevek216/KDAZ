"""The closed self-play training loop: generate -> train -> evaluate -> promote, repeated.

Each attempt: (1) self-play a corpus from the current champion (netbatch --overlap);
(2) train a candidate on the most recent 2 corpora (fresh net, per-epoch checkpoints);
(3) epoch-sweep every checkpoint against the champion in the batched arena;
(4) promote the best epoch to runs/gen{N+1}.best.pt iff its win-rate lower confidence
bound clears 50% (statistically better, not just lucky). Failures regenerate with fresh
seeds; --max-fails consecutive failures (default 3) end the run when --gens isn't given.

State lives in {runs}/loop_state.json (attempts, corpora, history) so an interrupted or
killed run resumes at the phase it died in. Ctrl+C prints a standings summary and saves
state. --notify URL gets a plain-text POST when the run ends for any reason (works with
ntfy.sh topics: install the app, pick a topic, pass https://ntfy.sh/<topic>).

    cd agent
    .venv/Scripts/python -m kdagent.loop --device cuda --notify https://ntfy.sh/my-topic
    .venv/Scripts/python -m kdagent.loop --gens 3 --games 10000 --sims 256 --device cuda
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

GEN_RE = re.compile(r"^gen(\d+)\.best\.pt$")


# --------------------------------------------------------------------------- state
def load_state(path: Path) -> dict:
    if path.exists():
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    return {"attempt_no": 0, "fails": 0, "corpora": [], "history": [], "current": None}


def save_state(state: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".json.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=2)
    tmp.replace(path)


def find_champion(runs_dir: Path) -> tuple[int, Path]:
    """Highest gen{N}.best.pt in the runs dir — the current champion."""
    best = None
    for p in runs_dir.glob("gen*.best.pt"):
        m = GEN_RE.match(p.name)
        if m:
            n = int(m.group(1))
            if best is None or n > best[0]:
                best = (n, p)
    if best is None:
        raise SystemExit(f"no gen*.best.pt champion found in {runs_dir} — "
                         "seed the loop with a gen0 (e.g. train on a rollout corpus)")
    return best


# --------------------------------------------------------------------------- phases
def run_phase(name: str, cmd: list[str]) -> float:
    """Run one phase as a subprocess (streamed to the console); return its wall seconds."""
    print(f"\n=== {name}: {' '.join(cmd)} ===", flush=True)
    t0 = time.time()
    proc = subprocess.Popen(cmd)
    try:
        rc = proc.wait()
    except KeyboardInterrupt:
        proc.terminate()
        proc.wait()
        raise
    if rc != 0:
        raise RuntimeError(f"{name} failed with exit code {rc}")
    return time.time() - t0


def notify(url: str | None, message: str) -> None:
    if not url:
        return
    try:
        req = urllib.request.Request(
            url, data=message.encode("utf-8"),
            headers={"Title": "Kingdomino training loop"}, method="POST")
        urllib.request.urlopen(req, timeout=15)
        print(f"(notified {url})", flush=True)
    except Exception as e:  # notification failure must never kill the run
        print(f"(notify failed: {e})", flush=True)


def summary_text(state: dict, runs_dir: Path) -> str:
    lines = ["gen   attempt  games   best-epoch  win%   +/-   promoted"]
    for h in state["history"]:
        lines.append(f"{h['candidate']:3d}  {h['attempt']:7d}  {h['games']:6d}  "
                     f"{h['best_epoch']:10d}  {h['mean'] * 100:5.1f}  {h['ci'] * 100:4.1f}"
                     f"   {'yes' if h['promoted'] else 'no'}")
    champ, _ = find_champion(runs_dir)
    lines.append(f"champion: gen{champ} | consecutive fails: {state['fails']}"
                 f" | attempts: {state['attempt_no']}")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser(description="Closed generate->train->evaluate->promote loop.")
    ap.add_argument("--gens", type=int, default=None,
                    help="stop after N successful promotions (default: run until "
                         "--max-fails consecutive promotion failures)")
    ap.add_argument("--max-fails", dest="max_fails", type=int, default=3,
                    help="consecutive promotion failures that end the run")
    ap.add_argument("--games", type=int, default=10_000, help="self-play games per corpus")
    ap.add_argument("--sims", type=int, default=256, help="MCTS sims per move in self-play")
    ap.add_argument("--eval-games", dest="eval_games", type=int, default=1000,
                    help="arena games per epoch in the promotion sweep")
    ap.add_argument("--eval-sims", dest="eval_sims", type=int, default=None,
                    help="arena sims (default: --sims)")
    ap.add_argument("--epochs", type=int, default=5)
    ap.add_argument("--batch-size", dest="batch_size", type=int, default=512)
    ap.add_argument("--concurrent", type=int, default=2048, help="games in flight (GPU batch)")
    ap.add_argument("--device", default="cuda")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--runs-dir", dest="runs_dir", default="runs")
    ap.add_argument("--data-dir", dest="data_dir", default="data/selfplay")
    ap.add_argument("--notify", default=None,
                    help="URL POSTed a plain-text summary when the run ends (e.g. ntfy.sh topic)")
    args = ap.parse_args()
    eval_sims = args.eval_sims if args.eval_sims is not None else args.sims

    runs_dir, data_dir = Path(args.runs_dir), Path(args.data_dir)
    runs_dir.mkdir(parents=True, exist_ok=True)
    data_dir.mkdir(parents=True, exist_ok=True)
    state_path = runs_dir / "loop_state.json"
    state = load_state(state_path)
    py = sys.executable
    promotions_this_run = 0
    stop_reason = "unknown"

    try:
        while True:
            champ_gen, champ_path = find_champion(runs_dir)
            candidate = champ_gen + 1

            # Resume an in-flight attempt only if it targets the current champion.
            cur = state["current"]
            if cur is None or cur["champion"] != champ_gen:
                state["attempt_no"] += 1
                cur = state["current"] = {
                    "attempt": state["attempt_no"], "champion": champ_gen,
                    "candidate": candidate,
                    "corpus": str(data_dir / f"gen{candidate}_a{state['attempt_no']}.jsonl"),
                    "corpus_done": False,
                    "prefix": str(runs_dir / f"gen{candidate}_a{state['attempt_no']}"),
                    "trained": False,
                }
                save_state(state, state_path)
            attempt = cur["attempt"]
            print(f"\n##### attempt {attempt}: gen{champ_gen} -> gen{candidate} "
                  f"(consecutive fails: {state['fails']}) #####", flush=True)

            # 1. Generate (a partial corpus from an interrupted run is regenerated).
            if not cur["corpus_done"]:
                run_phase("generate", [
                    py, "-m", "kdagent.selfplay", "--backend", "netbatch", "--overlap",
                    "--ckpt", str(champ_path), "--sims", str(args.sims),
                    "--games", str(args.games), "--concurrent", str(args.concurrent),
                    "--seed", str(args.seed + attempt * 1_000_003),
                    "--out", cur["corpus"], "--device", args.device])
                cur["corpus_done"] = True
                state["corpora"].append(cur["corpus"])
                save_state(state, state_path)

            # 2. Train on the most recent 2 corpora (fresh net, per-epoch checkpoints).
            if not cur["trained"]:
                window = state["corpora"][-2:]
                run_phase("train", [
                    py, "-m", "kdagent.train", "--corpus", *window,
                    "--epochs", str(args.epochs), "--batch-size", str(args.batch_size),
                    "--seed", str(args.seed + attempt), "--device", args.device,
                    "--out", cur["prefix"]])
                cur["trained"] = True
                save_state(state, state_path)

            # 3. Evaluate every epoch vs the champion (rerun in full if interrupted).
            sweep_json = f"{cur['prefix']}.sweep.json"
            run_phase("evaluate", [
                py, "-m", "kdagent.epoch_sweep", "--prefix", cur["prefix"],
                "--opponent", f"netmcts:{eval_sims}:{champ_path}",
                "--sims", str(eval_sims), "--games", str(args.eval_games),
                "--concurrent", str(args.concurrent), "--seed", str(args.seed + attempt),
                "--device", args.device, "--json-out", sweep_json])
            with open(sweep_json, encoding="utf-8") as f:
                best = json.load(f)["best"]

            # 4. Promotion gate: win-rate lower confidence bound must clear 50%.
            promoted = best["mean"] - best["ci"] > 0.5
            state["history"].append({
                "attempt": attempt, "candidate": candidate, "games": args.games,
                "sims": args.sims, "eval_games": best["n"], "eval_sims": eval_sims,
                "best_epoch": best["epoch"], "mean": best["mean"], "ci": best["ci"],
                "promoted": promoted, "corpus": cur["corpus"],
            })
            if promoted:
                src = f"{cur['prefix']}.epoch{best['epoch']}.pt"
                dst = runs_dir / f"gen{candidate}.best.pt"
                shutil.copyfile(src, dst)
                state["fails"] = 0
                promotions_this_run += 1
                print(f"\nPROMOTED: {dst} (epoch {best['epoch']}, "
                      f"{best['mean'] * 100:.1f}% +/- {best['ci'] * 100:.1f} vs gen{champ_gen})",
                      flush=True)
            else:
                state["fails"] += 1
                print(f"\nNOT PROMOTED: best epoch {best['epoch']} at "
                      f"{best['mean'] * 100:.1f}% +/- {best['ci'] * 100:.1f} "
                      f"(needs lower bound > 50%) — fail {state['fails']}/{args.max_fails}",
                      flush=True)
            state["current"] = None
            save_state(state, state_path)

            if args.gens is not None and promotions_this_run >= args.gens:
                stop_reason = f"reached --gens {args.gens}"
                break
            if state["fails"] >= args.max_fails:
                stop_reason = f"{state['fails']} consecutive promotion failures"
                break
    except KeyboardInterrupt:
        save_state(state, state_path)
        print("\n\ninterrupted — current standings:")
        print(summary_text(state, runs_dir))
        notify(args.notify, "Interrupted.\n" + summary_text(state, runs_dir))
        sys.exit(130)
    except Exception as e:
        save_state(state, state_path)
        notify(args.notify, f"Crashed: {e}\n" + summary_text(state, runs_dir))
        raise

    print(f"\ndone: {stop_reason}")
    print(summary_text(state, runs_dir))
    notify(args.notify, f"Done: {stop_reason}.\n" + summary_text(state, runs_dir))


if __name__ == "__main__":
    main()
