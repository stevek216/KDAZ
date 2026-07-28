<?php
/**
 *------
 * BGA framework: © Gregory Isabelli <gisabelli@boardgamearena.com> & Emmanuel Colin <ecolin@boardgamearena.com>
 * kingdomino implementation : © Alena Laskavaia <laskava@gmail.com> & Romain Fromi <romain.fromi@gmail.com>
 *
 * This code has been produced on the BGA studio platform for use on http://boardgamearena.com.
 * See http://en.boardgamearena.com/#!doc/Studio for more information.
 * -----
 *
 * kingdomino.game.php
 *
 * This is the main file for your game logic.
 *
 * In this PHP file, you are going to defines the rules of the game.
 *
 */
require_once(APP_GAMEMODULE_PATH . 'module/table/table.game.php');
require_once(__DIR__ . '/modules/php/Db/DbTokens.php');
require_once(__DIR__ . '/modules/php/DebugSetups.php');

class kingdomino extends Table
{
    use DebugSetups;

    public \Bga\Games\Kingdomino\Db\DbTokens $tokens;
    public array $dominoes;   // loaded from material.inc.php
    public array $terrains;   // loaded from material.inc.php
    public array $treasures;  // loaded from material.inc.php
    public bool $gameinit;

    function __construct()
    {
        // Your global variables labels:
        //  Here, you can assign labels to global variables you are using for this game.
        //  You can use any number of global variables with IDs between 10 and 99.
        //  If your game has options (variants), you also have to associate here a label to
        //  the corresponding ID in gameoptions.inc.php.
        // Note: afterwards, you can get/set the global variables with getGameStateValue/setGameStateInitialValue/setGameStateValue
        parent::__construct();
        self::initGameStateLabels(array( //
            "move_nbr" => 6,
            // mine
            "turn_number" => 10, // current turn number
            "active_pick" => 11, // number of pick line 1 or 2
            "last_turn" => 12, // last turn flag
            "3_players_v2" => 13, // active rule for 3 players games (migration purpose)
            "lt_pending_domino" => 14, // domino whose just-placed tiles may still complete treasure squares (0 = none)
            "lt_next_domino" => 15, // domino auto-chosen after treasures resolve (0 = none)
            "dynasty" => 100,
            "the_middle_kingdom" => 101,
            "harmony" => 102,
            "the_mighty_duel" => 103,
            "the_lost_treasures" => 104,
        ));
        $this->tokens = new \Bga\Games\Kingdomino\Db\DbTokens($this);
        $this->gameinit = false;
    }

    protected function getGameName()
    {
        // Used for translations and stuff. Please do not modify.
        return "kingdomino";
    }

    /*
     * setupNewGame:
     *
     * This method is called only once, when a new game is launched.
     * In this method, you must setup the game according to the game rules, so that
     * the game is ready to be played.
     */
    protected function setupNewGame($players, $options = array())
    {
        // Set the colors of the players with HTML color code
        // The default below is red/green/blue/orange/brown
        // The number of colors defined here must correspond to the maximum number of players allowed for the gams
        $gameinfos = self::getGameinfos();
        $default_colors = $gameinfos ['player_colors'];
        // Create players
        // Note: if you added some extra field on "player" table in the database (dbmodel.sql), you can initialize it there.
        $sql = "INSERT INTO player (player_id, player_color, player_canal, player_name, player_avatar) VALUES ";
        $values = array();
        foreach ($players as $player_id => $player) {
            $color = array_shift($default_colors);
            $values [] = "('" . $player_id . "','$color','" . $player ['player_canal'] . "','" . addslashes($player ['player_name']) . "','" . addslashes($player ['player_avatar']) . "')";
        }
        $sql .= implode(',', $values);
        self::DbQuery($sql);
        self::reattributeColorsBasedOnPreferences($players, $gameinfos ['player_colors']);
        self::reloadPlayersBasicInfos();
        /**
         * ********** Start the game initialization ****
         */
        // Init global values with their initial values
        self::setGameStateInitialValue('active_pick', 1);
        self::setGameStateInitialValue('last_turn', 0);
        self::setGameStateInitialValue('3_players_v2', 1);
        self::setGameStateInitialValue('lt_pending_domino', 0);
        self::setGameStateInitialValue('lt_next_domino', 0);

        $mightyDuel = self::getGameStateValue('the_mighty_duel') == 1;

        // Setup the initial game situation here
        $numberOfPlayers = count($players);
        $dominoes = range(1, 48);
        $values = [];
        shuffle($dominoes);
        if ($numberOfPlayers == 2 && !$mightyDuel) {
            $dominoes = array_slice($dominoes, 24);
        }
        foreach ($dominoes as $position => $domino) {
            $values[] = "($domino, $position)";
        }
        self::dbQuery("INSERT INTO dominoes (`number`, `draw_pile_position`) VALUES " . join(',', $values));
        $this->drawDominoes($numberOfPlayers);

        if ($this->isLostTreasures()) {
            $this->setupTreasures();
        }

        $this->activeNextPlayer(); // just in case so its not 0

        // Most game statistics are intentionally NOT init'd here. A stat that is never
        // init'd and never set/inc'd is excluded from that game's stats analysis,
        // which is what we want: finalScoring inc's the per-terrain/bonus scores (so
        // normally-ended games record them), while immediate-victory games (which skip
        // finalScoring) leave them unset. immediate_victory is set only when a player
        // actually wins that way. Reading an un-init stat returns 0, so the score-sheet
        // UI still works.
        // discarded_dominoes is the exception: it is inc'd during play, so it must be
        // init'd here (before any incStat) and NOT in finalScoring, which would reset it.
        self::initStat('player', 'discarded_dominoes', 0);

        /**
         * ********** End of the game initialization ****
         */
    }

    function drawDominoes($numberOfPlayers)
    {
        if ($numberOfPlayers == 3 && self::getGameStateValue('turn_number') > 0 && self::getGameStateValue('3_players_v2') == 1) {
            // After all 3 players have selected a domino, discard the leftover domino into the box every round.
            $number = self::getUniqueValueFromDB("SELECT `number` FROM dominoes WHERE location = 'FUTURE' AND owner_player IS NULL LIMIT 1");
            self::dbQuery("UPDATE dominoes SET location = 'DISCARD' WHERE `number` = $number");
            self::notifyAllPlayers('dominoDiscarded', clienttranslate('Domino ${number} is discarded'), array('number' => $number));
        }
        $quantity = $numberOfPlayers != 3 || self::getGameStateValue('3_players_v2') == 1 ? 4 : 3;
        self::dbQuery("UPDATE dominoes SET location = 'CURRENT' WHERE location = 'FUTURE'");
        self::dbQuery("UPDATE dominoes SET location = 'FUTURE', draw_pile_position = NULL WHERE location = 'DRAW_PILE' ORDER BY draw_pile_position DESC LIMIT $quantity");
        $dominoesDrawn = self::getObjectListFromDB("SELECT `number` FROM dominoes WHERE location = 'FUTURE' ORDER BY `number`", true);
        if (sizeof($dominoesDrawn) > 0) {
            $message = clienttranslate('New dominoes revealed: ${first}, ${second}, ${third}, ${fourth}');
            $args = array('dominoes' => $dominoesDrawn, 'first' => $dominoesDrawn[0], 'second' => $dominoesDrawn[1], 'third' => $dominoesDrawn[2]);
            if ($quantity == 3) {
                $message = clienttranslate('New dominoes revealed: ${first}, ${second}, ${third}');
            } else {
                $args['fourth'] = $dominoesDrawn[3];
            }
            self::incGameStateValue('turn_number', 1);
            self::notifyAllPlayers("dominoesDrawn", $message, $args);
        } else {
            self::notifyAllPlayers("dominoesDrawn", clienttranslate('There are no more dominoes, this is the last turn!'), array('dominoes' => array()));
        }
    }

