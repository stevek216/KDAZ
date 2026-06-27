# Kingdomino UI

A browser UI to play Kingdomino (Mighty Duel) against the agent. The engine is the single
source of rules truth — the UI asks the server (which drives the `kingdomino` bridge) for
legal moves, placements, and scores, and never reimplements a rule (CLAUDE §3).

## Architecture

- **Backend** — `agent/kdagent/server.py`: a dependency-free Python stdlib HTTP server that
  wraps the engine bridge + the arena agents. It drives the game (auto-resolves the hidden
  draws and plays the opponent's turns) and exposes the public state + legal moves over a
  small JSON API, plus serves this static front-end.
- **Front-end** — `web/app/`: vanilla HTML/CSS/JS. Renders both kingdoms and the draft using
  the BoardGameArena tile sprites (`assets/tiles-2025.webp`, single-square crops via the
  `(terrain, crowns) → domino-half` map), and handles the human's claim + place interactions
  (ghost preview, rotation, legal-cell highlighting).

## Run it

From `agent/` (with the venv + `maturin develop` done — see `agent/README.md`):

```powershell
# Rollout-MCTS opponent (no trained net needed):
.\.venv\Scripts\python.exe -m kdagent.server --opponent mcts:128

# A trained network opponent (net-guided MCTS), on the GPU:
.\.venv\Scripts\python.exe -m kdagent.server --opponent netmcts:128:runs\gen0.best.pt --device cuda
```

Then open <http://127.0.0.1:8000>. Pick your castle, toggle Harmony / Middle Kingdom, and
press **New game**. Claim a highlighted tile; on your placement turn, hover a green cell
(press **R** to rotate) and click to place.

## Note on the tile art

`assets/tiles-2025.webp` is BoardGameArena's tile artwork, used here for authentic local
play. It is copyrighted — fine for personal use, but swap in original art before hosting or
distributing the UI.
