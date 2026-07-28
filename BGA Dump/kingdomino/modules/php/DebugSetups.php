<?php
/**
 * DebugSetups.php
 *
 * Test-only deterministic board seeders for Kingdomino.
 *
 * This is a trait composed into the GLOBAL `kingdomino` game class, so inside
 * these methods $this is the game object and all of its protected helpers
 * (gamestate, tokens, dominoes, DbQuery, getKingdomView, ...) are available.
 *
 * DEBUG ONLY: never call these from normal game flow. See debug_setup1 docblock
 * for the recommended (studio-guarded) invocation point.
 */

trait DebugSetups
{
    /**
     * Seed the Lost Treasures multi-square test for the ACTIVE player.
     *
     * Non-destructive: every OTHER player's KINGDOM (their placed dominoes) is left
     * untouched, so their UI still shows their real board. Only the active player's
     * kingdom and the shared current/future lines are re-dealt. Dominoes are picked
     * dynamically from the draw pile, so the helper never yanks a tile another
     * player has already placed.
     *
     * The active player ends up with a kingdom of castle + two vertical flank
     * dominoes, and a single CURRENT domino D to place. Placing D vertically at
     * (0,1) rotation 3 completes TWO qualifying 2x2 squares at once, so two gems
     * must be placed in sequence.
     *
     *   flankLeft  rot 3 at (-1,1) -> (-1,1),(-1,2)   left square
     *   flankRight rot 3 at ( 1,1) -> ( 1,1),( 1,2)   right square
     *   D          placed at (0,1) rot 3 -> (0,1),(0,2)
     *
     * D must have two DIFFERENT terrains and each flank two terrains distinct from
     * each other and from D's, so every completed square holds 4 distinct terrains
     * (a domino always fills both missing tiles of a completed square, so a
     * single-terrain D could never make a 4-distinct square). The flank dominoes do
     * not need to be legally connected - they are written straight to the DB and
     * only D's placement is validated - while D connects via the castle at (0,1).
     *
     * The future line is dealt WITH kings so the round can roll over after D is
     * placed without leaving an ownerless CURRENT domino (which would crash
     * changeActivePlayer(null)).
     *
     * Recommended invocation: call from the BGA Studio debug interface while a
     * Lost Treasures game is running, e.g. `$game->debug_setup1();`. It jumps the
     * FSM straight into placeDomino for the current active player (reload the
     * page to see the place UI).
     */
    public function debug_setup1(): void
    {
        // Test-only guard: only run on studio / non-production. BGA exposes the
        // studio flag via getBgaEnvironment(); if unavailable we still run but
        // this method is documented as debug-only and never auto-wired.
        if (method_exists($this, 'getBgaEnvironment') && $this->getBgaEnvironment() === 'prod') {
            return;
        }

        $players = $this->loadPlayersBasicInfos();
        $playerIds = array_map('intval', array_keys($players));
        $numPlayers = count($players);
        $activePlayerId = (int) $this->getActivePlayerId();
        if ($activePlayerId <= 0) {
            // Fall back to the first player so the helper is usable even before
            // an active player is set.
            $activePlayerId = $playerIds[0];
        }

        // Re-deal only the active player's kingdom and the shared current/future
        // lines; other players' KINGDOM dominoes stay where they are.
        self::DbQuery("UPDATE dominoes SET location='DRAW_PILE', owner_player=NULL, rotation=NULL, horizontal_position=NULL, vertical_position=NULL WHERE location='KINGDOM' AND owner_player=$activePlayerId");
        self::DbQuery("UPDATE dominoes SET location='DRAW_PILE', owner_player=NULL, rotation=NULL, horizontal_position=NULL, vertical_position=NULL WHERE location IN ('CURRENT','FUTURE')");

        // Free dominoes to draw from (everything except other players' placed tiles).
        $pile = array_map('intval', array_keys(
            self::getCollectionFromDB("SELECT `number` FROM dominoes WHERE location='DRAW_PILE' AND owner_player IS NULL ORDER BY `number`")
        ));
        $terrainsOf = fn(int $n) => [$this->dominoes[$n]['left']['terrain'], $this->dominoes[$n]['right']['terrain']];

        // D: the domino the active player will place. Two different terrains required.
        $D = null;
        foreach ($pile as $n) {
            [$left, $right] = $terrainsOf($n);
            if ($left !== $right) {
                $D = $n;
                break;
            }
        }
        if ($D === null) {
            $this->notifyAllPlayers('message', clienttranslate('Debug: no two-terrain domino available'), []);
            return;
        }
        $forbidden = $terrainsOf($D);

        // Two flank dominoes: two distinct terrains each, neither matching D's, so
        // each completed square = {flank.left, flank.right, D.left, D.right} = 4 distinct.
        $pickFlank = function (array $exclude) use ($pile, $terrainsOf, $forbidden) {
            foreach ($pile as $n) {
                if (in_array($n, $exclude, true)) {
                    continue;
                }
                [$left, $right] = $terrainsOf($n);
                if ($left !== $right && !in_array($left, $forbidden, true) && !in_array($right, $forbidden, true)) {
                    return $n;
                }
            }
            return null;
        };
        $flankLeft = $pickFlank([$D]);
        $flankRight = $pickFlank([$D, $flankLeft]);
        if ($flankLeft === null || $flankRight === null) {
            $this->notifyAllPlayers('message', clienttranslate('Debug: no flank dominoes available'), []);
            return;
        }

        // --- Active player's kingdom: castle + two vertical flanks. ---
        self::DbQuery("UPDATE dominoes SET location='KINGDOM', owner_player=$activePlayerId, rotation=3, horizontal_position=-1, vertical_position=1 WHERE `number`=$flankLeft");
        self::DbQuery("UPDATE dominoes SET location='KINGDOM', owner_player=$activePlayerId, rotation=3, horizontal_position=1, vertical_position=1 WHERE `number`=$flankRight");

        // --- D becomes the sole CURRENT domino, owned by the active player, so
        // placeDomino's "CURRENT ORDER BY number LIMIT 1" always selects it. ---
        self::DbQuery("UPDATE dominoes SET location='CURRENT', owner_player=$activePlayerId, rotation=NULL, horizontal_position=NULL, vertical_position=NULL WHERE `number`=$D");

        // --- Future line WITH kings, so the round can roll over without an
        // ownerless CURRENT (which would crash changeActivePlayer(null)). Keep one
        // (plus the 3-player-v2 discard) unclaimed for the active player to pick. ---
        $threePlayerOld = $numPlayers == 3 && self::getGameStateValue('3_players_v2') == 0;
        $dominoesPerTurn = $threePlayerOld ? 3 : 4;
        $unclaimedFuture = ($numPlayers == 3 && !$threePlayerOld) ? 2 : 1;
        $usedSoFar = [$D, $flankLeft, $flankRight];
        $futureDominoes = [];
        foreach ($pile as $n) {
            if (in_array($n, $usedSoFar, true)) {
                continue;
            }
            $futureDominoes[] = $n;
            if (count($futureDominoes) === $dominoesPerTurn) {
                break;
            }
        }
        self::DbQuery("UPDATE dominoes SET location='FUTURE', owner_player=NULL WHERE `number` IN (" . implode(',', $futureDominoes) . ")");
        $claimedCount = $dominoesPerTurn - $unclaimedFuture;
        for ($i = 0; $i < $claimedCount; $i++) {
            $owner = $playerIds[$i % $numPlayers];
            $number = $futureDominoes[$i];
            self::DbQuery("UPDATE dominoes SET owner_player=$owner WHERE `number`=$number");
        }

        // --- Deterministic draw-pile order for the leftover dominoes. ---
        $used = array_merge($usedSoFar, $futureDominoes);
        $position = 0;
        foreach ($pile as $n) {
            if (in_array($n, $used, true)) {
                continue;
            }
            self::DbQuery("UPDATE dominoes SET draw_pile_position=$position WHERE `number`=$n");
            $position++;
        }

        // --- Lost Treasures: make sure the supply holds 2 gems (one per square). ---
        // Tokens already exist (setupNewGame created them); only MOVE box gems up.
        if ($this->isLostTreasures()) {
            $slot = (int) $this->tokens->countTokensInLocation('supply');
            foreach (array_keys($this->treasures) as $key) {
                if ($slot >= 2) {
                    break;
                }
                if ($this->tokens->getTokenLocation($key) === 'box') {
                    $this->tokens->moveToken($key, 'supply', $slot);
                    $slot++;
                }
            }
        }

        // --- Game state values consistent with "about to place domino D". ---
        // turn_number 2 so post-placement routing uses the normal current-domino
        // flow rather than the first-round king-selection branch.
        self::setGameStateValue('lt_pending_domino', 0);
        self::setGameStateValue('lt_next_domino', 0);
        self::setGameStateValue('turn_number', 2);
        self::setGameStateValue('active_pick', 1);
        self::setGameStateValue('last_turn', 0);

        // Refresh only the active player's displayed score for the rebuilt kingdom.
        $score = $this->getKingdomScore($this->getKingdomView($activePlayerId));
        self::DbQuery("UPDATE player SET player_score=$score WHERE player_id=$activePlayerId");

        $this->notifyAllPlayers('message', clienttranslate('Debug: two-square scenario seeded for ${player_name}. Reload (F5), then place domino ${number} at (0,1) rotated vertically to complete two squares.'), array(
            'player_name' => $this->getActivePlayerName(),
            'number' => $D,
        ));

        // Jump straight into placeDomino (state 4) for the CURRENT active player.
        // jumpToState does not change the active player, so it avoids the
        // "changeActivePlayer during activeplayer state" (GS1) error - the seeded
        // player is already active and holds CURRENT domino D.
        $this->gamestate->jumpToState(4);
    }

