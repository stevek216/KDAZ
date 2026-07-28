# Implementation Plan: The Lost Treasures expansion

Rules reference: [RULES_LT.md](RULES_LT.md). Assets: `img/treasures.webp` (7-cell
sprite) + `.treasure*` CSS in [kingdomino.css](../kingdomino.css).

Goal: add "The Lost Treasures" as an optional variant (game option), reusing the
already-declared `token` table so **no DB migration / `upgradeTableDb` is needed**.

> **Convention:** never reference this plan document (or its filename) in any code
> comments. The plan is scratch/tracking only; production code must read on its own.

## 1. Mechanic summary (what the code must enforce)

- 16 treasure tokens: 5 gem colors x 3 identical + 1 Joker. Each has a Chest side
  (back) and a Gem side (front).
- Setup: all 16 face-down (Chest). 2 are flipped to Gem side -> **2 gems are always
  available** to choose from.
- On placing a domino, if it **completes a 2x2 square of 4 different terrains**, the
  player discovers a treasure: pick 1 of the 2 available gems, place it at the
  **intersection** (the shared corner of the 4 tiles), choose 1 of 4 orientations.
  Then flip a new Chest to Gem so 2 remain available.
- Each gem has 4 corner faces, each holding either **N crowns** or a **skull** (or
  nothing). Orientation rotates which face points at which of the 4 tiles.
- End scoring: gem crown faces **add crowns** to the territory of the tile they point
  at; a **skull** face makes that tile's whole territory score **0**.
- Immediate victory: a player who collects **5 different gem types** wins instantly,
  no scoring. Joker is wild (counts as any type) but only gives 1 crown.

## 2. Data model (no migration)

Reuse the existing [`token`](../dbmodel.sql) table `(token_key, token_location,
token_state)` as-is. Encode everything into those 3 fields.

