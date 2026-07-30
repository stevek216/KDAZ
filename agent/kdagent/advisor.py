"""The advisor's brain: an observed BGA position in, ranked advice out.

`kdagent.bga` turns a snapshot into an engine game; this module runs the search on it and
shapes the answer for the side panel. Kept separate from `kdagent.server` so the whole
pipeline is testable without a socket, and so the play-UI server stays a thin router.

Read-only by construction: nothing here can act on a table, and the extension that feeds it
never clicks. Live assistance in rated games is against BGA's rules — this is for private,
friendly and hotseat tables, and for post-game review.
"""
from __future__ import annotations

import json
import math
import os
import time
from pathlib import Path

import numpy as np

import kingdomino as kd
from kdagent import bga

RUNS = Path(__file__).resolve().parents[1] / "runs"
TOPK = 6
# Below this win probability the top move is barely a preference; the panel says so rather
# than presenting search noise as a plan.
CLOSE_BAND = (0.45, 0.55)


def list_checkpoints() -> list[dict]:
    """Every checkpoint the server could load, newest first."""
    out = []
    for p in sorted(RUNS.glob("*.pt"), key=lambda p: p.stat().st_mtime, reverse=True):
        out.append(
            {
                "name": p.stem,
                "path": str(p),
                "bytes": p.stat().st_size,
                "mtime": int(p.stat().st_mtime),
            }
        )
    return out


class Advisor:
    """Holds the loaded network and the search budget; scores positions on demand."""

    def __init__(self, checkpoint: str | None = None, sims: int = 256, device: str = "cpu",
                 value_blend: float = 0.75):
        self.sims = int(sims)
        self.device = device
        # Blend the score head into the search's leaf value: 52.3% +/- 1.8 over 3000 games on
        # gen11 @512 sims, and free (the head is already trained). The advisor builds its own
        # NetEvaluator rather than going through `arena.make_agent`, so this has to be threaded
        # explicitly — otherwise the advisor silently advises from a weaker evaluator than the
        # one every measurement was taken on.
        self.value_blend = float(value_blend)
        self.checkpoint = None
        self._ev = None
        self._rollout = None
        if checkpoint:
            self.load(checkpoint)

    # -- model ------------------------------------------------------------------------
    def load(self, checkpoint: str) -> str:
        """Load a checkpoint by path or bare name (`gen10.best` → `runs/gen10.best.pt`)."""
        path = Path(checkpoint)
        if not path.exists():
            cand = RUNS / f"{checkpoint}.pt" if not checkpoint.endswith(".pt") else RUNS / checkpoint
            if not cand.exists():
                raise FileNotFoundError(f"no checkpoint {checkpoint!r} (looked in {RUNS})")
            path = cand
        from kdagent.mcts.evaluators import NetEvaluator
        from kdagent.net import load_net

        net, _ = load_net(str(path), self.device)
        self._ev = NetEvaluator(net, device=self.device, value_blend=self.value_blend)
        self.checkpoint = str(path)
        return self.checkpoint

    def evaluator(self):
        """The net when one is loaded, else a rollout evaluator so the advisor still works
        (weakly) with no checkpoint — better than a blank panel while a run is training."""
        if self._ev is not None:
            return self._ev
        if self._rollout is None:
            from kdagent.mcts.evaluators import RolloutEvaluator

            self._rollout = RolloutEvaluator(seed=0)
        return self._rollout

    @property
    def model_name(self) -> str:
        return Path(self.checkpoint).stem if self.checkpoint else "rollout (no checkpoint)"

    # -- scoring ----------------------------------------------------------------------
    def score(self, game, meta: dict) -> dict:
        """Run the search on `game` and build the panel's payload."""
        obs = json.loads(game.observation())
        previews = json.loads(game.action_previews())
        seat = obs["to_act"]
        started = time.time()

        if game.num_actions() == 0:
            return {"error": "no legal actions at this position"}

        if self.sims > 0 and game.num_actions() > 1:
            from kdagent.mcts.search import MCTS

            search = MCTS(self.evaluator(), n_sims=self.sims, c_puct=1.5, seed=0)
            policy, root_value, root = search.run(game, add_noise=False)
            # Search values live on [-1,1]; the panel wants win probabilities.
            value = [(float(v) + 1.0) / 2.0 for v in root_value]
            recs = [
                {
                    "index": i,
                    "prob": float(policy[i]),
                    "visits": int(root.N[i]),
                    "q": (float(root.W[i][seat] / root.N[i]) + 1.0) / 2.0
                    if root.N[i] > 0
                    else None,
                    "prior": float(root.priors[i]),
                }
                for i in range(len(policy))
            ]
            # Rank what we DISPLAY by a lower confidence bound on Q rather than raw visits.
            # At a saturated position (everything winning) visits track the prior, not the
            # payoff, and the panel would present exploration noise as a preference.
            recs.sort(key=_lcb, reverse=True)
            recs = recs[:TOPK]
            pv = _principal_variation(root, game)
            top_prior = int(np.argmax(root.priors))
            net_top = {
                "index": top_prior,
                "prior": float(root.priors[top_prior]),
                "desc": bga.describe(previews[top_prior], obs),
            }
        else:
            priors, absval = self.evaluator().evaluate(game)
            value = [float(v) for v in absval]
            order = list(np.argsort(-np.asarray(priors))[:TOPK])
            recs = [
                {
                    "index": int(i),
                    "prob": float(priors[i]),
                    "visits": 0,
                    "q": None,
                    "prior": float(priors[i]),
                }
                for i in order
            ]
            pv = []
            net_top = {
                "index": recs[0]["index"],
                "prior": recs[0]["prior"],
                "desc": bga.describe(previews[recs[0]["index"]], obs),
            }

        you = meta.get("you")
        for r in recs:
            a = previews[r["index"]]
            r["desc"] = bga.describe(a, obs)
            r["hl"] = bga.highlight(a, obs)
            r["action"] = a
            if r["hl"]:
                # Whether the move belongs to the seat whose kingdom this client renders as
                # its own board. BGA draws your kingdom and everyone else's in different
                # containers at different scales, so the extension only paints placements on
                # the page when they are yours — the panel's own board covers the rest.
                r["hl"]["own"] = you is not None and you == seat
            if a["type"] == "place":
                r["score_delta"] = a["score_delta"]
                r["score_total"] = a["score_total"]

        return {
            "to_act": seat,
            "you": you,
            "names": meta["names"],
            "colors": meta["colors"],
            "your_turn": you is not None and you == seat,
            "phase": obs["phase"],
            "round": obs["round"],
            "deck_remaining": obs["deck_remaining"],
            "current_domino": obs["current_domino"],
            "sims": self.sims if game.num_actions() > 1 else 0,
            "model": self.model_name,
            "value": value,
            "close": CLOSE_BAND[0] <= value[seat] <= CLOSE_BAND[1],
            "n_legal": game.num_actions(),
            "recommendations": recs,
            "net_top": net_top,
            "pv": pv,
            "board": bga.board_view(obs),
            "lines": bga.line_view(obs),
            "bga_scores": meta["bga_scores"],
            "elapsed_ms": int(1000 * (time.time() - started)),
        }


