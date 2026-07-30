# BGA advisor — Chrome extension + local brain

A read-only Kingdomino advisor for BoardGameArena: a Chrome (MV3) extension watches a live
table, a local Python server reconstructs the position in the engine, runs the search, and
the extension's side panel shows the answer.

**Lane:** read-only analysis for private / friendly / hotseat tables and post-game review.
The extension never clicks and never plays. Live assistance in *rated* games is against BGA's
rules — that constraint is on the user, not enforceable by the code, and the panel says so.

This mirrors the Space Base advisor (`../../SpaceBase/advisor/`), and the differences are all
in one direction: Kingdomino's data layer is far friendlier, so several things that are still
open problems over there are simply done here (see [Why this one is easier](#why-this-one-is-easier)).

---

## Architecture

```
BGA page (top window + #gameIframe + hotseat iframes)
  └─ page-agent.js   (MAIN world)   reads gameui/gamedatas + the notification bus,
        │                            builds a snapshot, window.postMessage
  └─ content.js      (ISOLATED)     bridge page → service worker; paints the
        │                            recommended move on the board
background.js (service worker)      POST → http://localhost:8000/recommend_bga
        │                            caches the reply, notifies the panel
sidepanel.html/js                   docked UI: advice, win probability, the
                                     reconstructed kingdoms, PV, sims + model pickers
        │
kdagent.server (Python, localhost)  bga.py → engine rebuild → MCTS → advice
```

The four-file split is forced by MV3:

- Content scripts run in an **isolated world** and cannot see `window.gameui`; a second
  content script declared `"world": "MAIN"` can. MAIN-world scripts have no `chrome.*` APIs,
  so the two bridge over `window.postMessage`.
- BGA's CSP blocks page-context fetches to localhost. The **service worker** fetches instead,
  under the extension's `host_permissions` — no CSP or CORS gymnastics.
- The **side panel** is browser UI, not page DOM: BGA cannot break it, restyle it, or garbage-
  collect it, and it survives page reloads.

## Why this one is easier

Space Base's advisor scrapes a presentation layer and reconstructs state from it. Kingdomino
needs none of that, because of two facts about `kingdomino.game.php` / `kingdomino.js`:

1. **`gamedatas.dominoes` is the entire game state.** Every tile carries its location
   (`CURRENT` / `FUTURE` / `KINGDOM` / `DISCARD`), which king owns it, and — for placed tiles
   — where it sits. There is nothing else to know.
2. **The client keeps it live.** Every `notif_*` handler writes locations and owners back into
   `gamedatas`, so a poll of the data layer is current, not a load-time fossil.

So there is **no DOM scraping** in the capture path at all, with one deliberate exception
(below), and the "event-sourced state" that is still Space Base's unfinished Phase C is
essentially free here. Two further consequences:

- **Every seat's position is public**, so the advisor can advise whoever is on the clock, not
  just this client's own turns. Space Base cannot: BGA only sends each client its own choices.
- **The reconstructed deck is exact.** The only hidden information in Kingdomino is the
  *order* of future draws, and the engine models a draw as a chance node over the remaining
  *set* (CLAUDE §3). An observer who knows which tiles have been seen therefore knows the deck
  exactly — no determinization, no guessing.

### The one thing the client does not tell us

`notif_dominoPlaced` writes `location = 'KINGDOM'` back into `gamedatas` but **not** the
placement's coordinates (kingdomino.js). So `page-agent.js` subscribes to the notification bus
(`dojo.subscribe`, additive and invisible to the game) and records `args.position` itself.
Coordinates that predate the extension come from the load-time `getAllDatas`, which does
include them — so an F5 mid-game is fully recoverable.

### The one thing nobody can recover

`getAllDatas` omits the discard pile entirely, and BGA never re-sends it. A tile discarded
*before* the extension started watching is therefore unknowable: it looks like it is still in
the draw pile. Rather than advise from a deck that contains a tile nobody can draw, the engine
**refuses** — `RebuildError::DeckGap` — and the panel says why. Discards are rare in Mighty
Duel (a 7×7 board leaves a lot of room), and the notification stream covers every one that
happens while the extension is loaded.

## The reconstruction, and why it lives in the engine

`kingdomino_engine::core::rebuild` turns an observed position into a `GameState`. It is in the
engine, not the advisor, because working out whose turn it is, which king is acting and what
is left in the deck are **rules answers**, and the UI never reimplements a rule (CLAUDE §3).

The caller supplies only what is directly observable — placed tiles with anchor and rotation,
the two draft lines with their claims, the discards, and which decision is pending. Everything
else is derived:

| derived | from |
| --- | --- |
| `remaining` | `FULL_DECK` minus every tile anyone has seen |
| `round` | how many lines have been drawn: `(48 − |remaining|) / 4` |
| `turn_cursor` | a resolved tile leaves the line, so the acting king is the first slot still holding one |
| `to_act` | that king's owner (or, during the starting round, `claim_order`) |
| `claim_order` | 2p BGA is always the snake `A,B,B,A` (CLAUDE §6), so one observation fixes it |

Every derivation is cross-checked against an independent one — the claims on the next line
must agree with the acting king; the seen tiles must make up whole drawn lines — and a
mismatch is a hard error, never a silently-wrong state. **That refusal is the point.** Advice
computed from a subtly wrong position looks exactly as confident as advice computed from a
right one, so the failure has to be loud.

Coordinates: BGA's castle-relative `(x, y)` (castle `(0,0)`, `+x` right, `+y` **up** the
screen) maps to the engine's backing store as `row = CENTER − y`, `col = CENTER + x`, and
BGA's rotation (`0`=+x, `1`=−y, `2`=−x, `3`=+y) to the engine's `rot` (`0`=up, `1`=right,
`2`=down, `3`=left) as `rot = (rotation + 1) % 4`. The engine's row-major grid therefore
renders exactly as the table looks.

## The live oracle

`argsPlaceDomino` ships **BGA's own list of every legal placement, each with the score it
would produce** — and BGA broadcasts it to every client, not just the active one. On every
placement decision the server compares it against the engine's legal actions and the engine's
own post-move score (`bga.check_previews`).

That is a continuous differential test of our placement *and* scoring rules against the
reference implementation, running on real positions, for free. `tests/bga_parity.rs` does the
same audit offline against transcribed predicates; this one runs live and catches anything the
transcription missed. A disagreement is shown in the panel, because advice from a position
where we and BGA disagree about the rules is not trustworthy.

There is a second, cheaper cross-check on every snapshot: BGA's running player scores against
the engine's crown score (BGA only adds the variant bonuses at final scoring, so the crown
component is the comparable number).