    /*
     * getAllDatas:
     *
     * Gather all informations about current game situation (visible by the current player).
     *
     * The method is called each time the game interface is displayed to a player, ie:
     * _ when the game starts
     * _ when a player refreshes the game page (F5)
     */
    protected function getAllDatas()
    {
        $result = array();
        // Get information about players
        // Note: you can retrieve some extra field you added for "player" table in "dbmodel.sql" if you need it.
        $sql = "SELECT player_id id, player_score score, player_name, player_color FROM player ";
        $players = self::getCollectionFromDb($sql);
        $result['players'] = $players;
        $result['dominoes'] = self::getCollectionFromDb("SELECT `number`, location, owner_player, rotation, horizontal_position x, vertical_position y FROM dominoes WHERE location IN ('CURRENT', 'FUTURE', 'KINGDOM') ORDER BY number");
        $dominoesPerTurn = sizeof($players) == 3 && self::getGameStateValue('3_players_v2') == 0 ? 3 : 4;
        $turnsLeft = self::getUniqueValueFromDB("SELECT count(*) FROM dominoes WHERE location = 'DRAW_PILE'") / $dominoesPerTurn;
        if (self::getUniqueValueFromDB("SELECT 1 FROM dominoes WHERE location = 'FUTURE' LIMIT 1") != null) {
            $turnsLeft++;
        }
        $result['turnsLeft'] = $turnsLeft;
        $result['dominoesDescription'] = $this->dominoes;
        $result['gridSize'] = $this->isMightyDuel() ? 7 : 5;

        if ($this->gamestate->state()['name'] == 'gameEnd') {
            $result['scoring'] = [];
            $result['scoreSheet'] = [];
            foreach ($players as $player_id => $player) {
                $scoreSheet = ['name' => $player['player_name'], 'color' => $player['player_color']];
                $kingdom = $this->getKingdomView($player_id);
                $scoringTerritories = ['field' => [], 'forest' => [], 'lake' => [], 'grassland' => [], 'swamp' => [], 'mountain' => []];
                foreach ($this->getScoringTerritories($player_id, $kingdom) as $territory) {
                    if ($territory['score'] > 0) {
                        $scoringTerritories[$territory['terrain']][] = $territory;
                    }
                }
                foreach ($this->terrains as $terrain => $terrainName) {
                    $scoreForTerrain = self::getStat($terrain . '_score', $player_id);
                    $scoreSheet[$terrain] = $scoreForTerrain;
                    foreach ($scoringTerritories[$terrain] as $territory) {
                        $result['scoring'][] = array(
                            'playerId' => $player_id,
                            'player_name' => $player['player_name'],
                            'territory' => $territory,
                            'totalScoreForTerrain' => $scoreForTerrain,
                            'terrain' => $terrainName);
                    }
                }
                $scoreSheet['displayBonus'] = self::getGameStateValue('the_middle_kingdom') == 1 || self::getGameStateValue('harmony') == 1;
                $scoreSheet['bonus'] = self::getStat('the_middle_kingdom_score', $player_id) + self::getStat('harmony_score', $player_id);
                $scoreSheet['score'] = $this->getUniqueValueFromDB("SELECT player_score FROM player WHERE player_id='$player_id'");
                $result['scoreSheet'][] = $scoreSheet;
            }
        }

        if ($this->isLostTreasures()) {
            $supply = $this->tokens->getTokensOfTypeInLocation('treasure', 'supply', null, 'token_state');
            $placed = [];
            foreach ($players as $player_id => $player) {
                $gems = $this->tokens->getTokensOfTypeInLocation('treasure', $this->treasureLocation($player_id));
                foreach ($gems as $key => $gem) {
                    $placed[$player_id][] = ['key' => $key] + $this->decodeTokenPos((int) $gem['state']);
                }
            }
            $result['treasures'] = [
                'supply' => array_keys($supply),
                'boxCount' => (int) $this->tokens->countTokensInLocation('box'),
                'placed' => $placed,
                'description' => $this->treasures,
            ];
        }

        return $result;
    }

    /*
     * getGameProgression:
     *
     * Compute and return the current game progression.
     * The number returned must be an integer beween 0 (=the game just started) and
     * 100 (= the game is finished or almost finished).
     *
     * This method is called each time we are in a game state with the "updateGameProgression" property set to true
     * (see states.inc.php)
     */
    function getGameProgression()
    {
        $dominoes = self::getUniqueValueFromDB("SELECT count(*) FROM dominoes");
        $dominoToPlace = self::getUniqueValueFromDB("SELECT count(*) FROM dominoes WHERE location NOT IN ('KINGDOM', 'DISCARD')");
        return 100 * ($dominoes - $dominoToPlace) / $dominoes;
    }

    //////////////////////////////////////////////////////////////////////////////
    //////////// Utility functions
    ////////////

    function isMightyDuel()
    {
        $players = $this->loadPlayersBasicInfos();
        return sizeof($players) == 2 && self::getGameStateValue('the_mighty_duel') == 1;
    }

    function isLostTreasures()
    {
        return self::getGameStateValue('the_lost_treasures') == 1;
    }

    function setupTreasures()
    {
        $tokens = [];
        foreach (array_keys($this->treasures) as $key) {
            $tokens[] = ['key' => $key];
        }
        $this->tokens->createTokens($tokens, 'box');
        // Shuffle to assign a reveal order, then reveal the 2 lowest-order gems.
        $this->tokens->shuffle('box');
        $next = $this->tokens->getTokensOfTypeInLocation('treasure', 'box', null, 'token_state');
        $reveal = array_slice(array_keys($next), 0, 2);
        $this->tokens->moveTokens($reveal, 'supply');
    }