def _lcb(rec: dict) -> float:
    """KataGo-style lower confidence bound on a move's value, for display ranking."""
    if rec["visits"] and rec["q"] is not None:
        q = rec["q"]
        se = math.sqrt(max(q * (1.0 - q), 1e-9) / rec["visits"])
        return q - 1.96 * se
    return -1.0


def _principal_variation(root, game, depth: int = 8) -> list[str]:
    """The line the search actually believes in: follow the most-visited child, stepping over
    chance nodes (a draw the search sampled, not a decision anyone makes)."""
    out: list[str] = []
    node = root
    for _ in range(depth):
        if node.terminal:
            break
        if node.chance:
            # Chance children are keyed by outcome index; take the most-explored one and say
            # so, since the plan below it is conditional on that draw.
            if not node.children:
                break
            idx, child = max(node.children.items(), key=lambda kv: _visit_total(kv[1]))
            outcomes = {o["index"]: o for o in json.loads(node.game.chance_outcomes())}
            o = outcomes.get(idx, {})
            out.append(f"(draw #{o.get('number', '?')})")
            node = child
            continue
        if node.N is None or node.N.sum() == 0:
            break
        best = int(np.argmax(node.N))
        try:
            obs = json.loads(node.game.observation())
            previews = json.loads(node.game.action_previews())
            name = obs["to_act"]
            out.append(f"seat {name + 1}: {bga.describe(previews[best], obs)}")
        except (ValueError, IndexError, KeyError):
            break
        child = node.children.get(best)
        if child is None:
            break
        node = child
    return out


def _visit_total(node) -> int:
    return int(node.N.sum()) if getattr(node, "N", None) is not None else 0


def recommend(snapshot: dict, advisor: Advisor) -> dict:
    """The whole pipeline: snapshot → position → search → advice.

    Never raises for a table we cannot read. A silent panel is the worst outcome — it looks
    identical to "no decision pending" — so every refusal comes back as a labelled field the
    panel renders loudly.
    """
    out: dict = {"table": str(snapshot.get("table") or ""), "at": time.time()}
    try:
        game, meta = bga.build_game(snapshot)
    except bga.Unsupported as e:
        return {**out, "unsupported": str(e)}
    except bga.CaptureError as e:
        return {**out, "capture_error": str(e)}

    out.update(advisor.score(game, meta))
    out["engine_position"] = meta["position"]
    # BGA commits a staged placement when the player picks their next tile, so the claim is
    # being advised on a board that includes a placement not yet sent to the server. Say so:
    # the advice is conditional on it, and a player who changes their mind must re-read.
    if snapshot.get("staged_placement"):
        out["staged_placement"] = snapshot["staged_placement"]
    # BGA ships its own legal-placement list with scores; disagreement means the advice may
    # be off-menu or mis-valued, and the panel must say so.
    try:
        out["legality_check"] = bga.check_previews(json.loads(game.action_previews()), snapshot)
    except (ValueError, KeyError, TypeError) as e:
        out["legality_check"] = {"error": str(e)}
    # A cheap, always-on cross-check of our scoring against BGA's own scoreboard. BGA scores
    # the crowns as it goes and only adds the variant bonuses at final scoring, so the
    # comparable number is the crown component, not the projected total.
    crowns = [b["crown_score"] for b in out.get("board", [])]
    if any(meta["bga_scores"]) and crowns != meta["bga_scores"]:
        out["score_check"] = {"engine": crowns, "bga": meta["bga_scores"]}
    return out


def record(snapshot: dict, reply: dict, record_dir: str | os.PathLike | None) -> str | None:
    """Append the snapshot + advice to a per-table log. Recording is always on when a
    directory is configured: the interesting positions are never the ones you thought to
    save, and a live table cannot be paused to investigate.

    Failures are reported, never raised — losing the log is a nuisance, losing the advice
    mid-turn is not. Returns an error string when the write failed.
    """
    if not record_dir:
        return None
    try:
        table = str(snapshot.get("table") or "unknown")
        d = Path(record_dir)
        d.mkdir(parents=True, exist_ok=True)
        line = json.dumps({"t": time.time(), "snapshot": snapshot, "reply": reply})
        with (d / f"{table}.jsonl").open("a", encoding="utf-8") as f:
            f.write(line + "\n")
        return None
    except OSError as e:
        return f"recording failed: {e}"