    /**
     * Seed the Lost Treasures immediate-victory test in one step. The active player
     * already holds 4 gems of 4 different colors, and the 5th color sits in the
     * supply on a pending treasure square - placing it triggers the immediate win.
     *
     * The board is intentionally NOT a legal Kingdomino position: we only need a
     * single 2x2 of 4 distinct terrains (so a square is pending) plus the gems, then
     * we jump straight into placeTreasure. The live game state (FUTURE line, kings,
     * current domino, turn) is left untouched - only two spare DRAW_PILE dominoes
     * are relocated to form the square.
     *
     * Pre-placed gems: yellow/green/pink/blue (treasure_1/4/7/10), off the pending
     * square. Supply: red (treasure_13); swap to treasure_16 (joker) to also win.
     *
     * Recommended invocation: from the BGA Studio debug interface during a Lost
     * Treasures game, e.g. `$game->debug_setupImmediate();`. Reload to see the board.
     */
    public function debug_setupImmediate(): void
    {
        $this->notifyAllPlayers('message', clienttranslate('Debug: immediate-victory scenario seed'), []);
        $activePlayerId = (int) $this->getActivePlayerId();
        if ($activePlayerId <= 0) {
            $players = $this->loadPlayersBasicInfos();
            $activePlayerId = (int) array_key_first($players);
        }

        // Pick two spare DRAW_PILE dominoes (unowned, no king) whose four halves are
        // four distinct terrains. Only relocating draw-pile dominoes keeps the live
        // FUTURE/kings/current/turn state intact.
        $pile = self::getCollectionFromDB("SELECT number FROM dominoes WHERE location = 'DRAW_PILE' AND owner_player IS NULL ORDER BY number");
        $domA = null;
        $domB = null;
        foreach (array_keys($pile) as $a) {
            $na = (int) $a;
            foreach (array_keys($pile) as $b) {
                $nb = (int) $b;
                if ($nb <= $na) {
                    continue;
                }
                $terrains = [
                    $this->dominoes[$na]['left']['terrain'],
                    $this->dominoes[$na]['right']['terrain'],
                    $this->dominoes[$nb]['left']['terrain'],
                    $this->dominoes[$nb]['right']['terrain'],
                ];
                if (count(array_unique($terrains)) === 4) {
                    $domA = $na;
                    $domB = $nb;
                    break 2;
                }
            }
        }
        if ($domA === null) {
            $this->notifyAllPlayers('message', clienttranslate('Error: new pair avail'), []);
            return; // no suitable pair available
        }

        // One 2x2 square of 4 distinct terrains at (1,1),(2,1),(1,2),(2,2). Legality
        // is irrelevant - we go straight to the treasure step.
        self::DbQuery("UPDATE dominoes SET location = 'KINGDOM', owner_player = $activePlayerId, rotation = 0, horizontal_position = 1, vertical_position = 1 WHERE `number` = $domA");
        self::DbQuery("UPDATE dominoes SET location = 'KINGDOM', owner_player = $activePlayerId, rotation = 0, horizontal_position = 1, vertical_position = 2 WHERE `number` = $domB");

        // Tokens: all to the box, then 4 collected gems (4 distinct colors) off the
        // pending square, and the 5th color into the supply to be discovered.
        foreach (array_keys($this->treasures) as $key) {
            $this->tokens->moveToken($key, 'box', 0);
        }
        $placedGems = [
            ['treasure_1', -1, -1, 0],  // yellow
            ['treasure_4', -2, -1, 0],  // green
            ['treasure_7', -1, -2, 0],  // pink
            ['treasure_10', -2, -2, 0], // blue
        ];
        foreach ($placedGems as [$key, $x, $y, $rotation]) {
            $this->tokens->moveToken($key, $this->treasureLocation($activePlayerId), $this->encodeTokenPos($x, $y, $rotation));
        }
        $this->tokens->moveToken('treasure_13', 'supply', 0);

        // Mark domA pending so getPendingTreasureSquares returns the (1,1) square,
        // then jump straight into placeTreasure (state 5) for the active player.
        self::setGameStateValue('lt_pending_domino', $domA);
        self::setGameStateValue('lt_next_domino', 0);

        // Visible feedback in the game log - a debug run otherwise sends nothing,
        // so the open client shows no change until a reload.
        $this->notifyAllPlayers('message', clienttranslate('Debug: immediate-victory scenario seeded for ${player_name}. Reload the page (F5), then place the gem on the highlighted square to win.'), array(
            'player_name' => $this->getActivePlayerName(),
        ));

        $this->gamestate->jumpToState(5);
    }