    function getKingdomView($player_id)
    {
        $kingdom = array();
        // Create all possible lines (and more) to avoid more checks later on
        for ($x = -9; $x <= 9; $x++) {
            $kingdom[$x] = array();
            for ($y = -9; $y <= 9; $y++) {
                $kingdom[$x][$y] = null;
            }
        }
        $kingdom[0][0] = array('terrain' => 'Castle', 'crowns' => 0);
        $dominoes = self::getCollectionFromDb("SELECT `number`, rotation, horizontal_position x, vertical_position y FROM dominoes WHERE location = 'KINGDOM' AND owner_player = $player_id");
        foreach ($dominoes as $number => $position) {
            $this->addToKingdomView($kingdom, $number, $position);
        }
        return $kingdom;
    }

    function addToKingdomView(&$kingdom, $number, $position)
    {
        $domino = $this->dominoes[$number];
        $rightTerrainCoordinates = $this->getRightTerrainCoordinates($position);
        $kingdom[$position['x']][$position['y']] = $domino['left'];
        $kingdom[$rightTerrainCoordinates['x']][$rightTerrainCoordinates['y']] = $domino['right'];
    }

    function getKingdomScore($kingdom)
    {
        $score = 0;
        foreach ($this->getKingdomTerritories($kingdom) as $territory) {
            $score += $territory['size'] * $territory['crowns'];
        }
        return $score;
    }

    // Running score including this player's already-placed gems. With Lost Treasures off
    // it equals getKingdomScore (gems add nothing), so the base game is unaffected.
    function getKingdomScoreWithGems($player_id, $kingdom)
    {
        $score = 0;
        foreach ($this->getScoringTerritories($player_id, $kingdom) as $territory) {
            $score += $territory['score'];
        }
        return $score;
    }

    function getKingdomTerritories($kingdom)
    {
        $territories = array();
        for ($x = -6; $x <= 6; $x++) {
            $kingdomToTerritories[$x] = array();
            for ($y = -6; $y <= 6; $y++) {
                if (isset($kingdom[$x][$y])) {
                    if (isset($kingdom[$x - 1]) && isset($kingdom[$x - 1][$y]) && $kingdom[$x - 1][$y]['terrain'] === $kingdom[$x][$y]['terrain']) {
                        $kingdomToTerritories[$x][$y] = &$kingdomToTerritories[$x - 1][$y];
                    }
                    if (isset($kingdom[$x][$y - 1]) && $kingdom[$x][$y - 1]['terrain'] === $kingdom[$x][$y]['terrain']) {
                        if (!isset($kingdomToTerritories[$x][$y])) {
                            $kingdomToTerritories[$x][$y] = &$kingdomToTerritories[$x][$y - 1];
                        } else if ($kingdomToTerritories[$x][$y] != $kingdomToTerritories[$x][$y - 1]) {
                            $this->mergeTerritories($kingdomToTerritories, $territories, $kingdomToTerritories[$x][$y], $kingdomToTerritories[$x][$y - 1]);
                        }
                    }
                    if (!isset($kingdomToTerritories[$x][$y])) {
                        $kingdomToTerritories[$x][$y] = $kingdom[$x][$y];
                        $kingdomToTerritories[$x][$y]['size'] = 1;
                        $kingdomToTerritories[$x][$y]['locations'] = [['x' => $x, 'y' => $y]];
                        $territories[] = &$kingdomToTerritories[$x][$y];
                    } else {
                        $kingdomToTerritories[$x][$y]['size']++;
                        $kingdomToTerritories[$x][$y]['locations'][] = ['x' => $x, 'y' => $y];
                        $kingdomToTerritories[$x][$y]['crowns'] += $kingdom[$x][$y]['crowns'];
                    }
                }
            }
        }
        return $territories;
    }

    function mergeTerritories(&$kingdomToTerritories, &$territories, &$territory1, &$territory2)
    {
        foreach ($kingdomToTerritories as $x => $line) {
            foreach ($line as $y => $territory) {
                if ($territory == $territory2) {
                    $kingdomToTerritories[$x][$y] = &$territory1;
                }
            }
        }
        $territory1['crowns'] += $territory2['crowns'];
        $territory1['size'] += $territory2['size'];
        $territory1['locations'] = array_merge($territory1['locations'], $territory2['locations']);
        unset($territories[array_search($territory2, $territories)]);
    }

    function getRightTerrainCoordinates($position)
    {
        switch ($position['rotation']) {
            case 0:
                return array('x' => $position['x'] + 1, 'y' => $position['y']);
            case 1:
                return array('x' => $position['x'], 'y' => $position['y'] - 1);
            case 2:
                return array('x' => $position['x'] - 1, 'y' => $position['y']);
            default:
                return array('x' => $position['x'], 'y' => $position['y'] + 1);
        }
    }

    function encodeTokenPos($x, $y, $rotation)
    {
        return ($x + 9) * 1000 + ($y + 9) * 10 + $rotation;
    }

    function decodeTokenPos($state)
    {
        $rotation = $state % 10;
        $y = (intdiv($state, 10) % 100) - 9;
        $x = intdiv($state, 1000) - 9;
        return array('x' => $x, 'y' => $y, 'rotation' => $rotation);
    }

    // Per-player location holding the gems placed in that kingdom.
    function treasureLocation($player_id)
    {
        return 'kingdom_' . $player_id;
    }

    // Intersections (bottom-left tile of the 2x2) that already hold a placed gem.
    function getOccupiedIntersections($player_id)
    {
        $gems = $this->tokens->getTokensOfTypeInLocation('treasure', $this->treasureLocation($player_id));
        $occupied = array();
        foreach ($gems as $gem) {
            $pos = $this->decodeTokenPos((int) $gem['state']);
            $occupied[] = array('x' => $pos['x'], 'y' => $pos['y']);
        }
        return $occupied;
    }

