"""Local web server to play Kingdomino against the agent.

A dependency-free (Python stdlib) HTTP server that wraps the `kingdomino` engine bridge and
the arena agents, and serves the static front-end in `web/app/`. The engine stays the single
source of rules truth — the server only drives the game (auto-resolving the hidden draws and
playing the opponent's turns) and exposes the public state + legal moves over a JSON API.

    cd agent
    .venv/Scripts/python -m kdagent.server                 # opponent = rollout MCTS
    .venv/Scripts/python -m kdagent.server --opponent netmcts:128:runs/gen0.best.pt --device cuda

Then open http://127.0.0.1:8000 in a browser.
"""
from __future__ import annotations

import argparse
import json
import random
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import kingdomino as kd
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


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):  # quiet
        pass

    def _json(self, obj, code=200):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

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
        if self.path == "/api/state":
            if not SESSION:
                self._json(new_game({}))
            else:
                self._json(state_dict(SESSION, []))
            return
        self._static(self.path.split("?", 1)[0])

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        try:
            if self.path == "/api/new":
                self._json(new_game(body))
            elif self.path == "/api/move":
                self._json(apply_move(int(body["index"])))
            else:
                self.send_error(404)
        except (ValueError, KeyError) as e:
            self._json({"error": str(e)}, code=400)


def main():
    ap = argparse.ArgumentParser(description="Play Kingdomino against the agent in a browser.")
    ap.add_argument("--opponent", default="mcts:128",
                    help="agent spec: mcts:SIMS | net:CKPT | netmcts:SIMS:CKPT")
    ap.add_argument("--device", default="cpu", help="torch device for net opponents (e.g. cuda)")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8000)
    args = ap.parse_args()
    DEFAULTS.update({"opponent": args.opponent, "device": args.device})

    srv = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"Kingdomino — opponent {args.opponent} (device {args.device})")
    print(f"  serving {WEB_ROOT}")
    print(f"  open http://{args.host}:{args.port}  (Ctrl+C to stop)", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\nbye")


if __name__ == "__main__":
    main()