    /**
     * Swap one current supply gem for a skull gem (a blue token whose faces
     * include -1) so the skull scoring can be tested. Reload the page after
     * calling so the placeTreasure args pick up the new supply.
     */
    public function debug_skull(): void
    {
        if (method_exists($this, 'getBgaEnvironment') && $this->getBgaEnvironment() === 'prod') {
            return;
        }
        if (!$this->isLostTreasures()) {
            return;
        }

        $supply = $this->tokens->getTokensOfTypeInLocation('treasure', 'supply', null, 'state');
        if (count($supply) === 0) {
            return;
        }

        // A skull gem has a -1 (skull) face; take one that is still in the box.
        $skullKey = null;
        foreach ($this->treasures as $key => $info) {
            if (in_array(-1, $info['faces'], true) && $this->tokens->getTokenLocation($key) === 'box') {
                $skullKey = $key;
                break;
            }
        }
        if ($skullKey === null) {
            return;
        }

        // Drop the skull gem into the first supply slot, returning the old gem to the box.
        $replaced = array_key_first($supply);
        $slot = (int) $supply[$replaced]['state'];
        $this->tokens->moveToken($replaced, 'box', 0);
        $this->tokens->moveToken($skullKey, 'supply', $slot);
    }

        public function debug_finalScoring(): void {
        $this->finalScoring();
        }
}