    // Apply this player's placed gems to the given territories: add crown faces to the
    // territory of the tile each face points at, and mark a territory skulled when a
    // skull face (-1) points at one of its tiles. Each territory gains 'gemCrowns' and
    // 'skulled', and its 'score' is set: skulled territories score 0, otherwise
    // size * (crowns + gemCrowns). Territories are passed by reference and mutated.
    function applyGemsToTerritories($player_id, &$territories)
    {
        foreach ($territories as &$territory) {
            if (!isset($territory['gemCrowns'])) {
                $territory['gemCrowns'] = 0;
            }
            if (!isset($territory['skulled'])) {
                $territory['skulled'] = false;
            }
        }
        unset($territory);

        // Map every territory tile to its territory index for fast lookup.
        $tileToTerritory = array();
        foreach ($territories as $index => $territory) {
            foreach ($territory['locations'] as $location) {
                $tileToTerritory[$location['x'] . ',' . $location['y']] = $index;
            }
        }

        $gems = $this->tokens->getTokensOfTypeInLocation('treasure', $this->treasureLocation($player_id));
        foreach ($gems as $key => $gem) {
            $pos = $this->decodeTokenPos((int) $gem['state']);
            $faces = $this->treasures[$key]['faces'];
            // Clockwise tile positions 0=TL,1=TR,2=BR,3=BL around the intersection (x,y).
            $tiles = array(
                array($pos['x'], $pos['y'] + 1),
                array($pos['x'] + 1, $pos['y'] + 1),
                array($pos['x'] + 1, $pos['y']),
                array($pos['x'], $pos['y']),
            );
            for ($p = 0; $p < 4; $p++) {
                // A clockwise rotation moves the TL face toward the TR position, so the
                // tile at clockwise-position p displays faces[(p - rotation + 4) % 4].
                $face = $faces[($p - $pos['rotation'] + 4) % 4];
                $tileKey = $tiles[$p][0] . ',' . $tiles[$p][1];
                if (!isset($tileToTerritory[$tileKey])) {
                    continue;
                }
                $index = $tileToTerritory[$tileKey];
                if ($face === -1) {
                    $territories[$index]['skulled'] = true;
                } else {
                    $territories[$index]['gemCrowns'] += $face;
                }
            }
        }

        foreach ($territories as &$territory) {
            $territory['score'] = $territory['skulled']
                ? 0
                : $territory['size'] * ($territory['crowns'] + $territory['gemCrowns']);
        }
        unset($territory);
    }

    // Territories of a player's kingdom, gem-adjusted when the variant is on: each gets
    // 'gemCrowns', 'skulled', and a final 'score'. With the variant off (no placed gems)
    // every territory simply scores size * crowns, matching the base game exactly.
    function getScoringTerritories($player_id, $kingdom)
    {
        $territories = $this->getKingdomTerritories($kingdom);
        foreach ($territories as &$territory) {
            $territory['gemCrowns'] = 0;
            $territory['skulled'] = false;
            $territory['score'] = $territory['size'] * $territory['crowns'];
        }
        unset($territory);
        if ($this->isLostTreasures()) {
            $this->applyGemsToTerritories($player_id, $territories);
        }
        return $territories;
    }

    // Tiles (x,y) covered by a placed domino: its left tile plus the right terrain coordinate.
    function getDominoTiles($number)
    {
        $position = self::getObjectFromDB("SELECT rotation, horizontal_position x, vertical_position y FROM dominoes WHERE `number` = $number AND location = 'KINGDOM'");
        if ($position == null) {
            return array();
        }
        $position['x'] = (int) $position['x'];
        $position['y'] = (int) $position['y'];
        $position['rotation'] = (int) $position['rotation'];
        $right = $this->getRightTerrainCoordinates($position);
        return array(
            array('x' => $position['x'], 'y' => $position['y']),
            array('x' => $right['x'], 'y' => $right['y']),
        );
    }

    // 2x2 squares (identified by bottom-left tile) that now qualify for a treasure:
    // 4 filled tiles, 4 distinct terrains, no Castle, touch a just-placed tile, no gem yet.
    function findNewTreasureSquares($kingdom, $placedTiles, $occupied = array())
    {
        $found = array();
        $occupiedSet = array();
        foreach ($occupied as $cell) {
            $occupiedSet[$cell['x'] . ',' . $cell['y']] = true;
        }
        foreach ($placedTiles as $tile) {
            // A tile belongs to up to four 2x2 blocks, each given by its bottom-left corner.
            foreach (array(array(-1, -1), array(-1, 0), array(0, -1), array(0, 0)) as $offset) {
                $cx = $tile['x'] + $offset[0];
                $cy = $tile['y'] + $offset[1];
                $key = $cx . ',' . $cy;
                if (isset($found[$key]) || isset($occupiedSet[$key])) {
                    continue;
                }
                $corners = array(
                    $kingdom[$cx][$cy] ?? null,
                    $kingdom[$cx + 1][$cy] ?? null,
                    $kingdom[$cx][$cy + 1] ?? null,
                    $kingdom[$cx + 1][$cy + 1] ?? null,
                );
                $terrains = array();
                $valid = true;
                foreach ($corners as $corner) {
                    if ($corner == null || $corner['terrain'] == 'Castle') {
                        $valid = false;
                        break;
                    }
                    $terrains[$corner['terrain']] = true;
                }
                if ($valid && count($terrains) == 4) {
                    $found[$key] = array('x' => $cx, 'y' => $cy);
                }
            }
        }
        return array_values($found);
    }

    // Pending intersections recomputed from the stored domino, minus those already filled.
    function getPendingTreasureSquares($player_id)
    {
        $pendingDomino = (int) self::getGameStateValue('lt_pending_domino');
        if ($pendingDomino == 0) {
            return array();
        }
        $kingdom = $this->getKingdomView($player_id);
        $placedTiles = $this->getDominoTiles($pendingDomino);
        return $this->findNewTreasureSquares($kingdom, $placedTiles, $this->getOccupiedIntersections($player_id));
    }

    function positionIsOccupied($kingdom, $position)
    {
        $rightTerrainCoordinates = $this->getRightTerrainCoordinates($position);
        return isset($kingdom[$position['x']][$position['y']]) || isset($kingdom[$rightTerrainCoordinates['x']][$rightTerrainCoordinates['y']]);
    }

    function dominoConnects($kingdom, $domino, $position)
    {
        return $this->terrainConnects($kingdom, $domino['left']['terrain'], $position)
            || $this->terrainConnects($kingdom, $domino['right']['terrain'], $this->getRightTerrainCoordinates($position));
    }

    function terrainConnects($kingdom, $terrain, $position)
    {
        $x = $position['x'];
        $y = $position['y'];
        return $this->isAdjacentToCastle($x, $y)
            || (isset($kingdom[$x + 1]) && isset($kingdom[$x + 1][$y]) && $kingdom[$x + 1][$y]['terrain'] == $terrain)
            || (isset($kingdom[$x - 1]) && isset($kingdom[$x - 1][$y]) && $kingdom[$x - 1][$y]['terrain'] == $terrain)
            || (isset($kingdom[$x][$y + 1]) && $kingdom[$x][$y + 1]['terrain'] == $terrain)
            || (isset($kingdom[$x][$y - 1]) && $kingdom[$x][$y - 1]['terrain'] == $terrain);
    }

    function isAdjacentToCastle($x, $y)
    {
        return $x == 0 && abs($y) == 1 || $y == 0 && abs($x) == 1;
    }

