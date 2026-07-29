"""Local web server: play Kingdomino against the agent, and advise a live BGA table.

A dependency-free (Python stdlib) HTTP server that wraps the `kingdomino` engine bridge and
the arena agents, and serves the static front-end in `web/app/`. The engine stays the single
source of rules truth — the server only drives the game (auto-resolving the hidden draws and
playing the opponent's turns) and exposes the public state + legal moves over a JSON API.

    cd agent
    .venv/Scripts/python -m kdagent.server                 # opponent = rollout MCTS
    .venv/Scripts/python -m kdagent.server --opponent netmcts:128:runs/gen0.best.pt --device cuda

Then open http://127.0.0.1:8000 in a browser.

The same process also backs the **BGA advisor** (`advisor/DESIGN.md`): the Chrome extension
POSTs snapshots of a live table to `/recommend_bga` and renders the reply in its side panel.

    .venv/Scripts/python -m kdagent.server --checkpoint gen10.best --sims 400 --device cuda

Advisor routes: `POST /recommend_bga`, `GET /latest`, `GET|POST /config`, `GET /models`,
`GET /latest_snapshot`, `POST /debug_dump`. All read-only with respect to the table — the
server can no more play a move on BGA than the extension can.
"""
from __future__ import annotations

import argparse
import gzip
import json
import random
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import kingdomino as kd
from kdagent import advisor as advisor_mod
from kdagent.arena import make_agent

WEB_ROOT = Path(__file__).resolve().parents[2] / "web" / "app"
CONTENT_TYPES = {
    ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8",
    ".js": "application/javascript; charset=utf-8", ".webp": "image/webp",
    ".png": "image/png", ".jpg": "image/jpeg", ".svg": "image/svg+xml",
    ".ico": "image/x-icon", ".json": "application/json",
}

# A single local session (one game at a time).
SESSION: dict = {}
DEFAULTS: dict = {}  # CLI defaults for a new game

# ---- BGA advisor state (one live table at a time, like the extension's side panel) ----
# Constructed eagerly because it is cheap — no network is loaded until one is asked for, so
# importing this module never drags in torch. `main` replaces it with the CLI's settings.
ADVISOR = advisor_mod.Advisor(None, sims=256)
ADVISOR_LOCK = threading.Lock()  # the search is not reentrant; snapshots can overlap
LATEST: dict = {}  # last reply, so the panel can re-read it after a reload
LATEST_SNAPSHOT: dict = {}  # last snapshot, for re-scoring and for debugging
RECORD_DIR: Path | None = None


def describe(action: dict, obs: dict, seat: int) -> dict:
    """A structured + human-readable record of an opponent action (for the UI log/animation)."""
    t = action["type"]
    if t == "claim":
        line = obs["current_line"] if obs["phase"] == "start_claim" else obs["next_line"]
        num = line[action["slot"]]["number"]
        return {"type": "claim", "seat": seat, "slot": action["slot"], "number": num,
                "text": f"Opponent claimed domino {num}"}
    if t == "place":
        cd = obs["current_domino"]
        return {"type": "place", "seat": seat, "row": action["row"], "col": action["col"],
                "rot": action["rot"], "a": cd["a"], "b": cd["b"], "number": cd["number"],
                "text": f"Opponent placed domino {cd['number']}"}
    cd = obs.get("current_domino")
    num = cd["number"] if cd else "?"
    return {"type": "discard", "seat": seat, "number": num,
            "text": f"Opponent discarded domino {num} (no legal placement)"}


def advance(s: dict) -> list[dict]:
    """Drive the game until it's the human's turn or it's over: sample the hidden draws and
    play every opponent decision. Returns the opponent/draw events since the human last acted."""
    g, human, agent = s["game"], s["human_seat"], s["agent"]
    events: list[dict] = []
    guard = 0
    while True:
        guard += 1
        if guard > 50000:
            break
        if g.is_terminal():
            break
        if g.is_chance():
            g.apply_chance()
            continue
        if g.to_act() == human:
            break
        seat = g.to_act()
        obs = json.loads(g.observation())
        legal = json.loads(g.legal_actions())
        idx = 0 if len(legal) == 1 else agent.act(g, s["move_no"])
        s["move_no"] += 1
        events.append(describe(legal[idx], obs, seat))
        g.apply(idx)
    return events