## Supported tables

The engine is built for **2-player "The Mighty Duel"**: all 48 dominoes, a 7×7 kingdom
(CLAUDE §1). The advisor checks the table and refuses anything else by name — base 5×5, 3–4
players, Lost Treasures — rather than guessing. The variant flags are read from the
server-rendered "additional rules" buttons (`kingdomino.view.php` gives each a `visible` /
`hidden` class), which is the only place a client can see them: `getAllDatas` does not carry
the table's options.

## Snapshot dialect

The extension → server contract. The authoritative copy is the `kdagent/bga.py` docstring.

```
{ "table": str,                # BGA table id; groups the server-side recording
  "seat_bga": str,             # the BGA player id this client plays
  "active_bga": str,           # who BGA says is on the clock
  "state": str,                # BGA gamestate name: chooseDomino | placeDomino
  "grid_size": int,            # 7 under The Mighty Duel
  "variants": {harmony, middle_kingdom, mighty_duel, lost_treasures, dynasty},
  "players": [{bga_id, name, score, color}, ...],
  "dominoes": {"<number>": {location, owner, x, y, rotation}},   # x/y/rot: KINGDOM only
  "bga_current": int,          # argsPlaceDomino.domino
  "bga_previews": [{x, y, rotation, score}, ...] }               # the oracle, above
```

Engine seats are assigned by sorting the BGA player ids — any consistent labelling works (the
value head is seat-relative), and sorting makes it stable and debuggable across snapshots.

### The decision that isn't in the game state

BGA **merges place and claim into one server action** when the placement is staged first.
`selectDomino` (kingdomino.js) hangs the chosen tile off the staged position and submits
`placeDomino(position, nextDomino)`; the backend calls `chooseDomino` itself inside
`routeAfterPlacement`. So while the player is picking their next tile the server state is
still `placeDomino` — the status bar says "You must choose a domino" and `gamestate.name`
disagrees. The client's own comment is explicit: *"the placement commits when the player picks
their next domino, so no button."*