    function positionFitsInGrid($kingdom, $position)
    {
        $gridSize = $this->isMightyDuel() ? 7 : 5;
        switch ($position['rotation']) {
            case 0:
                $xExtreme = $position['x'] >= 0 ? $position['x'] + 1 : $position['x'];
                $yExtreme = $position['y'];
                break;
            case 1:
                $xExtreme = $position['x'];
                $yExtreme = $position['y'] <= 0 ? $position['y'] - 1 : $position['y'];
                break;
            case 2:
                $xExtreme = $position['x'] <= 0 ? $position['x'] - 1 : $position['x'];
                $yExtreme = $position['y'];
                break;
            default:
                $xExtreme = $position['x'];
                $yExtreme = $position['y'] >= 0 ? $position['y'] + 1 : $position['y'];
                break;
        }
        if ($xExtreme != 0) {
            $xExpectedEmpty = $xExtreme > 0 ? $xExtreme - $gridSize : $xExtreme + $gridSize;
            for ($y = 1 - $gridSize; $y < $gridSize; $y++) {
                if ($kingdom[$xExpectedEmpty][$y]) {
                    return false;
                }
            }
        }
        if ($yExtreme != 0) {
            $yExpectedEmpty = $yExtreme > 0 ? $yExtreme - $gridSize : $yExtreme + $gridSize;
            for ($x = 1 - $gridSize; $x < $gridSize; $x++) {
                if ($kingdom[$x][$yExpectedEmpty]) {
                    return false;
                }
            }
        }
        return true;
    }

    function dominoCanBePlaced($kingdom, $domino)
    {
        $gridSize = $this->isMightyDuel() ? 7 : 5;
        // Let's loop in spirals from the castle, for fun and performances!
        $distanceFromCastle = 1;
        while ($distanceFromCastle < $gridSize) {
            $x = $distanceFromCastle;
            for ($y = $distanceFromCastle; $y > -$distanceFromCastle; $y--) {
                if ($this->dominoFits($kingdom, $domino, $x, $y)) {
                    return true;
                }
            }
            for ($x = $distanceFromCastle; $x > -$distanceFromCastle; $x--) {
                if ($this->dominoFits($kingdom, $domino, $x, $y)) {
                    return true;
                }
            }
            for ($y = -$distanceFromCastle; $y < $distanceFromCastle; $y++) {
                if ($this->dominoFits($kingdom, $domino, $x, $y)) {
                    return true;
                }
            }
            for ($x = -$distanceFromCastle; $x < $distanceFromCastle; $x++) {
                if ($this->dominoFits($kingdom, $domino, $x, $y)) {
                    return true;
                }
            }
            $distanceFromCastle++;
        }
        return false;
    }

    function dominoFits($kingdom, $domino, $x, $y)
    {
        return !$kingdom[$x][$y] && (
                $this->dominoFitsInPosition($kingdom, $domino, array('x' => $x, 'y' => $y, 'rotation' => 0))
                || $this->dominoFitsInPosition($kingdom, $domino, array('x' => $x, 'y' => $y, 'rotation' => 1))
                || $this->dominoFitsInPosition($kingdom, $domino, array('x' => $x, 'y' => $y, 'rotation' => 2))
                || $this->dominoFitsInPosition($kingdom, $domino, array('x' => $x, 'y' => $y, 'rotation' => 3))
            );
    }

    function dominoFitsInPosition($kingdom, $domino, $position)
    {
        return !$this->positionIsOccupied($kingdom, $position)
            && $this->dominoConnects($kingdom, $domino, $position)
            && $this->positionFitsInGrid($kingdom, $position);
    }

    function isLastTurn()
    {
        return self::getUniqueValueFromDB("SELECT count(*) FROM dominoes WHERE location = 'FUTURE'") == 0;
    }

    //////////////////////////////////////////////////////////////////////////////
    //////////// Player actions
    ////////////
    /*
     * Each time a player is doing some game action, one of the methods below is called.
     * (note: each method below must match an input method in kingdomino.action.php)
     */
    function chooseDomino($number)
    {
        $this->checkAction('chooseDomino');
        $player_id = $this->getActivePlayerId();
        if (self::getUniqueValueFromDB("SELECT location FROM dominoes WHERE number = $number AND owner_player IS NULL") != 'FUTURE') {
            throw new BgaUserException(self::_("Please choose a free domino among those displayed"));
        }
        self::DbQuery("UPDATE dominoes SET owner_player = $player_id WHERE number = $number AND location = 'FUTURE'");
        self::notifyAllPlayers('dominoChosen', clienttranslate('${player_name} chooses domino ${number}'),
            array('playerId' => $player_id, 'player_name' => self::getActivePlayerName(), 'number' => $number));
        $this->gamestate->nextState('');
    }

    function placeDomino($position, $nextDomino = null)
    {
        $player_id = $this->getActivePlayerId();
        $score = $this->getScoreIfDominoPlaced($position);
        $number = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");

        $x = $position['x'];
        $y = $position['y'];
        $rotation = $position['rotation'];
        self::DbQuery("UPDATE dominoes SET location = 'KINGDOM', rotation = $rotation, horizontal_position = $x, vertical_position = $y WHERE number = $number");

        self::DbQuery("UPDATE player SET player_score = $score WHERE player_id='$player_id'");
        self::notifyAllPlayers('dominoPlaced', clienttranslate('${player_name} places domino ${number} in his kingdom'),
            array('playerId' => $player_id, 'player_name' => self::getActivePlayerName(), 'number' => $number, 'position' => $position, 'score' => $score));
        $this->giveExtraTime($player_id);

        if ($this->isLostTreasures()) {
            $kingdom = $this->getKingdomView($player_id);
            $placedTiles = $this->getDominoTiles($number);
            $squares = $this->findNewTreasureSquares($kingdom, $placedTiles, $this->getOccupiedIntersections($player_id));
            $supplyAvailable = (int) $this->tokens->countTokensInLocation('supply') > 0;
            if (count($squares) > 0 && $supplyAvailable) {
                self::setGameStateValue('lt_pending_domino', $number);
                self::setGameStateValue('lt_next_domino', $nextDomino == null ? 0 : $nextDomino);
                $this->gamestate->nextState('placeTreasure');
                return;
            }
        }

        $this->routeAfterPlacement($nextDomino);
    }

    // Base routing after a domino is fully resolved (also replays a deferred next domino choice).
    function routeAfterPlacement($nextDomino = null)
    {
        if (!$this->isLastTurn()) {
            $this->gamestate->nextState('chooseDomino');
            if ($nextDomino) {
                $this->chooseDomino($nextDomino);
            }
        } else {
            $this->gamestate->nextState('nextPlayer');
        }
    }

    function getScoreIfDominoPlaced($position)
    {
        return $this->getKingdomScoreWithGems($this->getActivePlayerId(), $this->simulateDominoPlacement($position));
    }

    // Validate the staged placement and return the resulting simulated kingdom.
    function simulateDominoPlacement($position)
    {
        $this->checkAction('placeDomino');
        $number = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
        $domino = $this->dominoes[$number];
        $kingdom = $this->getKingdomView($this->getActivePlayerId());

        if ($this->positionIsOccupied($kingdom, $position)) {
            throw new BgaVisibleSystemException("You cannot place a domino on top of another");
        }
        if (!$this->dominoConnects($kingdom, $domino, $position)) {
            throw new BgaUserException(clienttranslate("The domino must be adjacent to your Castle or a terrain of the same type"));
        }
        if (!$this->positionFitsInGrid($kingdom, $position)) {
            throw new BgaVisibleSystemException("Your kingdom must fit in the grid");
        }

        $this->addToKingdomView($kingdom, $number, $position);
        return $kingdom;
    }