def state_dict(s: dict, events: list[dict]) -> dict:
    g = s["game"]
    obs = json.loads(g.observation())
    terminal = g.is_terminal()
    obs.update({
        "legal": [] if terminal else json.loads(g.legal_actions()),
        "human_seat": s["human_seat"],
        "opponent": s["opponent_label"],
        "terminal": terminal,
        "terminal_value": g.terminal_value() if terminal else None,
        "events": events,
    })
    return obs


def new_game(params: dict) -> dict:
    seed = int(params.get("seed", random.randrange(1 << 31)))
    human_seat = int(params.get("human_seat", 0))
    harmony = bool(params.get("harmony", True))
    middle = bool(params.get("middle_kingdom", True))
    opponent = params.get("opponent") or DEFAULTS["opponent"]
    device = params.get("device") or DEFAULTS["device"]
    # opponent fills the non-human seat; make_agent matches the arena specs.
    agent = make_agent(opponent, seed=1, device=device)
    SESSION.clear()
    SESSION.update({
        "game": kd.Game(seed, 2, harmony, middle),
        "human_seat": human_seat,
        "agent": agent,
        "opponent_label": agent.name,
        "move_no": 1,
        "seed": seed,
    })
    return state_dict(SESSION, advance(SESSION))


def apply_move(index: int) -> dict:
    s = SESSION
    g = s["game"]
    if g.is_terminal() or g.is_chance() or g.to_act() != s["human_seat"]:
        raise ValueError("not your turn")
    if not (0 <= index < g.num_actions()):
        raise ValueError(f"illegal action index {index}")
    g.apply(index)
    return state_dict(s, advance(s))


# --------------------------------------------------------------------------- BGA advisor
def recommend_bga(snapshot: dict) -> dict:
    """Score a live BGA position. Serialised: a burst of snapshots during an animation must
    not run two searches at once on one GPU."""
    global LATEST, LATEST_SNAPSHOT
    with ADVISOR_LOCK:
        reply = advisor_mod.recommend(snapshot, ADVISOR)
        LATEST, LATEST_SNAPSHOT = reply, snapshot
    failed = advisor_mod.record(snapshot, reply, RECORD_DIR)
    if failed:
        reply["record_error"] = failed
    return reply


def rescore() -> dict:
    """Re-run the search on the position already on screen — what the sims selector needs so
    a new budget takes effect immediately instead of at the next decision."""
    if not LATEST_SNAPSHOT:
        return {}
    return recommend_bga(LATEST_SNAPSHOT)


def advisor_config(patch: dict | None = None) -> dict:
    """Read or change the live search budget / checkpoint. A change re-scores the current
    position so the panel updates without waiting for the next decision."""
    changed = False
    error = None
    if patch:
        with ADVISOR_LOCK:
            if "sims" in patch:
                ADVISOR.sims = max(0, int(patch["sims"]))
                changed = True
            if patch.get("checkpoint"):
                try:
                    ADVISOR.load(str(patch["checkpoint"]))
                    changed = True
                except (FileNotFoundError, OSError, RuntimeError, KeyError) as e:
                    error = str(e)
    if changed:
        rescore()
    return {
        "sims": ADVISOR.sims,
        "checkpoint": ADVISOR.checkpoint,
        "model": ADVISOR.model_name,
        "device": ADVISOR.device,
        "recording": str(RECORD_DIR) if RECORD_DIR else None,
        **({"error": error} if error else {}),
    }