The only evidence is `gameui.selectedPosition`. When it is set, `page-agent.js` reports the
position **as it will be** — the tile moved to KINGDOM at the staged coordinates, `state`
rewritten to `chooseDomino` — so the claim is advised on the board the claim will actually
happen on. The reply carries `staged_placement` and the panel notes that the advice assumes
it. Four guards keep a stale staging from being read as a real one: the state must be
`placeDomino`, this client must be the active seat (BGA only clears `selectedPosition` when
*this* client enters `placeDomino` as the active player), the tile must still be in `CURRENT`,
and there must be an unclaimed tile to choose — on the final domino, or a treasure discovery,
BGA shows a standalone Confirm button and the pending decision really is still the placement.

Two further BGA conventions are handled in translation and nowhere else:

- **BGA's `CURRENT` is the line being placed from and `FUTURE` the one being claimed** — the
  engine's `current_line` / `next_line`. In the *starting* round `CURRENT` is empty and the
  claims land in `FUTURE`, so that first line maps to `current_line` instead.
- **`CURRENT` legitimately empties before a round's last claim.** The fourth king places (its
  tile leaves the line) and then claims; `drawDominoes` only rotates the lines afterwards,
  from `activateOwnerOfNextCurrentDomino`. A claim decision with an empty current line is a
  real position, not a torn read.

## Running it

```bash
cd agent
.venv/Scripts/python -m kdagent.server --checkpoint gen10.best --sims 400 --device cuda
```

Then load `advisor/extension/` unpacked via `chrome://extensions` (Developer mode) and open
the side panel on a BGA Kingdomino table. The panel's server box defaults to
`http://localhost:8000` — the same process that serves the play UI.

Server routes: `POST /recommend_bga`, `GET /latest`, `GET|POST /config` (sims + checkpoint,
re-scoring the live position immediately), `GET /models`, `GET /latest_snapshot`,
`GET /health`, `POST /debug_dump`. Every snapshot and reply is appended to
`runs/bga/<table>.jsonl` — the interesting positions are never the ones you thought to save,
and a live table cannot be paused to investigate.

## Testing

`kdagent/test_bga.py` carries the weight. `FakeBgaTable` replicates BGA's *bookkeeping* — the
`dominoes` table's locations and the king ownership column, plus `drawDominoes`' rotation rule
— and is driven in lockstep with a real engine game. At every decision it emits the snapshot
shape the extension sends, and the test asserts the translated position reproduces the
engine's own state: same acting seat, same phase and round, same deck, same kingdoms, same
scores, same legal actions. That exercises the whole capture path offline, over full games,
including the starting round, discards, and the final place-only round.

Underneath it, `core::rebuild`'s own tests round-trip every position of many random games
through `to_position` → `from_position`, and fork a rebuilt state to play a whole game in
lockstep with the original.

Not covered offline, and the reason to watch the first live game closely: that BGA's live
`gamedatas` really does match the shape assumed here, and that state args reach passive
clients (if they do not, `bga_previews` is simply absent and the oracle goes quiet — the
advice still works).

## Layout

```
advisor/
├── DESIGN.md            # this file
└── extension/           # load unpacked via chrome://extensions (Developer mode)
    ├── manifest.json
    ├── page-agent.js    # MAIN world: gamedatas + notification-sourced coordinates
    ├── content.js       # isolated world: bridge + on-board highlight
    ├── background.js    # service worker: localhost POST + reply cache
    ├── sidepanel.html
    └── sidepanel.js
```

## Known gaps / next steps

- **Not yet run against a live table.** Everything above is verified against a replica of
  BGA's backend bookkeeping, which is transcribed from the dump in `BGA Dump/kingdomino/` —
  faithful to the source, but not the same as the wire.
- **Hotseat is untested.** The capture handles the topology (visible-frame selection is
  ported from the Space Base agent), but hotseat and alt-account tables exercise different
  paths.
- **No position library.** Space Base can save a position for re-scoring under a later net;
  here the per-table recording is the raw material but there is no curation UI yet.
- **The advisor does not read the BGA move log**, which is the only other place a
  pre-extension discard could theoretically be recovered from.