    // Precompute every valid placement of the current domino with its resulting score
    // and treasure-discovery flag, so the client reads both from state args instead of
    // asking the server on each drag/rotate (and on Validate). One entry per valid
    // (x, y, rotation): { x, y, rotation, score, discoversTreasure }.
    function getPlacementPreviews($player_id)
    {
        $number = (int) self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
        if ($number <= 0) {
            return array();
        }
        $domino = $this->dominoes[$number];
        $kingdom = $this->getKingdomView($player_id);
        $checkTreasures = $this->isLostTreasures() && (int) $this->tokens->countTokensInLocation('supply') > 0;
        $occupied = $checkTreasures ? $this->getOccupiedIntersections($player_id) : array();
        $gridSize = $this->isMightyDuel() ? 7 : 5;
        $previews = array();
        for ($x = -$gridSize + 1; $x < $gridSize; $x++) {
            for ($y = -$gridSize + 1; $y < $gridSize; $y++) {
                for ($rotation = 0; $rotation < 4; $rotation++) {
                    $position = array('x' => $x, 'y' => $y, 'rotation' => $rotation);
                    if ($this->positionIsOccupied($kingdom, $position)
                        || !$this->dominoConnects($kingdom, $domino, $position)
                        || !$this->positionFitsInGrid($kingdom, $position)) {
                        continue;
                    }
                    $simulatedKingdom = $kingdom;
                    $this->addToKingdomView($simulatedKingdom, $number, $position);
                    $discovers = false;
                    if ($checkTreasures) {
                        $right = $this->getRightTerrainCoordinates($position);
                        $placedTiles = array(
                            array('x' => $x, 'y' => $y),
                            array('x' => $right['x'], 'y' => $right['y']),
                        );
                        $discovers = count($this->findNewTreasureSquares($simulatedKingdom, $placedTiles, $occupied)) >= 1;
                    }
                    $previews[] = array(
                        'x' => $x,
                        'y' => $y,
                        'rotation' => $rotation,
                        'score' => $this->getKingdomScoreWithGems($player_id, $simulatedKingdom),
                        'discoversTreasure' => $discovers,
                    );
                }
            }
        }
        return $previews;
    }

    function discardDomino()
    {
        $this->checkAction('discardDomino');
        $number = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
        $domino = $this->dominoes[$number];
        $kingdom = $this->getKingdomView($this->getActivePlayerId());
        if ($this->dominoCanBePlaced($kingdom, $domino)) {
            throw new BgaVisibleSystemException("You cannot discard a domino if it can be added to your kingdom");
        }
        $this->doDiscardDomino($number);
    }

    function doDiscardDomino($number)
    {
        $playerId = self::getActivePlayerId();
        self::DbQuery("UPDATE dominoes SET location = 'DISCARD' WHERE `number` = $number");
        self::notifyAllPlayers('dominoDiscarded', clienttranslate('${player_name} cannot place domino ${number} in his kingdom, it is discarded'),
            array('playerId' => $playerId, 'player_name' => self::getActivePlayerName(), 'number' => $number));
        self::incStat(1, 'discarded_dominoes', $playerId);
        if (!$this->isLastTurn()) {
            $this->gamestate->nextState('chooseDomino');
        } else {
            $this->gamestate->nextState('nextPlayer');
        }
    }

    function placeTreasure($tokenKey, $x, $y, $rotation)
    {
        $this->checkAction('placeTreasure');
        $player_id = $this->getActivePlayerId();

        if (!isset($this->treasures[$tokenKey]) || $this->tokens->getTokenLocation($tokenKey) !== 'supply') {
            throw new BgaUserException(self::_("Please choose an available gem"));
        }
        $rotation = (int) $rotation;
        if ($rotation < 0 || $rotation > 3) {
            throw new BgaVisibleSystemException("Invalid treasure rotation");
        }
        $pending = $this->getPendingTreasureSquares($player_id);
        $isValid = false;
        foreach ($pending as $cell) {
            if ($cell['x'] == $x && $cell['y'] == $y) {
                $isValid = true;
                break;
            }
        }
        if (!$isValid) {
            throw new BgaUserException(self::_("You cannot place a gem here"));
        }

        $this->doPlaceTreasure($player_id, $tokenKey, (int) $x, (int) $y, $rotation);
    }

    function doPlaceTreasure($player_id, $tokenKey, $x, $y, $rotation)
    {
        $this->tokens->moveToken($tokenKey, $this->treasureLocation($player_id), $this->encodeTokenPos($x, $y, $rotation));
        $type = $this->treasures[$tokenKey]['type'];
        $score = $this->getKingdomScoreWithGems($player_id, $this->getKingdomView($player_id));
        self::DbQuery("UPDATE player SET player_score = $score WHERE player_id='$player_id'");
        self::notifyAllPlayers('treasurePlaced', clienttranslate('${player_name} discovers a treasure'),
            array('playerId' => $player_id, 'player_name' => self::getActivePlayerName(), 'tokenKey' => $tokenKey, 'type' => $type, 'x' => $x, 'y' => $y, 'rotation' => $rotation, 'score' => $score));

        $revealed = $this->tokens->getTokenOnBottom('box');
        if ($revealed != null) {
            $this->tokens->moveToken($revealed['key'], 'supply', (int) $revealed['state']);
            self::notifyAllPlayers('treasureRevealed', '',
                array('tokenKey' => $revealed['key'], 'type' => $this->treasures[$revealed['key']]['type'], 'boxCount' => (int) $this->tokens->countTokensInLocation('box')));
        } else {
            self::notifyAllPlayers('treasureRevealed', '', array('tokenKey' => null, 'type' => null, 'boxCount' => 0));
        }

        if ($this->isLostTreasures() && $this->hasAllTreasureTypes($player_id)) {
            $this->declareImmediateVictory($player_id);
            return;
        }

        if (count($this->getPendingTreasureSquares($player_id)) > 0 && (int) $this->tokens->countTokensInLocation('supply') > 0) {
            $this->gamestate->nextState('placeTreasure');
            return;
        }

        $nextDomino = (int) self::getGameStateValue('lt_next_domino');
        self::setGameStateValue('lt_pending_domino', 0);
        self::setGameStateValue('lt_next_domino', 0);
        $this->routeAfterPlacement($nextDomino == 0 ? null : $nextDomino);
    }

