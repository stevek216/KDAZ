"""Replay a recorded BGA advisor decision, optionally at a different blend/sims.

The server logs every snapshot it was asked about plus the advice it gave, so any
decision from a live table can be re-staged exactly as the advisor saw it.

    python replay_bga.py runs/bga/891290510.jsonl            # last decision, as advised
    python replay_bga.py runs/bga/891290510.jsonl -n -3      # third from last
    python replay_bga.py runs/bga/891290510.jsonl --blend 0 0.75 --sims 800
"""
from __future__ import annotations

import argparse
import json


def load(path: str, n: int) -> dict:
    rows = [json.loads(l) for l in open(path, encoding="utf-8") if l.strip()]
    return rows[n]


def show(rec: dict, tag: str) -> None:
    print(f"\n=== {tag} ===")
    print(f"phase {rec['phase']} | to_act {rec['to_act']} | you {rec.get('you')} | "
          f"round {rec.get('round')} | deck left {rec.get('deck_remaining')} | "
          f"{rec.get('n_legal')} legal | sims {rec.get('sims')}")
    print(f"score on table: {rec.get('bga_scores')}  {rec.get('names')}")
    print(f"root value: {rec.get('value')}")
    for i, r in enumerate(rec["recommendations"]):
        star = " <-- picked" if i == 0 else ""
        print(f"  {i}: p={r['prob']:.3f} visits={r['visits']:>4} q={r['q']:+.4f} "
              f"prior={r['prior']:.3f} | {r['desc']}{star}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("log", help="a runs/bga/<table>.jsonl recording")
    ap.add_argument("-n", type=int, default=-1, help="which record (default -1 = last)")
    ap.add_argument("--checkpoint", default="gen11.best")
    ap.add_argument("--sims", type=int, default=None, help="override the recorded sim count")
    ap.add_argument("--blend", type=float, nargs="*", default=None,
                    help="re-run at these score-head blends, e.g. --blend 0 0.75")
    ap.add_argument("--device", default="cpu")
    args = ap.parse_args()

    row = load(args.log, args.n)
    show(row["reply"], f"as advised at the table  ({args.log} record {args.n})")
    if args.blend is None:
        return

    from kdagent.advisor import Advisor, recommend

    sims = args.sims or row["reply"].get("sims", 256)
    for a in args.blend:
        adv = Advisor(args.checkpoint, sims=sims, device=args.device, value_blend=a)
        show(recommend(row["snapshot"], adv), f"re-run: blend {a}, {sims} sims")


if __name__ == "__main__":
    main()
