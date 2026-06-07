# BGA-sourced reference data

Authoritative Kingdomino data captured from the **BoardGameArena** client, the way the
Space Base project sourced its cards from BGA. Treat these files as the source of truth;
the Rust tables in `src/components/` are transcribed from them and guarded by tally tests.

## `kingdomino_dominoes_bga.json`

All 48 base-game dominoes, captured 2026-06-07 from `gameui.gamedatas.dominoesDescription`
on a live Mighty Duel table (`gridSize: 7`, Middle Kingdom + Harmony enabled).

- **Schema:** `dominoes[number] = { left: {terrain, crowns}, right: {terrain, crowns} }`.
  `number` (1..48) is the draft-order rank on the tile's number side; `left`/`right` are the
  two terrain squares as BGA orients them. `crowns ∈ {0,1,2,3}`.
- **Terrain names are BGA's:** `field` (= rulebook "wheat field"), `forest`, `lake`,
  `grassland`, `swamp`, `mountain` (= the **mine** terrain, rulebook "mines"). The engine's
  `Terrain` enum maps `mountain → Mine`.
- **Verified tallies** (asserted by `components::domino` tests):
  - squares per terrain: field 26, forest 22, lake 18, grassland 14, swamp 10, mountain 6
    (= 96 = 48×2)
  - crowns per terrain: field 5, forest 6, lake 6, grassland 6, swamp 6, mountain 10 (= 39)

## Art assets (for the eventual UI — not needed by the engine)

The BGA client renders all tiles from a single sprite sheet:

- tiles: `https://x.boardgamearena.net/data/themereleases/current/games/kingdomino/<release>/img/tiles-2025.jpg`
- box art: `https://x.boardgamearena.net/data/gamemedia/kingdomino/box/en_280.png`

(The `<release>` path segment changes when BGA redeploys; re-capture from the live page if
the URL 404s. Pull these when building `web/`, not before.)