    // Jokers fill missing colors: distinct real colors + joker count reaching 5 covers all types.
    function hasAllTreasureTypes($player_id)
    {
        $gems = $this->tokens->getTokensOfTypeInLocation('treasure', $this->treasureLocation($player_id), null, null);
        $distinctColors = array();
        $jokers = 0;
        foreach (array_keys($gems) as $key) {
            $type = $this->treasures[$key]['type'];
            if ($type === 'joker') {
                $jokers++;
            } else {
                $distinctColors[$type] = true;
            }
        }
        return count($distinctColors) + $jokers >= 5;
    }

    function declareImmediateVictory($player_id)
    {
        // Immediate victory ignores points: winner gets 1, everyone else 0. finalScoring
        // is skipped, so the per-terrain/variant score stats stay unset (excluded from
        // analysis). Record the win both per-player (the winner) and table-wide.
        self::DbQuery("UPDATE player SET player_score = 0, player_score_aux = 0");
        self::DbQuery("UPDATE player SET player_score = 1 WHERE player_id = '$player_id'");
        self::setStat(1, 'immediate_victory', $player_id);
        self::setStat(1, 'immediate_victory');
        self::notifyAllPlayers('immediateVictory', clienttranslate('${player_name} collects all treasure types and wins immediately!'),
            array('playerId' => $player_id, 'player_name' => self::getActivePlayerName()));
        $this->gamestate->nextState('gameEnd');
    }

    //////////////////////////////////////////////////////////////////////////////
    //////////// Game state arguments
    ////////////
    /*
     * Here, you can create methods defined as "game state arguments" (see "args" property in states.inc.php).
     * These methods function is to return some additional information that is specific to the current
     * game state.
     */
    function argsPlaceDomino()
    {
        $kingdom = $this->getKingdomView($this->getActivePlayerId());
        $gridSize = $this->isMightyDuel() ? 7 : 5;
        $xMin = $yMin = 1 - $gridSize;
        $xMax = $yMax = $gridSize - 1;
        for ($x = -$gridSize + 1; $x < $gridSize; $x++) {
            for ($y = -$gridSize + 1; $y < $gridSize; $y++) {
                if ($kingdom[$x][$y]) {
                    $xMin = max($xMin, $x - $gridSize + 1);
                    $xMax = min($xMax, $x + $gridSize - 1);
                    $yMin = max($yMin, $y - $gridSize + 1);
                    $yMax = min($yMax, $y + $gridSize - 1);
                }
            }
        }
        return array(
            'domino' => self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1"),
            'currentPosition' => self::getUniqueValueFromDB("SELECT 1 + count(*) FROM dominoes WHERE location = 'FUTURE' AND owner_player IS NOT NULL"),
            'kingdom' => $kingdom,
            'placementPreviews' => $this->getPlacementPreviews($this->getActivePlayerId()),
            'xMin' => $xMin,
            'xMax' => $xMax,
            'yMin' => $yMin,
            'yMax' => $yMax
        );
    }

    function argsPlaceTreasure()
    {
        $player_id = $this->getActivePlayerId();
        $supplyTokens = $this->tokens->getTokensOfTypeInLocation('treasure', 'supply', null, 'token_state');
        $supply = array();
        foreach (array_keys($supplyTokens) as $key) {
            $supply[] = array('key' => $key, 'type' => $this->treasures[$key]['type']);
        }
        $faces = array();
        foreach ($supply as $gem) {
            $faces[$gem['key']] = $this->treasures[$gem['key']]['faces'];
        }
        return array(
            'supply' => $supply,
            'intersections' => $this->getPendingTreasureSquares($player_id),
            'faces' => $faces,
        );
    }

    //////////// --- Game state arguments generated end ---
    //////////// Game state actions
    ////////////
    /*
     * Here, you can create methods defined as "game state actions" (see "action" property in states.inc.php).
     * The action method of state X is called everytime the current game state is set to X.
     */
    function activateNextPlayer()
    {
        if (self::getGameStateValue('turn_number') == 1 && $this->remainsFutureDomino()) {
            $this->activateOwnerOfNextKing();
        } else {
            $this->activateOwnerOfNextCurrentDomino();
        }
    }

    private function remainsFutureDomino()
    {
        $numPlayers = count($this->loadPlayersBasicInfos());
        $remainingDominoes = self::getUniqueValueFromDB("SELECT count(*) FROM dominoes WHERE location = 'FUTURE' AND owner_player IS NULL");
        if ($numPlayers == 3 && self::getGameStateValue('3_players_v2') == 1) {
            //  After all 3 players have selected a domino, discard the leftover domino into the box every round.
            return $remainingDominoes > 1;
        } else {
            return $remainingDominoes > 0;
        }
    }

    private function activateOwnerOfNextKing()
    {
        // Currently for 2 players game a balanced setup is done: first player gets first and last domino choices.
        $numPlayers = count($this->loadPlayersBasicInfos());
        if ($numPlayers != 2 || self::getUniqueValueFromDB("SELECT count(*) FROM dominoes WHERE owner_player IS NOT NULL") != 2) {
            $this->activeNextPlayer(); // Just follow the random order generated around the table
        }
        $this->gamestate->nextState('chooseDomino');
    }

    private function activateOwnerOfNextCurrentDomino()
    {
        $nextPlayer = $this->getOwnerOfNextCurrentDomino();
        if ($nextPlayer == null) {
            if ($this->isLastTurn()) {
                $this->finalScoring();
                $this->gamestate->nextState('gameEnd');
                return;
            } else {
                $numPlayers = count($this->loadPlayersBasicInfos());
                $this->drawDominoes($numPlayers);
                $nextPlayer = $this->getOwnerOfNextCurrentDomino();
            }
        }
        $this->gamestate->changeActivePlayer($nextPlayer);
        $this->gamestate->nextState('placeDomino');
    }