def save_debug_dump(payload: dict) -> dict:
    """Persist a one-click black box from the extension. Real opponents mean a turn cannot be
    held open to investigate; dump now, debug after the game."""
    d = (RECORD_DIR or Path("runs/bga")) / "dumps"
    d.mkdir(parents=True, exist_ok=True)
    path = d / f"dump-{int(time.time())}.json.gz"
    blob = json.dumps({"payload": payload, "latest": LATEST, "snapshot": LATEST_SNAPSHOT})
    with gzip.open(path, "wt", encoding="utf-8") as f:
        f.write(blob)
    return {"path": str(path), "bytes": path.stat().st_size}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):  # quiet
        pass

    def _cors(self):
        # The extension's service worker fetches from a chrome-extension:// origin.
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")

    def _json(self, obj, code=200):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self._cors()
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.end_headers()

    def _static(self, path: str):
        rel = path.lstrip("/") or "index.html"
        fp = (WEB_ROOT / rel).resolve()
        if WEB_ROOT not in fp.parents and fp != WEB_ROOT / rel or not fp.is_file():
            self.send_error(404)
            return
        data = fp.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", CONTENT_TYPES.get(fp.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/api/state":
            if not SESSION:
                self._json(new_game({}))
            else:
                self._json(state_dict(SESSION, []))
            return
        # ---- advisor ----
        if path == "/latest":
            self._json(LATEST)
            return
        if path == "/latest_snapshot":
            self._json(LATEST_SNAPSHOT)
            return
        if path == "/config":
            self._json(advisor_config())
            return
        if path == "/models":
            self._json({"checkpoints": advisor_mod.list_checkpoints(),
                        "current": ADVISOR.checkpoint})
            return
        if path == "/health":
            self._json({"ok": True, "game": "kingdomino", "model": ADVISOR.model_name})
            return
        self._static(path)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        try:
            if self.path == "/api/new":
                self._json(new_game(body))
            elif self.path == "/api/move":
                self._json(apply_move(int(body["index"])))
            elif self.path == "/recommend_bga":
                self._json(recommend_bga(body))
            elif self.path == "/config":
                self._json(advisor_config(body))
            elif self.path == "/debug_dump":
                self._json(save_debug_dump(body))
            else:
                self.send_error(404)
        except (ValueError, KeyError) as e:
            self._json({"error": str(e)}, code=400)
        except Exception as e:  # noqa: BLE001 — see below
            # Anything unexpected must still come back as JSON. Letting it escape closes the
            # socket with no reply, which the side panel can only report as "server down" —
            # the one failure mode that hides its own cause.
            import traceback

            traceback.print_exc()
            self._json({"error": f"{type(e).__name__}: {e}"}, code=500)


def main():
    global ADVISOR, RECORD_DIR
    ap = argparse.ArgumentParser(
        description="Play Kingdomino against the agent, and advise a live BGA table.")
    ap.add_argument("--opponent", default="mcts:128",
                    help="agent spec: mcts:SIMS | net:CKPT | netmcts:SIMS:CKPT")
    ap.add_argument("--device", default="cpu", help="torch device for net opponents (e.g. cuda)")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8000)
    ap.add_argument("--checkpoint", default=None,
                    help="advisor network: a runs/ name (gen10.best) or a path; "
                         "omitted = rollout search, which is weak but works")
    ap.add_argument("--sims", type=int, default=256,
                    help="advisor search budget per decision (0 = raw policy)")
    ap.add_argument("--record-dir", default="runs/bga",
                    help="where to log BGA snapshots + advice ('' to disable)")
    args = ap.parse_args()
    DEFAULTS.update({"opponent": args.opponent, "device": args.device})
    RECORD_DIR = Path(args.record_dir) if args.record_dir else None
    ADVISOR = advisor_mod.Advisor(args.checkpoint, sims=args.sims, device=args.device)

    srv = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"Kingdomino — opponent {args.opponent} (device {args.device})")
    print(f"  serving {WEB_ROOT}")
    print(f"  advisor: {ADVISOR.model_name}, {ADVISOR.sims} sims"
          + (f", recording to {RECORD_DIR}" if RECORD_DIR else ", not recording"))
    print(f"  open http://{args.host}:{args.port}  (Ctrl+C to stop)", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\nbye")


if __name__ == "__main__":
    main()