| field | meaning |
|-------|---------|
| `token_key` | `treasure_1` .. `treasure_16`. **No type/color in the key** - the key is just an id. 16 rows. |
| `token_location` | `box` (chest, hidden) / `supply` (gem side, choosable) / `kingdom_<player_id>` (placed in that player's kingdom; not bare `<player_id>` since DbTokens rejects numeric-only locations) |
| `token_state` | while `box`/`supply`: reveal order `1..16`. While placed: encoded `(x,y,rotation)` of the intersection |

Position encoding (intersection identified by its bottom-left tile `(x,y)`, covering
tiles `(x,y),(x+1,y),(x,y+1),(x+1,y+1)`; rotation `0..3`):
`state = (x+9)*1000 + (y+9)*10 + rotation` (x,y in -9..9). Decimal-readable: the
digits split cleanly as `(x+9) | (y+9) | rotation`, e.g. `x=2,y=-1,rot=3` ->
`11083` reads as `11 | 08 | 3`. Add `encodeTokenPos` / `decodeTokenPos` helpers next
to [getRightTerrainCoordinates](../kingdomino.game.php#L320).

### Token accessor class

Copy the generic token-table helper from the Scholars project
(`../bga-scholars/modules/php/Db/DbTokens.php`) into `modules/php/Db/DbTokens.php`
**as-is** (only the namespace adjusted to this game). Do not trim it - the unused
game-level helpers are harmless dead code; we only call the generic CRUD. Instantiate
in the Game constructor with the game object: `$this->tokens = new DbTokens($this);`.

Treasure-specific queries then read naturally, e.g.
`getTokensOfTypeInLocation('treasure', 'supply')`, `getTokenOnTop('box')` to reveal,
`moveToken('treasure_4', $player_id, $encodedPos)` to place.

### Gem face data ([material.inc.php](../material.inc.php))

Material is keyed by token id `treasure_1..16`. Each entry has a `type` (the gem
identity - needed for the 5-different-types immediate victory) and `faces`. The DB key
stays type-free; the type lives only here. The `type` value is just an identifier
(name or int) - the three tokens of a gem share it.

```php
$this->treasures = [
  // type = gem identity (victory grouping); faces = [TL,TR,BR,BL], n crowns / 0 / -1 skull
  'treasure_1'  => ['type' => 'yellow', 'faces' => [1,1,1,0]],
  'treasure_2'  => ['type' => 'yellow', 'faces' => [1,1,1,0]],   // identical to 1
  'treasure_3'  => ['type' => 'yellow', 'faces' => [1,1,1,0]],
  // ... green (4-6), pink (7-9), blue (10-12), red (13-15) ...
  'treasure_16' => ['type' => 'joker',  'faces' => [0,0,1,0]],
];
```

The `joker` type is the wildcard: it counts as any missing type for the victory check
(but still scores only its own crowns) - no separate flag, `type === 'joker'` says it.
Each face is an int: `n` crowns, `0` nothing, `-1` skull. Exact per-corner values are
**placeholders** - transcribe from the token art later. Faces are in fixed clockwise
order `[TL, TR, BR, BL]`; orientation `r` rotates the assignment by `r` quarter-turns.
The three tokens of a gem are identical, so share both `type` and `faces`.

## 3. Game option

- [gameoptions.json](../gameoptions.json): new id `104` "The Lost Treasures" Off/On
  (`tmdisplay`, `nobeginner`). Decide compatibility with The Mighty Duel (7x7) - it
  should work; call it out in review.
- [kingdomino.game.php](../kingdomino.game.php#L32) `initGameStateLabels`: add
  `"the_lost_treasures" => 104`.
- Add `isLostTreasures()` helper (mirror [isMightyDuel()](../kingdomino.game.php#L231)).

## 4. State machine ([states.inc.php](../states.inc.php))

Insert one new active state between place and the existing routing.

```
chooseDomino(2) -> nextPlayer(3) -> placeDomino(4) --+--> chooseDomino(2)
                                                     +--> nextPlayer(3)
                                                     +--> placeTreasure(5)   [new]
placeTreasure(5) --+--> placeTreasure(5)   (another square from same domino)
                   +--> chooseDomino(2)
                   +--> nextPlayer(3)
                   +--> gameEnd(99)         (immediate victory)
```

- `placeDomino` (4): add transition `'placeTreasure' => 5`.
- `5 => placeTreasure`: `activeplayer`, `possibleactions: ['placeTreasure']`,
  `args: argsPlaceTreasure`, transitions to 2 / 3 / 5 / 99.

## 5. Server logic ([kingdomino.game.php](../kingdomino.game.php))

**Setup** (`setupNewGame`): if `isLostTreasures()`, insert the 16 tokens with a
shuffled reveal order, set the 2 lowest-order to `supply`, rest to `box`. New stats
init (see 9).

**Detect squares** (new `findNewTreasureSquares($kingdom, $placedTiles)`):
- A tile `(a,b)` belongs to candidate 2x2 squares with bottom-left corners
  `(a-1,b-1),(a-1,b),(a,b-1),(a,b)`.
- A square qualifies if: all 4 tiles filled, all 4 terrains **distinct**, no tile is
  the Castle (open question - confirm), it includes at least one newly placed tile,
  and its intersection has no gem yet.
- Return the list of unique qualifying intersections.

**Place domino** ([placeDomino](../kingdomino.game.php#L476)): after writing the
domino and score, if `isLostTreasures()` compute `findNewTreasureSquares`. If
non-empty and `supply` has tokens, stash the pending intersections (a global state
value or recompute in args) and `nextState('placeTreasure')` instead of the normal
branch. The `$nextDomino` auto-chain must be deferred until treasures are resolved.

**argsPlaceTreasure**: return the available `supply` gems (type + token_key), the
list of valid pending intersections, and the gem face data for client preview.

**placeTreasure($tokenKey, $x, $y, $rotation)** (+ wrapper in
[kingdomino.action.php](../kingdomino.action.php)):
- Validate token is in `supply`, `(x,y)` is one of the pending intersections,
  rotation in 0..3.
- Set token `token_location = kingdom_<player_id>`, `token_state = encode(x,y,rotation)`.
- Reveal next gem: flip the lowest-order `box` token to `supply` (if any).
- Notify `treasurePlaced` + `treasureRevealed`.
- Check immediate victory (see 7). Else: if more pending squares -> `placeTreasure`
  again; else resume normal flow (`chooseDomino` / `nextPlayer`, replaying any
  deferred `$nextDomino`).

## 6. Scoring ([finalScoring](../kingdomino.game.php#L657) + territories)

Extend the territory pass so gems contribute:
- Build territories as today via [getKingdomTerritories](../kingdomino.game.php#L272)
  (gives each territory its tile `locations`).
- For each placed gem: decode `(x,y,rotation)`, map its 4 rotated faces to the 4
  tiles `(x,y+1)=TL,(x+1,y+1)=TR,(x+1,y)=BR,(x,y)=BL`. For each face find that tile's
  territory; if the face is `-1` mark it `skulled`, otherwise add the face value to a
  per-territory `gemCrowns`.
- Territory score = `skulled ? 0 : size * (crowns + gemCrowns)`.
- Keep per-terrain stat accumulation working; gem crowns should flow into the same
  notifications (`scoreTerritory` / `scoreTerrain`) so the end screen and
  [getAllDatas](../kingdomino.game.php#L158) score sheet stay correct.

## 7. Immediate victory

- After each `placeTreasure`, collect the `type` (from material) of each gem the player
  has placed. Treating `type === 'joker'` as a free missing type, if distinct-types
  covered `>= 5`, that player wins now.
- Set their score above all others (or a dedicated flag the end screen reads), notify
  `immediateVictory`, `nextState('gameEnd')`, and **skip** `finalScoring`.

## 8. Client ([kingdomino.js](../kingdomino.js), tpl, css)

- `setup()` / `getAllDatas`: render the supply (2 gem tokens + Chest pile count) in a
  new template container; render already-placed gems on each kingdom at their
  intersection pixel position (`x*100`, `-y*100` grid, offset to the corner) with a
  `rotate(r*90deg)` transform. The token id is the CSS class: a gem renders with
  `class="treasure <token_key>"` (e.g. `treasure_4`), a chest with `treasure_back`,
  per the [CSS](../kingdomino.css#L441) - no type/color lookup needed.
- `onEnteringState('placeTreasure')`: highlight valid intersections, let the player
  select a supply gem, rotate it (reuse the existing rotate-arrow pattern around
  [manipulationArrows](../kingdomino.js#L322)), and click an intersection -> ajax
  `placeTreasure`. A live preview of crown/skull-per-tile is a nice-to-have.
- `setupNotifications`: add `treasurePlaced`, `treasureRevealed`, `immediateVictory`,
  and ensure scoring notifs render gem crowns.
- tpl/view: add supply container blocks (mirror `current_domino_space`); view exposes
  an `LT_VISIBLE` flag like the other variants.

## 9. Stats ([stats.json](../stats.json) + `initStat`)

Optional but cheap: `treasures_collected` (player), `treasure_crowns_score` (player),
and a table-level `immediate_victory` flag.

## 10. Zombie / async

- `zombieTurn` ([here](../kingdomino.game.php#L739)) needs a `placeTreasure` case:
  auto-pick the first supply gem and a valid orientation/intersection (prefer pointing
  any skull at the smallest/empty territory) so abandoned games still progress.

## 11. Open questions (resolve before coding)

1. One domino can complete **multiple** qualifying squares - one treasure each, or one
   per turn? (Plan assumes one per square, resolved sequentially.)
2. Does a 2x2 that includes the **Castle** count as "4 different landscapes"? (Plan
   assumes no.)
3. Is taking a discovered treasure **mandatory**? (Rules read mandatory; orientation
   lets you aim a skull harmlessly.)
4. Behavior when the **supply is empty** (more squares than gems left, esp. 7x7 Mighty
   Duel): skip the treasure silently.
5. RESOLVED - per-corner crown/skull values transcribed into `$this->treasures` from
   the token art.
6. RESOLVED - Joker provides exactly 1 crown and is wild only for the victory count.

## 12. Phases (progress tracking)

Each phase is independently testable by starting a new game with the option on.

- [x] **Phase 1 - Foundation.** Game option + state label + `isLostTreasures()` +
  copy `DbTokens` + material face data + token setup. Render supply read-only. No
  gameplay yet.
- [x] **Phase 2 - Discovery.** Square detection + `placeTreasure` state/action/args +
  persistence + reveal next gem.
- [x] **Phase 3 - Scoring.** Gem crowns + skull integrated into final scoring and the
  end-screen score sheet.
- [x] **Phase 4 - Immediate victory.** 5-types check (Joker wild) ends the game.
- [ ] **Phase 5 - Polish.** Zombie/async handling + Mighty Duel check + previews,
  animations, stats.

## 13. Manual test cases (no test harness - all manual)

Run the matching block on BGA Studio after each phase and tick when verified. Each
block assumes a fresh game **with the option ON** unless stated. Reset checkboxes when
re-testing after later changes.

### Phase 1 - Foundation

- [x] Option "The Lost Treasures" appears in the new-game setup and persists.
- [x] New game with option ON starts normally; option OFF behaves exactly like base.
- [x] DB `token` table has 16 rows: 2 in `supply`, 14 in `box`.
- [x] Supply UI shows 2 gem-side tokens + a chest pile count of 14.
- [x] F5 reload: supply still renders identically (`getAllDatas` round-trip).
- [x] Base gameplay (choose/place domino) is unaffected.

### Phase 2 - Discovery

- [x] Placing a domino that completes a 2x2 of 4 **different** terrains enters
  `placeTreasure`; a non-qualifying placement does not.
- [x] 2x2 with a repeated terrain does **not** trigger.
- [x] Player can pick either of the 2 supply gems, rotate through all 4 orientations,
  and place it at the highlighted intersection only.
- [x] After placing: a new gem flips from `box`, supply returns to 2, chest count -1.
- [x] One domino completing **two** squares offers two treasures in sequence.
- [x] 2x2 that includes the **Castle** does NOT trigger a treasure (current assumption;
 - [x] still open with the designer - revisit when answered).
- [x] F5 mid-`placeTreasure` resumes in the same state with the same choices.
- [x] Placed gem renders at the correct intersection with the correct rotation; survives
  reload.

### Phase 3 - Scoring

- [x] A gem's crown face adds its crowns to the **pointed** tile's territory (verify the
  end total vs hand calc).
- [ ] A skull face makes the pointed territory score **0**, even with crowns present.
- [x] Rotation actually changes which territories get crowns/skull.
- [x] End-of-game score sheet shows the gem contribution and the right grand total.
- [ ] Option OFF: final scores are byte-identical to base game (regression).

### Phase 4 - Immediate victory

- [ ] Collecting 5 distinct gem types ends the game **immediately**, that player wins,
  no normal scoring runs.
- [ ] Joker substitutes for a missing type to reach 5 (4 distinct + Joker wins).
- [ ] Two Jokers do **not** count as 2 types (still need 5 distinct slots).
- [ ] Reaching 5 types mid-turn beats a higher-scoring opponent.

### Phase 5 - Polish

- [ ] Zombie player in `placeTreasure` auto-resolves and the game continues.
- [ ] The Mighty Duel (7x7): discovery, placement, and scoring all work on the larger
  grid.
- [ ] Supply exhausted (no gems left): qualifying squares are skipped silently, no error.
- [ ] Stats (treasures collected / crown score / immediate victory) record correctly.
- [ ] 2, 3, and 4-player games each play through to the end without error.
