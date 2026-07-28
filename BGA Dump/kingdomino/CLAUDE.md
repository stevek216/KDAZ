# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Board Game Arena (BGA) Studio implementation of **Kingdomino**. There is no local
build, test, or package tooling: code runs only inside the BGA Studio server, which
loads these files by fixed filename convention. "Running" the game means deploying to
BGA Studio and starting a table there. `version.php` (`999999-9999`) is the dev sentinel
and should not be edited; `bga-framework.d.ts` and `_ide_helper.php` are
editor-only stubs for the framework API (never shipped, never executed).

## Architecture

Two sides talk over a fixed BGA contract. The PHP backend owns all rules and is the only
source of truth; the JS frontend is a renderer driven entirely by notifications.

### Backend (PHP, authoritative)
- [kingdomino.game.php](kingdomino.game.php) - all game logic. Mutates the DB, then
  calls `notifyAllPlayers(...)` and `gamestate->nextState(...)`. Player-action methods
  (`chooseDomino`, `placeDomino`, `discardDomino`) must each have a matching thin wrapper
  in [kingdomino.action.php](kingdomino.action.php) that unpacks AJAX args. `getAllDatas()`
  is the full-state snapshot sent on load/refresh - keep it in sync with what the JS
  `setup()` expects.
- [states.inc.php](states.inc.php) - the state machine. Editing it while a game is live
  breaks that game. Flow: `chooseDomino` (2) -> `nextPlayer` (3, game-type router) ->
  `placeDomino` (4) and back. `nextPlayer` routes via `activateNextPlayer()`.
- [material.inc.php](material.inc.php) - the 48 dominoes (`$this->dominoes`, indexed by
  number = strength order) and the 6 `$this->terrains`. Loaded into the game class at
  construction; available as `$this->dominoes` everywhere.
- [dbmodel.sql](dbmodel.sql) - schema. The `dominoes` table is the entire game state:
  each domino has a `location` (DRAW_PILE / FUTURE / CURRENT / KINGDOM / DISCARD),
  `owner_player`, and placement (`rotation`, `horizontal_position`, `vertical_position`).
  The `token` table is unused. Changing this file only takes effect on a fresh game.
- [kingdomino.view.php](kingdomino.view.php) + [kingdomino_kingdomino.tpl](kingdomino_kingdomino.tpl)
  - server-rendered HTML skeleton and `{VARIABLE}` / `BEGIN/END` block substitution.

### Frontend (JS, dumb renderer)
- [kingdomino.js](kingdomino.js) - one `declare("bgagame.kingdomino", ...)` Dojo class.
  `setup()` builds the board from `getAllDatas()`; `onEnteringState`/`onLeavingState`
  drive per-state UI; `setupNotifications()` wires each backend notification to a
  `notif_*` handler. Never compute or trust scores client-side - `previewScore.html`
  asks the backend.
- [kingdomino.css](kingdomino.css) - CSS-sprite rendering. Terrain/crown art comes from
  `img/tiles*.jpg` via `background-position`; do not add per-tile image files.

## Key mechanics to know before editing logic

- **Coordinates**: the castle is `(0,0)`. A domino occupies its `(x,y)` (the "left"
  half) plus a neighbor derived from `rotation` (0=+x, 1=-y, 2=-x, 3=+y) via
  `getRightTerrainCoordinates`. Kingdom validation lives in `dominoFitsInPosition`
  (occupied + connects + fits-in-grid). `positionFitsInGrid` enforces the 5x5 bounding
  box (7x7 under The Mighty Duel).
- **Scoring**: territories are connected same-terrain regions found by a union-find sweep
  in `getKingdomTerritories` / `mergeTerritories`; a territory scores `size * crowns`.
  Final scoring (`finalScoring`) also applies the variant bonuses and sets
  `player_score_aux` = `biggestTerritory*100 + totalCrowns` as the tiebreaker.

## Variants and options (must stay consistent across files)

A new option/variant touches several files at once; treat them as a set:
- IDs and labels: [gameoptions.json](gameoptions.json) (101 The middle Kingdom, 102
  Harmony, 103 The Mighty Duel) and the state labels in `kingdomino.game.php`
  `initGameStateLabels` (`dynasty`, `the_middle_kingdom`, `harmony`, `the_mighty_duel`).
- The Mighty Duel switches the grid to 7x7 - check every `isMightyDuel()` / `gridSize`
  branch (setup deals all 48 dominoes; placement and view use size 7).
- **3-player rule migration**: `3_players_v2` is a per-game flag distinguishing the old
  3-domino-per-round rule from the new 4-with-discard rule. Old games keep value 0; new
  games use 1. Guard 3-player branches with it.
- New stats go in [stats.json](stats.json) AND `initStat(...)` in `setupNewGame`.
- **Edition** ([gamepreferences.json](gamepreferences.json) id 201, 2025 vs 2016) is a
  client-only display preference: JS sets `html[data-edition]` and CSS swaps
  `img/tiles-2025.jpg` vs `img/tiles.jpg`. It never affects game logic.

## Conventions

- All user-facing strings use `clienttranslate(...)` (PHP) / `_(...)` (JS).
- Backend changes that affect the UI must emit a notification; the JS only reacts to
  notifications and `getAllDatas()`, never polls.
- `img/` must stay flat (no subdirectories) per BGA packaging rules; `misc/` and
  `modules/` are for non-deployed notes and optional PHP includes respectively.
- **New JS should be vanilla, not Dojo.** Prefer plain DOM/JS APIs
  (`element.classList`, `addEventListener`, `element.remove()`, `querySelectorAll`,
  etc.) over `dojo.*` in new or edited code. The one allowed exception is `$(id)`
  (the BGA shortcut for `document.getElementById`). Existing Dojo code can stay; do
  not rewrite it wholesale unless asked.
- **New JS: use `const`/`let`, never `var`.** Prefer `const` for locals, `let` when
  reassigned. Optional chaining is fine (e.g. `$('id')?.remove()`). Existing `var`
  code can stay unless you are editing it.
- **New JS should use the modern framework API, not the deprecated globals.** The
  current methods live under the `this.bga.*` namespace; the old top-level methods
  are marked `@deprecated` in [bga-framework.d.ts](bga-framework.d.ts) and each
  annotation names its replacement. Common swaps: `addActionButton` ->
  `this.bga.statusBar.addActionButton`, page title -> `this.bga.statusBar.setTitle`,
  `ajaxcall`/`ajaxAction` -> `this.bga.actions.performAction`, `checkAction` ->
  `this.bga.actions.checkAction`, `showMessage` -> `this.bga.dialogs.showMessage`,
  `isCurrentPlayerActive` -> `this.bga.players.isCurrentPlayerActive`. Check the
  d.ts for the exact replacement before using an old method. Existing deprecated
  calls can stay unless you are asked to migrate them.