    private function getOwnerOfNextCurrentDomino()
    {
        return self::getUniqueValueFromDB("SELECT owner_player FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
    }

    function stPlaceDomino()
    {
        // In asynchronous games, if the domino must be discarded on last turn it is done automatically to fasten the game.
        if (self::isAsync() && $this->isLastTurn()) {
            $number = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
            $domino = $this->dominoes[$number];
            $kingdom = $this->getKingdomView($this->getActivePlayerId());
            if (!$this->dominoCanBePlaced($kingdom, $domino)) {
                $this->doDiscardDomino($number);
            }
        }
    }

    function finalScoring()
    {
        $players = $this->loadPlayersBasicInfos();


        foreach ($players as $player_id => $player) {
            // Baseline this player's per-terrain/bonus score stats so they are recorded
            // for normally-ended games (inc below adds to them). Immediate-victory
            // games skip finalScoring, so these stay unset and are excluded from analysis.
            foreach (['lake', 'forest', 'field', 'grassland', 'swamp', 'mountain', 'the_middle_kingdom', 'harmony'] as $statBase) {
                $this->bga->playerStats->set($statBase . '_score', 0, $player_id);
            }

            $this->bga->notify->all('startPlayerScoring', '', array('playerId' => $player_id, 'player_name' => $player['player_name']));

            $kingdom = $this->getKingdomView($player_id);
            $scoringTerritories = ['forest' => [], 'field' => [], 'grassland' => [], 'lake' => [], 'swamp' => [], 'mountain' => []];
            $biggestTerritory = 0;
            $totalCrowns = 0;
            $territoryScore = 0;
            foreach ($this->getScoringTerritories($player_id, $kingdom) as $territory) {
                // The center Castle is its own territory but has no '_score' stat (and always scores 0).
                if (isset($this->terrains[$territory['terrain']])) {
                    $this->bga->playerStats->inc($territory['terrain'] . '_score', $territory['score'], $player_id);
                }
                $biggestTerritory = max($biggestTerritory, $territory['size']);
                $territoryScore += $territory['score'];
                if ($territory['score'] > 0) {
                    $scoringTerritories[$territory['terrain']][] = $territory;
                    $totalCrowns += $territory['crowns'] + $territory['gemCrowns'];
                }
            }
            $this->DbQuery("UPDATE player SET player_score = $territoryScore WHERE player_id='$player_id'");
            $tieBreaker = $biggestTerritory * 100 + $totalCrowns;
            $this->DbQuery("UPDATE player SET player_score_aux = $tieBreaker WHERE player_id='$player_id'");
            foreach ($this->terrains as $terrain => $terrainName) {
                $scoreForTerrain = $this->bga->playerStats->get($terrain . '_score', $player_id);
                foreach ($scoringTerritories[$terrain] as $territory) {
                    $this->bga->notify->all('scoreTerritory', '',
                        array('playerId' => $player_id, 'player_name' => $player['player_name'], 'territory' => $territory, 'totalScoreForTerrain' => $scoreForTerrain, 'terrain' => $terrainName));
                }
                // Log a line per terrain that actually scored (skip the 0-point terrains).
                $terrainMessage = $scoreForTerrain > 0 ? clienttranslate('${player_name} scores ${score} for ${terrain_name}') : '';
                $this->bga->notify->all('scoreTerrain', $terrainMessage,
                    array('i18n' => array('terrain_name'), 'playerId' => $player_id, 'player_name' => $player['player_name'], 'terrain' => $terrain, 'terrain_name' => $terrainName, 'score' => $scoreForTerrain));
            }

            $displayBonus = false;
            $bonus = 0;
            if ($this->getGameStateValue('the_middle_kingdom') == 1) {
                $displayBonus = true;
                $minX = $minY = $maxX = $maxY = 0;
                $kingdom = $this->getKingdomView($player_id);
                for ($x = -7; $x <= 7; $x++) {
                    for ($y = -7; $y <= 7; $y++) {
                        if ($kingdom[$x][$y] != null) {
                            $minX = min($minX, $x);
                            $minY = min($minY, $y);
                            $maxX = max($maxX, $x);
                            $maxY = max($maxY, $y);
                        }
                    }
                }
                if (-$minX == $maxX && -$minY == $maxY) {
                    $this->DbQuery("UPDATE player SET player_score = player_score + 10 WHERE player_id='$player_id'");
                    $this->bga->notify->all('score', clienttranslate('The middle Kingdom: ${player_name} scores 10 additional points for having his castle in the center of his territory'),
                        array('playerId' => $player_id, 'player_name' => $player['player_name'], 'score' => 10));
                    $this->bga->playerStats->inc('the_middle_kingdom_score', 10, $player_id);
                    $bonus += 10;
                }
            }
            if ($this->getGameStateValue('harmony') == 1) {
                $displayBonus = true;
                if ($this->getUniqueValueFromDB("SELECT 1 FROM dominoes WHERE location = 'DISCARD' AND owner_player = $player_id LIMIT 1") == null) {
                    $this->DbQuery("UPDATE player SET player_score = player_score + 5 WHERE player_id='$player_id'");
                    $this->bga->notify->all('score', clienttranslate('Harmony: ${player_name} scores 5 additional points for his complete territory'),
                        array('playerId' => $player_id, 'player_name' => $player['player_name'], 'score' => 5));
                    $this->bga->playerStats->inc('harmony_score', 5, $player_id);
                    $bonus += 5;
                }
            }

            $score = $this->getUniqueValueFromDB("SELECT player_score FROM player WHERE player_id='$player_id'");
            $this->bga->notify->all('endPlayerScoring', clienttranslate('${player_name} scores ${score} points in total'),
                array('playerId' => $player_id, 'player_name' => $player['player_name'], 'score' => $score, 'bonus' => $bonus, 'displayBonus' => $displayBonus));
        }
    }

    //////////// --- Game state actions generated end ---
    //////////////////////////////////////////////////////////////////////////////
    //////////// Zombie
    ////////////
    /*
     * zombieTurn:
     *
     * This method is called each time it is the turn of a player who has quit the game (= "zombie" player).
     * You can do whatever you want in order to make sure the turn of this player ends appropriately
     * (ex: pass).
     */
    function zombieTurn($state, $active_player)
    {
        switch ($state['name']) {
            case 'chooseDomino':
                $domino = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'FUTURE' AND owner_player IS NULL LIMIT 1");
                $this->chooseDomino($domino);
                break;
            case 'placeDomino':
                $domino = self::getUniqueValueFromDB("SELECT number FROM dominoes WHERE location = 'CURRENT' ORDER BY number LIMIT 1");
                $this->doDiscardDomino($domino);
                break;
            case 'placeTreasure':
                // Zombie skips the discovered treasure (gem stays in supply) and resumes flow.
                $nextDomino = (int) self::getGameStateValue('lt_next_domino');
                self::setGameStateValue('lt_pending_domino', 0);
                self::setGameStateValue('lt_next_domino', 0);
                $this->routeAfterPlacement($nextDomino == 0 ? null : $nextDomino);
                break;
        }
    }

    ///////////////////////////////////////////////////////////////////////////////////:
    ////////// DB upgrade
    //////////
    /*
     * upgradeTableDb:
     *
     * You don't have to care about this until your game has been published on BGA.
     * Once your game is on BGA, this method is called everytime the system detects a game running with your old
     * Database scheme.
     * In this case, if you change your Database scheme, you just have to apply the needed changes in order to
     * update the game database and allow the game to continue to run with your new version.
     *
     */
    function upgradeTableDb($from_version)
    {
        // $from_version is the current version of this game database, in numerical form.
        // For example, if the game was running with a release of your game named "140430-1345",
        // $from_version is equal to 1404301345
        // Example:
        //        if( $from_version <= 1404301345 )
        //        {
        //            $sql = "ALTER TABLE xxxxxxx ....";
        //            self::DbQuery( $sql );
        //        }
        //        if( $from_version <= 1405061421 )
        //        {
        //            $sql = "CREATE TABLE xxxxxxx ....";
        //            self::DbQuery( $sql );
        //        }
        //        // Please add your future database scheme changes here
        //
        //
    }
}
