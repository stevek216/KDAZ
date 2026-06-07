# Kingdomino UI — to come

A browser UI to play against the trained agent and watch games, mirroring the Space Base
web app (`../../SpaceBase/web/`): a Rust→WASM binding over the engine plus a static
front-end. The UI **never reimplements a rule** — it asks the engine for legal actions,
placements, and scores (CLAUDE §3).

## Status

Not started. Built after the engine and a first trained agent exist.
