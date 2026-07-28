<?php
/**
 *------
 * BGA framework: Gregory Isabelli & Emmanuel Colin & BoardGameArena
 * Scholars implementation : © Alena Laskavaia <laskava@gmail.com> - aka Victoria_La
 *
 * This code has been produced on the BGA studio platform for use on http://boardgamearena.com.
 * See http://en.boardgamearena.com/#!doc/Studio for more information.
 * -----
 *
 */

/*
 * This is a generic class to manage game pieces.
 *
 * On DB side this is based on a standard table with the following fields:
 * token_key (string), token_location (string), token_state (int)
 *
 *
 * CREATE TABLE IF NOT EXISTS `token` (
 * `token_key` varchar(32) NOT NULL,
 * `token_location` varchar(32) NOT NULL,
 * `token_state` int(10),
 * PRIMARY KEY (`token_key`)
 * ) ENGINE=InnoDB DEFAULT CHARSET=utf8;
 *
 *
 */
namespace Bga\Games\Kingdomino\Db;

use Bga\GameFramework\SystemException;

// array_get / toJson back the unused game-level helper methods (we only use the
// generic CRUD here); they are Scholars helpers, so provide local stand-ins.
function array_get(array $array, string $key, mixed $default = null): mixed {
    return $array[$key] ?? $default;
}

function toJson(mixed $value): string {
    return json_encode($value);
}

class DbTokens {
    var string $table = "token";
    var bool $autoreshuffle = false; // If true, a new deck is automatically formed with a reshuffled discard as soon at is needed
    var ?array $autoreshuffle_trigger = null; // Callback on autoreshuffle: ["obj" => object, "method" => method_name]
    // autoreshuffle_trigger = array( 'obj' => object, 'method' => method_name )
    // If defined, tell the name of the deck and what is the corresponding discard (ex : "mydeck" => "mydiscard")
    var array $autoreshuffle_custom = [];

    protected array $keyindex = []; // cache

    public object $game; // game ref (the Table-derived game object)
    function __construct(object $game) {
        $this->game = $game;
        $this->table = "token";
        $this->autoreshuffle_trigger = ["obj" => $this, "method" => "autoreshuffleHandler"];
    }

    // MUST be called before any other method if db table is not called 'token'
    function init(string $table): void {
        $this->table = $table;
    }

    function clear_cache(): void {
        $this->keyindex = [];
    }

    function init_cache(): void {
        if (count($this->keyindex) == 0) {
            $this->keyindex = $this->getTokensOfTypeInLocation(null);
        }
    }

    // This inserts new records in the database. Generically speaking you should only be calling during setup with some
    // rare exceptions.
    //
    // Token records are added into location specified, (default is 'deck')
    //
    // $tokens is an array with at least the following fields:
    // array(
    //      array(                              // This is my first token
    //          "key" => <unique key>           // This unique alphanum and underscore key, use {INDEX} to replace with index if 'nbr' > 1, i..e "meeple_{INDEX}_red"
    //          "nbr" => <nbr>                  // Number of tokens with this key, default is 1. If nbr >1 and key does not have {INDEX} it will throw an exception
    //          "location" => <location>        // Optional argument specifies the location, alphanum and underscore
    //          "state" => <state>              // Optional argument specifies integer state, if not specified and $token_state_global is not specified auto-increment is used
    function createTokens(array $tokens, ?string $location_global, ?int $token_state_global = null): array {
        if ($location_global) {
            $next_pos = $this->getExtremePosition(true, $location_global) + 1;
        } else {
            $next_pos = 0;
        }
        $values = [];
        $keys = [];
        foreach ($tokens as $token_info) {
            if (isset($token_info["nbr"])) {
                $n = $token_info["nbr"];
            } else {
                $n = 1;
            }
            if (isset($token_info["nbr_start"])) {
                $start = $token_info["nbr_start"];
            } else {
                $start = 0;
            }
            for ($i = $start; $i < $n + $start; $i++) {
                if (isset($token_info["location"])) {
                    $location = $token_info["location"];
                } else {
                    $location = $location_global;
                }
                if (isset($token_info["state"])) {
                    $token_state = (int) $token_info["state"];
                } else {
                    $token_state = $token_state_global;
                }
                if ($token_state === null) {
                    if ($location == $location_global) {
                        $token_state = $next_pos;
                        $next_pos++;
                    } else {
                        $token_state = 0;
                    }
                }
                $key = $token_info["key"];
                if ($key == null) {
                    throw new SystemException("createTokens: key cannot be null");
                }
                $key = $this->varsub($key, array_merge($token_info, ["INDEX" => $i]));
                if ($location == null) {
                    throw new SystemException("createTokens: location cannot be null (set per token location or location_global");
                }
                self::checkLocation($location);
                self::checkKey($key);
                $values[] = [$key, $location, $token_state];
                $keys[] = $key;
            }
        }
        $this->DbCreateTokens($values);
        return $keys;
    }

    function createTokenIfNot(string $key, string $location = "limbo", int $token_state = 0): string {
        if ($this->getTokenInfo($key)) {
            $this->moveToken($key, $location, $token_state);
            return $key;
        } else {
            return $this->createToken($key, $location, $token_state);
        }
    }
    function createToken(string $key, string $location = "limbo", int $token_state = 0): string {
        self::checkLocation($location);
        self::checkState($token_state);
        self::checkKey($key);
        $values = [];
        $values[] = [$key, $location, $token_state];
        $this->DbCreateTokens($values);
        return $key;
    }

    function DbCreateTokens(array $values): void {
        $this->clear_cache();
        $seqvalues = [];
        foreach ($values as $row) {
            $seqvalues[] = "( '$row[0]', '$row[1]', '$row[2]' )";
        }
        $sql = "INSERT INTO " . $this->table . " (token_key,token_location,token_state)";
        $sql .= " VALUES " . implode(",", $seqvalues);
        $this->game->DbQuery($sql);
    }

    /**
     * Create tokens during the game (dynamic piles) with auto increment number
     * token must be in form of "prefix_index" where prefix is passed to this function, and index is generated as max
     * number of tokens starting prefix pre-existing in db
     *
     * @return string - token key
     */
    function createTokenAutoInc(string $type, string $location = "limbo", int $token_state = 0, int $start = 0): string {
        $this->clear_cache();
        $allsuf = $this->getTokensOfTypeInLocation($type);
        $resnum = count($allsuf) + $start;
        return $this->createToken("{$type}_{$resnum}", $location, $token_state);
    }

    function createTokensPack(string $key, string $location, int $nbr = 1, int $nbr_start = 1, ?array $iterArr = null, ?int $token_state = null): array {
        $this->clear_cache();
        // null or empty array (loosely == null) both default to a single empty iteration
        if ($iterArr == null) {
            $iterArr = [""];
        }
        $tokenSpec = ["key" => $key, "location" => $location, "nbr" => $nbr, "nbr_start" => $nbr_start];
        $tokens = [];
        foreach ($iterArr as $iterKey) {
            $newspec = [];
            foreach ($tokenSpec as $tokenSpecKey => $value) {
                $value = $this->varsub($value, ["TYPE" => $iterKey]);
                $value = $this->varsub($value, ["COLOR" => $iterKey]);
                $newspec[$tokenSpecKey] = $value;
            }
            $tokens[] = $newspec;
        }
        return $this->createTokens($tokens, null, $token_state);
    }

    // Get max on min state on the specific location
    function getExtremePosition(bool $getMax, string $location, ?string $token_key = null): mixed {
        self::checkLocation($location, true);
        if ($getMax) {
            $sql = "SELECT MAX( token_state ) res ";
        } else {
            $sql = "SELECT MIN( token_state ) res ";
        }
        $sql .= "FROM " . $this->table;
        $like = "LIKE";
        if (strpos($location, "%") === false) {
            $like = "=";
        } else {
            $location = preg_replace("/_/", "\\_", $location);
        }
        $sql .= " WHERE token_location $like '$location' ";
        if ($token_key != null) {
            self::checkKey($token_key, true);
            $like = "LIKE";
            if (strpos($token_key, "%") === false) {
                $like = "=";
            } else {
                $token_key = preg_replace("/_/", "\\_", $token_key);
            }
            $sql .= " AND token_key $like '$token_key' ";
        }
        return $this->game->getUniqueValueFromDB($sql);
    }

    // Shuffle token of a specified location, result of the operation will changes state of the token to be a position after shuffling
    function shuffle(string $location): void {
        self::checkLocation($location);
        $this->clear_cache();
        $token_keys = $this->game->getObjectListFromDB("SELECT token_key FROM " . $this->table . " WHERE token_location='$location'", true);
        shuffle($token_keys);
        $n = 0;
        foreach ($token_keys as $token_key) {
            $this->game->DbQuery("UPDATE " . $this->table . " SET token_state='$n' WHERE token_key='$token_key'");
            $n++;
        }
    }

    function deleteAll(): void {
        $this->game->DbQuery("DELETE FROM " . $this->table);
        $this->clear_cache();
    }

    // Pick the first "$nbr" cards on top of specified deck and place it in target location
    // Return cards infos or void array if no card in the specified location
    // Warning: this does not send client notification, the reveal has to follow
    function pickTokensForLocation(int $nbr, string $from_location, string $to_location, int $state = 0, bool $no_deck_reform = false, ?bool &$was_reshuffled = null): array {
        $this->clear_cache();
        $tokens = self::getTokensOnTop($nbr, $from_location);
        $tokens_ids = [];
        foreach ($tokens as $i => $card) {
            $tokens_ids[] = $card["key"];
            $tokens[$i]["location"] = $to_location;
            $tokens[$i]["state"] = $state;
        }
        $sql = "UPDATE " . $this->table . " SET token_location='" . addslashes($to_location) . "', token_state='$state' ";
        $sql .= "WHERE token_key IN ('" . implode("','", $tokens_ids) . "') ";
        $this->game->DbQuery($sql);
        if (isset($this->autoreshuffle_custom[$from_location]) && count($tokens) < $nbr && $this->autoreshuffle && !$no_deck_reform) {
            // No more cards in deck & reshuffle is active => form another deck
            if ($this->countTokensInLocation($this->autoreshuffle_custom[$from_location]) == 0) {
                return $tokens;
            }
            $nbr_token_missing = $nbr - count($tokens);
            self::reformDeckFromDiscard($from_location);
            $newcards = self::pickTokensForLocation($nbr_token_missing, $from_location, $to_location, $state, true); // Note: block anothr deck reform
            foreach ($newcards as $card) {
                $tokens[] = $card;
            }
            $was_reshuffled = true;
        }
        return $tokens;
    }

    /**
     * Return token on top of this location, top defined as item with higher state value
     */
    function getTokenOnTop(string $location, bool $no_deck_reform = true): ?array {
        $result_arr = $this->getTokensOnTop(1, $location);
        if (count($result_arr) > 0) {
            return $result_arr[0];
        }
        if (isset($this->autoreshuffle_custom[$location]) && $this->autoreshuffle && !$no_deck_reform) {
            // No more cards in deck & reshuffle is active => form another deck
            self::reformDeckFromDiscard($location);
            $result_arr = $this->getTokensOnTop(1, $location);
            if (count($result_arr) > 0) {
                return $result_arr[0];
            }
        }
        return null;
    }

    /**
     * Return "$nbr" tokens on top of this location, top defined as item with higher state value
     */
    function getTokensOnTop(int $nbr, string $location): array {
        self::checkLocation($location);
        self::checkPosInt($nbr);
        $sql = $this->getSelectQuery();
        $sql .= " WHERE token_location='$location'";
        $sql .= " ORDER BY token_state DESC";
        $sql .= " LIMIT $nbr";
        return $this->game->getObjectListFromDB($sql);
    }

    /** Bottom card of a deck (lowest state); null if empty. Used by Rest's bottom-scroll reveal. */
    function getTokenOnBottom(string $location): ?array {
        self::checkLocation($location);
        $sql = $this->getSelectQuery();
        $sql .= " WHERE token_location='$location'";
        $sql .= " ORDER BY token_state ASC";
        $sql .= " LIMIT 1";
        $rows = $this->game->getObjectListFromDB($sql);
        return $rows[0] ?? null;
    }

    function reformDeckFromDiscard(string $from_location, ?string $discard_location = null): void {
        $this->checkLocation($from_location);
        $this->clear_cache();
        if (isset($this->autoreshuffle_custom[$from_location])) {
            $discard_location = $this->autoreshuffle_custom[$from_location];
        } elseif (!$discard_location) {
            throw new SystemException("reformDeckFromDiscard: Unknown discard location for $from_location !");
        }
        $this->checkLocation($discard_location);
        $this->moveAllTokensInLocation($discard_location, $from_location);
        $this->shuffle($from_location);
        if ($this->autoreshuffle_trigger) {
            $obj = $this->autoreshuffle_trigger["obj"];
            $method = $this->autoreshuffle_trigger["method"];
            $obj->$method($from_location, $discard_location);
        }
    }

    // Set token state
    function setTokenState(string $token_key, int|string $state): int|string {
        try {
            self::checkState($state);
        } catch (\Exception $e) {
            throw new SystemException($e->getMessage() . " for $token_key");
        }
        self::checkKey($token_key);
        $this->clear_cache();
        $sql = "UPDATE " . $this->table;
        $sql .= " SET token_state='$state'";
        $sql .= " WHERE token_key='$token_key'";
        $this->game->DbQuery($sql);
        return $state;
    }

    /**
     * Move a token to a specific location and state (state defaults to 0).
     */
    function moveToken(string $token_key, string $location, int $state = 0): null|\mysqli_result|bool {
        self::checkLocation($location);
        self::checkState($state, true);
        self::checkKey($token_key);
        $this->clear_cache();
        $sql = "UPDATE " . $this->table;
        $sql .= " SET token_location='$location', token_state='$state'";
        $sql .= " WHERE token_key='$token_key'";
        return $this->game->DbQuery($sql);
    }

    /**
     * Move tokens (array of ids) to specific location in specific state, if state set to null do not change state
     */
    function moveTokens(array $tokens, string $location, ?int $state = 0): void {
        self::checkLocation($location);
        self::checkState($state, true);
        self::checkTokenKeyArray($tokens);
        $sql = "UPDATE " . $this->table;
        $sql .= " SET token_location='$location'";
        if ($state !== null) {
            $sql .= ", token_state='$state'";
        }
        $sql .= " WHERE token_key IN ('" . implode("','", $tokens) . "')";
        $this->game->DbQuery($sql);
        $this->clear_cache();
    }

    // Move a card to a specific location where card are ordered. If location_arg place is already taken, increment
    // all tokens after location_arg in order to insert new card at this precise location
    function insertToken(string $token_key, string $location, int $state = 0): void {
        self::checkLocation($location);
        self::checkState($state);
        $sql = "UPDATE " . $this->table;
        $sql .= " SET token_state=token_state+1";
        $sql .= " WHERE token_location='$location' ";
        $sql .= " AND token_state>=$state";
        $this->game->DbQuery($sql);
        self::moveToken($token_key, $location, $state);
        $this->clear_cache();
    }

    function insertTokenOnExtremePosition(string $token_key, string $location, bool $bOnTop): int {
        $extreme_pos = self::getExtremePosition($bOnTop, $location);
        if ($bOnTop) {
            $pos = $extreme_pos + 1;
        } else {
            $pos = $extreme_pos - 1;
        }
        self::insertToken($token_key, $location, $pos);
        $this->clear_cache();
        return $pos;
    }

    // Move all tokens from a location to another
    // !!! state is reset to 0 or specified value !!!
    // if "from_location" and "from_state" are null: move ALL cards to specific location
    function moveAllTokensInLocation(?string $from_location, string $to_location, ?int $from_state = null, int $to_state = 0): void {
        if ($from_location != null) {
            self::checkLocation($from_location);
        }
        self::checkLocation($to_location);
        $sql = "UPDATE " . $this->table . " ";
        $sql .= "SET token_location='$to_location', token_state='$to_state' ";
        if ($from_location !== null) {
            $sql .= "WHERE token_location='" . addslashes($from_location) . "' ";
            if ($from_state !== null) {
                $sql .= "AND token_state='$from_state' ";
            }
        }
        $this->game->DbQuery($sql);
        $this->clear_cache();
    }

    /**
     * Move all tokens from a location to another location arg stays with the same value
     */
    function moveAllTokensInLocationKeepOrder(string $from_location, string $to_location): void {
        self::checkLocation($from_location);
        self::checkLocation($to_location);
        $sql = "UPDATE " . $this->table;
        $sql .= " SET token_location='$to_location'";
        $sql .= " WHERE token_location='$from_location'";
        $this->game->DbQuery($sql);
        $this->clear_cache();
    }

    /**
     * Get tokens of a specific type in a specific location, since there is no field for type we use like expression on
     * key
     *
     * @param ?string $type - null matches any type; if type contains % it will be treated as LIKE, otherwise % will be appended
     * @param ?string $location - null matches any location; if location contains % it will be treated as LIKE
     * @param int|string|null $state - state can be just numeric or can be expression i.e. "!=0"
     * @param ?string $order_by - field to order by (it has to start with "token_" prefix)
     */
    function getTokensOfTypeInLocation(?string $type, ?string $location = null, int|string|null $state = null, ?string $order_by = null): array {
        $sql = $this->getSelectQuery();
        $sql .= " WHERE true ";
        if ($type !== null) {
            if (strpos($type, "%") === false) {
                $type .= "%";
            }
            self::checkType($type);
            $type = preg_replace("/_/", "\\_", $type);
            $sql .= " AND token_key LIKE '$type'";
        }
        if ($location !== null) {
            self::checkLocation($location, true, false);
            $like = "LIKE";
            if (strpos($location, "%") === false) {
                $like = "=";
            } else {
                $location = preg_replace("/_/", "\\_", $location);
            }
            $sql .= " AND token_location $like '$location' ";
        }
        if ($state !== null) {
            self::checkState($state, false);
            $sql .= " AND token_state = '$state'";
        }
        if ($order_by !== null) {
            $sql .= " ORDER BY $order_by ASC";
        }

        return $this->game->getCollectionFromDB($sql);
    }

    function getTokensOnLocations(array $locs): array {
        $sql = $this->getSelectQuery();
        $sql .= " WHERE token_location IN ('" . implode("','", $locs) . "') ";
        return $this->game->getCollectionFromDB($sql);
    }

    function getTokensOfTypeInLocationSingle(?string $type, ?string $location = null, int|string|null $state = null, ?string $order_by = null): ?array {
        $res = $this->getTokensOfTypeInLocation($type, $location, $state, $order_by);
        if (count($res) == 0) {
            return null;
        }
        return reset($res);
    }

    function getTokensOfTypeInLocationSingleKey(?string $type, ?string $location = null, int|string|null $state = null, ?string $order_by = null): mixed {
        $res = $this->getTokensOfTypeInLocation($type, $location, $state, $order_by);
        if (count($res) == 0) {
            return null;
        }
        return reset($res)["key"];
    }

    function getTokenState(string $token_id, int $def = 0): int {
        $res = $this->getTokenInfo($token_id);
        if ($res == null) {
            return $def;
        }
        return (int) $res["state"];
    }

    function getTokenLocation(string $token_id): ?string {
        $res = $this->getTokenInfo($token_id);
        if ($res == null) {
            return null;
        }
        return $res["location"];
    }

    /**
     * Get specific token info
     */
    function getTokenInfo(string $token_key): ?array {
        self::checkKey($token_key);
        $this->init_cache();
        return $this->keyindex[$token_key] ?? null;
    }

    function getTokensInfo(array $tokens): array {
        self::checkTokenKeyArray($tokens);
        $this->init_cache();
        $res = [];
        foreach ($tokens as $id) {
            $res[$id] = $this->keyindex[$id] ?? null;
        }
        return $res;
    }

    function countTokensInLocation(string $location, ?int $state = null): mixed {
        self::checkLocation($location, true);
        self::checkState($state, true);
        $like = "LIKE";
        if (strpos($location, "%") === false) {
            $like = "=";
        } else {
            $location = preg_replace("/_/", "\\_", $location);
        }
        $sql = "SELECT COUNT( token_key ) cnt FROM " . $this->table;
        $sql .= " WHERE token_location $like '$location' ";
        if ($state !== null) {
            $sql .= "AND token_state='$state' ";
        }

        return $this->game->getUniqueValueFromDB($sql);
    }

    // Return an array "location" => number of cards
    function countTokensInLocations(): array {
        $result = [];
        $sql = "SELECT token_location, COUNT( token_key ) cnt FROM " . $this->table . " GROUP BY token_location ";
        return $this->game->getCollectionFromDB($sql, true);
    }

    function varsub(mixed $line, array $keymap): mixed {
        if ($line === null) {
            throw new SystemException("varsub: line cannot be null");
        }
        if (strpos($line, "{") !== false) {
            foreach ($keymap as $key => $value) {
                if (strpos($line, "{$key}") !== false) {
                    $line = preg_replace("/\{$key\}/", $value, $line);
                }
            }
        }
        return $line;
    }

    final function checkLocation(?string $location, bool $like = false, bool $canBeNull = false): void {
        if ($location === null) {
            if ($canBeNull === false) {
                throw new SystemException("location cannot be null");
            } else {
                return;
            }
        }
        $extra = "";
        if ($like) {
            $extra = "%";
        }
        if (preg_match("/^[A-Za-z{$extra}][A-Za-z_0-9{$extra}-]*$/", $location) == 0) {
            throw new SystemException("location must be alphanum and underscore non empty string");
        }
    }

    final function checkState(int|string|null $state, bool $canBeNull = false): void {
        if ($state === null && $canBeNull == false) {
            throw new SystemException("state cannot be null");
        }
        if ($state !== null && preg_match("/^-?[0-9]+$/", $state) != 1) {
            throw new SystemException("state must be integer number");
        }
    }

    final function checkTokenKeyArray(array $token_arr): void {
        $res = $this->checkListOrTokenArray($token_arr);
        if ($res != 1) {
            $debug = var_export($token_arr, true);
            throw new SystemException("token_arr is not a list of token ids $res: $debug");
        }
    }

    final function checkKey(mixed $key, bool $like = false): void {
        if ($key == null) {
            throw new SystemException("key cannot be null");
        }
        if (!is_string($key)) {
            throw new SystemException("key is not a string");
        }
        $extra = "";
        if ($like) {
            $extra = "%";
        }
        if (preg_match("/^[A-Za-z_0-9{$extra}]+$/", $key) == 0) {
            throw new SystemException("key must be alphanum and underscore non empty string '$key'");
        }
    }

    final function checkType(mixed $key): void {
        if ($key == null) {
            throw new SystemException("type cannot be null");
        }
        $this->checkKey($key, true);
    }

    final function checkPosInt(mixed $key): void {
        if ($key && preg_match("/^[0-9]+$/", $key) == 0) {
            throw new SystemException("must be integer number");
        }
    }

    /**
     * Checks that given array either list of keys or list returned by function such get getTokensInfo which is map of
     * key => info pairs
     * throws exception if not of any of this structures, otherwise it returns
     *
     * @return int code, one of:<br>
     *         0 - array is empty
     *         1 - array of token ids
     *         2 - array of key => info map with token_id as index
     *         3 - array of key => info with number as index (returned by sorting methods)
     */
    final function checkListOrTokenArray(mixed $token_arr, bool $bThrow = true): int {
        try {
            if ($token_arr === null) {
                throw new SystemException("token_arr cannot be null");
            }
            $debug = var_export($token_arr, true);
            if (!is_array($token_arr)) {
                throw new SystemException("token_arr is not an array: $debug");
            }
            if (count($token_arr) == 0) {
                return 0;
            }
            $type = -1;
            foreach ($token_arr as $key => $info) {
                $typeone = $this->checkTokenIdOrInfo($info);
                if ($type == -1) {
                    $type = $typeone;
                } elseif ($type != $typeone) {
                    throw new SystemException("token_arr data has mixed types $type != $typeone: $debug");
                }
                if (is_numeric($key)) {
                    // ok
                    if ($typeone == 2) {
                        $typeone = 3;
                    }
                } elseif ($typeone == 2) {
                    $k = $info["key"];
                    if ($key != $k) {
                        throw new SystemException("token_arr data key info mismatch $key != $k: $debug");
                    }
                }
                if ($key === "key" || $key === "location" || $key === "state") {
                    throw new SystemException("token_arr data is not right array: $key $debug");
                }
            }
            return $type;
        } catch (SystemException $e) {
            if ($bThrow) {
                throw $e;
            }
            return -1;
        }
    }

    final function checkTokenIdOrInfo(mixed $info, bool $bThrow = true): int {
        try {
            if (is_array($info)) {
                if (array_key_exists("key", $info)) {
                    $this->checkKey($info["key"]);
                    return 2;
                }
                $debug = var_export($info, true);
                throw new SystemException("token info structure is not correct: $debug");
            } else {
                $this->checkKey($info);
                return 1;
            }
        } catch (SystemException $e) {
            if ($bThrow) {
                throw $e;
            }
            return -1;
        }
    }

    /**
     * Converts arbitrary data structure to token key list
     * Accepted structures
     *    <li> string id
     *    <li> info - i.e. ['key'=>'x', 'state'=>1, 'location'=>'y']
     *    <li> info[] - info array (with key or integer as index)
     *    <li> id[] - id array
     * @return string[] - token id list
     */
    function toTokenKeyList(string|array $tokens): array {
        if (!$tokens) {
            return [];
        }
        $kind = $this->checkTokenIdOrInfo($tokens, false);
        switch ($kind) {
            case 1: // key
                return [$tokens];
            case 2: // info
                return [$tokens["key"]];
        }
        $kind = $this->checkListOrTokenArray($tokens);
        switch ($kind) {
            case 0:
                return [];
            case 1:
                return $tokens;
            case 2:
                return array_keys($tokens);
            case 3:
                $keys = [];
                foreach ($tokens as $info) {
                    $keys[] = $info["key"];
                }
                return $keys;
            default:
                $debug = var_export($tokens, true);
                throw new SystemException("tokens structure is not supported: $debug");
        }
    }

    function getSelectQuery(): string {
        $sql = "SELECT token_key AS \"key\", token_location AS \"location\", token_state AS \"state\"";
        $sql .= " FROM " . $this->table;
        return $sql;
    }

    function dbReplaceValues(array $values): void {
        if (count($values) == 0) {
            return;
        }
        $this->clear_cache();
        $fields_list = $this->game->dbGetFieldList($this->table);
        $key = array_shift($fields_list);
        $table = $this->table;
        foreach ($values as $row) {
            $quoted = [];
            foreach ($fields_list as $field) {
                $value = $row[$field] ?? null;

                if ($value === null) {
                    $quoted[] = "$field = NULL";
                } elseif (is_numeric($value)) {
                    $quoted[] = "$field = $value";
                } else {
                    $value = $this->game->escapeStringForDB($value);
                    $quoted[] = "$field = '$value'";
                }
            }
            $setValues = implode(",", $quoted);
            $keyValue = $row[$key];
            $sql = "UPDATE $table SET $setValues WHERE $key = '$keyValue'";

            $this->game->DbQuery($sql);
        }
    }

    // ---- Game-level token helpers: notifications, counters, material-based creation ----

    protected function setCounter(array &$array, string $key, mixed $value): void {
        $array[$key] = ["value" => $value, "name" => $key];
    }

    protected function counterNameOf(string $location): string {
        return "counter_$location";
    }

    protected function fillCounters(array &$array, array $locs, bool $create = true): void {
        foreach ($locs as $location => $count) {
            $key = $this->counterNameOf($location);
            if ($create || array_key_exists($key, $array)) {
                $this->setCounter($array, $key, $count);
            }
        }
    }

    function autoreshuffleHandler(string $place_from, string $place_to): void {
        $player_id = $this->game->getMostlyActivePlayerId();
        if ($this->isCounterAllowedForLocation($player_id, $place_from)) {
            $this->notifyCounterChanged($place_from, ["nod" => true]);
        }
        if ($place_to != $place_from && $this->isCounterAllowedForLocation($player_id, $place_to)) {
            $this->notifyCounterChanged($place_to, ["nod" => true]);
        }
    }

    protected function fillTokensFromArray(array &$array, array $cards): void {
        foreach ($cards as $pos => $card) {
            $id = $card["key"];
            $array[$id] = $card;
        }
    }

    public function getTokenName(string|array|null $token_id): mixed {
        if (is_array($token_id)) {
            return $token_id;
        }
        if ($token_id == null) {
            return "null";
        }
        if (!$token_id) {
            return "";
        }
        return $this->getRulesFor($token_id, "name", $token_id);
    }

    public function getAllDatas(): array {
        $result = [];
        $current_player_id = $this->game->getCurrentPlayerId(); // !! We must only return informations visible by this player !!

        $token_types = $this->game->material->get();
        $result["token_types"] = $token_types;
        $result["tokens"] = [];
        $result["counters"] = $this->getDefaultCounters();
        $locs = $this->countTokensInLocations();
        //$color = $this->getPlayerColor($current_player_id);
        foreach ($locs as $location => $count) {
            $sort = $this->getRulesFor($location, "sort", null);
            //$this->game->debugLog("$location sort=$sort");
            if ($this->isCounterAllowedForLocation($current_player_id, $location)) {
                $this->fillCounters($result["counters"], [$location => $count]);
            }
            $content = $this->isContentAllowedForLocation($current_player_id, $location);

            if ($content === false) {
                continue;
            }
            if ($content === true) {
                $tokens = $this->getTokensOfTypeInLocation(null, $location, null, $sort);
                $this->fillTokensFromArray($result["tokens"], $tokens);
            } else {
                $num = floor($content);
                if ($count < $num) {
                    $num = $count;
                }
                $tokens = $this->getTokensOnTop($num, $location);
                $this->fillTokensFromArray($result["tokens"], $tokens);
            }
        }

        return $result;
    }

    function getReverseLocationTokensMapping(array $tokens, bool $flatten = false): array {
        $array = [];
        foreach ($tokens as $pos => $token) {
            $id = $token["location"];
            if ($flatten) {
                $array[$id] = $token["key"];
            } else {
                if (!array_get($array, $id)) {
                    $array[$id] = [];
                }
                $array[$id][] = $token["key"];
            }
        }
        return $array;
    }

    protected function getDefaultCounters(): array {
        $token_types = $this->game->material->get();
        $types = $token_types;
        $res = [];
        $players_basic = $this->game->loadPlayersBasicInfosWithBots();
        foreach ($types as $key => $info) {
            if (!$this->isConsideredLocation($key)) {
                continue;
            }
            $scope = array_get($info, "scope");
            $counter = array_get($info, "counter");
            if ($scope && $counter != "hidden") {
                if ($scope == "player") {
                    // per player location
                    foreach ($players_basic as $player_info) {
                        $color = $player_info["player_color"];
                        $this->setCounter($res, $this->counterNameOf("{$key}_{$color}"), 0);
                    }
                } else {
                    $this->setCounter($res, $this->counterNameOf("{$key}"), 0);
                }
            }
        }
        return $res;
    }

    function getAllRules(string $token_id, mixed $default = []): mixed {
        return $this->getRulesFor($token_id, "*", $default);
    }

    function getRulesFor(string $token_id, string $field = "r", mixed $default = ""): mixed {
        return $this->game->material->getRulesFor($token_id, $field, $default);
    }

    /**
     * Create tokens based on fields found in $this->token_types
     * Only tokens with 'create' field set will be considered
     * 'create' field can be one the following values:
     * 1 - the token with id $id will be created, count must be set to 1 if used
     * 4 - the token with id "${id}_{COLOR}" for each player will be created, count must be 1
     * 2 - the token with id "${id}_{INDEX}" will be created, using count
     * 3 - the token with id "${id}_{COLOR}_{INDEX}" will be created, using count, per player
     * 'location' - if set token will be created on this location, if not set in 'limbo'
     * 'state' - if set token will be create with this state, otherwise it is 0
     */
    function createAllTokens(): void {
        $token_types = $this->game->material->get();
        foreach ($token_types as $id => $info) {
            $this->createTokenFromInfo($id, $info);
        }
    }

    function createTokenFromInfo(string $id, array $info): void {
        $create_type = array_get($info, "create", 0);
        if (!$create_type) {
            return;
        }
        $count = array_get($info, "count", 1);

        if (!$count) {
            return;
        }

        try {
            $token_id = $id;
            if ($create_type === 1 || $create_type === "single") {
                $token_id = $id;
            } elseif ($create_type === 2 || $create_type === "index") {
                $token_id = "{$id}_{INDEX}";
            } elseif ($create_type === 3 || $create_type === "color_index") {
                $token_id = "{$id}_{COLOR}_{INDEX}";
            } elseif ($create_type === 4 || $create_type === "color") {
                $token_id = "{$id}_{COLOR}";
            } elseif ($create_type === 5 || $create_type === "index_color") {
                $token_id = "{$id}_{INDEX}_{COLOR}";
            }
            if (strpos($token_id, "{INDEX}") === false) {
                $count = 1;
            }
            // location and state use recursive parent fallback
            $location = $this->game->getRulesFor($id, "location", $info["location"] ?? "limbo");
            $state = $this->game->getRulesFor($id, "state", $info["state"] ?? 0);
            $start = array_get($info, "start", 1);
            if (strpos($token_id, "{COLOR}") === false) {
                $this->createTokensPack($token_id, $location, $count, $start, null, $state);
            } else {
                $this->createTokensPack($token_id, $location, $count, $start, $this->game->getPlayerColors(), $state);
            }
        } catch (\Exception $e) {
            $location = $this->game->getRulesFor($id, "location", $info["location"] ?? "limbo");
            $this->game->systemAssert("Failed to create tokens in location $token_id $location x $count ");
        }
    }

    protected function isConsideredLocation(string $id): bool {
        $type = $this->getRulesFor($id, "type", "");
        return $type == "location";
    }

    protected function isContentAllowedForLocation(int|string $player_id, string $location, string $attr = "content"): bool|int {
        if ($location === "dev_null") {
            return false;
        }

        if ($this->isConsideredLocation($location)) {
            $info = $this->getAllRules($location, null);
            $scope = array_get($info, "scope");
            $content_type = array_get($info, $attr);

            if ($scope) {
                if ($content_type == "public") {
                    // content allowed for everyboady
                    return true;
                }
                if (is_numeric($content_type)) {
                    // numeric content means show top N cards
                    return (int) $content_type;
                }
                if ($content_type == "private" && $this->game->isRealPlayer($player_id)) {
                    // content allow only if location of same color
                    $color = $this->game->custom_getPlayerColorById($player_id);
                    return str_ends_with($location, $color);
                }
                return false;
            } else {
                return false; // not listed as location
            }
        }

        if ($attr == "counter") {
            return false;
        } // not listed - do not need counter
        return true; // otherwise it location ok
    }

    protected function isCounterAllowedForLocation(int|string $player_id, string $location): bool|int {
        return $this->isContentAllowedForLocation($player_id, $location, "counter");
    }

    function dbSetTokenState(string|array $token_id, ?int $state = null, string $notif = "*", array $args = [], int $player_id = 0): void {
        $this->dbSetTokenLocation($token_id, null, $state, $notif, $args, $player_id);
    }

    function dbPickTokensForLocation(int $count, string $from_place, string $to_place, ?int $state = null, string $notif = "*", array $args = [], int $player_id = 0): void {
        $picks = $this->pickTokensForLocation($count, $from_place, $to_place);
        $real = count($picks);

        if ($real > 0) {
            if ($real == 1) {
                $pick = array_shift($picks);
                $this->dbSetTokenLocation($pick["key"], $to_place, $state, $notif, ["place_from" => $from_place] + $args, $player_id);
            } else {
                $this->dbSetTokensLocation($picks, $to_place, $state, $notif, ["place_from" => $from_place] + $args, $player_id);
            }
        } else {
            $this->game->notifyMessage(clienttranslate('Nothing left in ${token_name}'), ["token_name" => $from_place]);
        }
    }

    /**
     * Move a token to a new location, update DB, and send a "tokenMoved" notification.
     *
     * The following keys are auto-added to $args (and available in $notif as substitution variables):
     *  - token_id   — the token key being moved
     *  - place_id   — the destination location
     *  - token_name — will be change to actual token name on the client
     *  - place_name — same as place_id (used for log rendering, will be change to name)
     *  - new_state  — the state value after the move
     *  - place_from — the token's previous location
     *  - token_div  — same as token_id (only added when $notif contains '${token_div}')
     *
     * Caller-supplied $args take precedence over auto-added keys (merged after).
     *
     * @param string|array $token_id  Token key to move (array token-info is tolerated and reduced to its key)
     * @param string|null $place_id  Destination location (null = keep current location)
     * @param int|null    $state     New state value (null = keep current state)
     * @param string      $notif     Notification message with ${…} placeholders ("*" = default message)
     * @param array       $args      Extra notification args (merged over auto-added keys)
     * @param int         $player_id Player to attribute the notification to (0 = auto-detect)
     */
    function dbSetTokenLocation(string|array $token_id, ?string $place_id, ?int $state = null, string $notif = "*", array $args = [], int $player_id = 0): void {
        if (is_array($token_id)) {
            $this->game->error("token_id is array " . toJson($token_id));
            $token_id = array_get($token_id, "key");
        }
        $this->game->systemAssert("token_id is null/empty $token_id, $place_id $notif", $token_id != null && $token_id != "");
        if ($notif === "*") {
            $notif = clienttranslate('${player_name} moves ${token_name} into ${place_name} ${reason}');
        }
        if ($state === null) {
            $state = $this->getTokenState($token_id);
        }
        $place_from = $this->getTokenLocation($token_id) ?? "limbo";
        $this->game->systemAssert("token_id does not exists, create first: $token_id", $place_from);
        if ($place_id === null) {
            $place_id = $place_from;
        }
        $this->moveToken($token_id, $place_id, $state);

        $notifyArgs = [
            "token_id" => $token_id,
            "place_id" => $place_id,
            "new_state" => $state,
            "place_from" => $place_from,
        ];

        if (str_contains($notif, '${token_div}')) {
            $notifyArgs["token_div"] = $token_id;
        }

        if (str_contains($notif, '${place_name}')) {
            $notifyArgs["place_name"] = $place_id;
        }
        if (str_contains($notif, '${token_name}')) {
            $notifyArgs["token_name"] = $token_id;
        }
        $args = array_merge($notifyArgs, $args);

        //$this->warn("$type $notif ".$args['token_id']." -> ".$args['place_id']."|");
        if ($player_id != 0) {
            // use it
        } elseif (array_key_exists("player_id", $args)) {
            $player_id = $args["player_id"];
        } else {
            $player_id = $this->game->getMostlyActivePlayerId();
        }

        $this->game->notifyWithName("tokenMoved", $notif, $args, $player_id);
        if ($this->isCounterAllowedForLocation($player_id, $place_from)) {
            $this->notifyCounterChanged($place_from, ["nod" => true]);
        }
        if ($place_id != $place_from && $this->isCounterAllowedForLocation($player_id, $place_id)) {
            $this->notifyCounterChanged($place_id, ["nod" => true]);
        }
    }

    /**
     * Sends tokenMove notification with multiple objects, parameters of notication (must be handled by tokenMove)
     * list - array of token ids
     * token_divs - comma separate list of tokens (to inject visualisation)
     * token_names - comma separate list of tokens (to inject names)
     * new_state - if same state - new state of all tokens
     * new_states - if multiple states array of integer states
     *
     * @param array $token_arr
     *            - array of tokens keys or token info
     * @param string $place_id
     *            - location of all tokens will be set to $place_id value
     * @param null|int $state
     *            - if null is passed state won't be changed
     * @param string $notif
     * @param array $args
     */
    function dbSetTokensLocation(array $token_arr, string $place_id, ?int $state = null, string $notif = "*", array $args = [], int $player_id = 0): void {
        $type = $this->checkListOrTokenArray($token_arr);
        if ($type == 0) {
            return;
        }
        $this->game->systemAssert("place_id cannot be null", $place_id != null);
        if ($notif === "*") {
            $notif = clienttranslate('${player_name} moves ${token_names} into ${place_name} ${reason}');
        }
        $keys = [];
        $states = [];
        if (isset($args["place_from"])) {
            $place_from = $args["place_from"];
        } else {
            $place_from = null;
        }
        foreach ($token_arr as $token) {
            if (is_array($token)) {
                $token_id = $token["key"];
                $states[] = $token["state"];
                if ($place_from == null) {
                    $place_from = $token["location"];
                }
            } else {
                $token_id = $token;
            }
            $keys[] = $token_id;
        }
        $this->moveTokens($keys, $place_id, $state);
        $notifyArgs = [
            "list" => $keys, //
            "place_id" => $place_id, //
            "place_name" => $place_id,
        ];
        if ($state !== null) {
            $notifyArgs["new_state"] = $state;
        } elseif (count($states) > 0) {
            $notifyArgs["new_states"] = $states; // this only used for visualization, state won't change in db
        }
        if (strstr($notif, '${you}')) {
            $notifyArgs["you"] = "you"; // translated on client side, this is for replay after
        }
        if (strstr($notif, '${token_divs}')) {
            $notifyArgs["token_divs"] = implode(",", $keys);
        }
        if (strstr($notif, '${token_div}')) {
            $notifyArgs["token_div"] = $keys[0];
        }
        if (strstr($notif, '${token_names}')) {
            $notifyArgs["token_names"] = implode(",", $keys);
        }
        if (strstr($notif, '${token_name}')) {
            $notifyArgs["token_name"] = $keys[0];
        }
        $num = count($keys);
        if (strstr($notif, '${token_div_count}') || strstr($notif, '${count}')) {
            $notifyArgs["count"] = $num;
        }
        $notifyArgs["place_from"] = $place_from;
        $args = array_merge($notifyArgs, $args);
        //$this->warn("$type $notif ".$args['token_id']." -> ".$args['place_id']."|");
        if (!$player_id) {
            if (array_key_exists("player_id", $args)) {
                $player_id = $args["player_id"];
            } else {
                $player_id = $this->game->getMostlyActivePlayerId();
            }
        }
        $this->game->notifyWithName("tokenMoved", $notif, $args, $player_id);
        // send counter update if required
        if ($place_from && $this->isCounterAllowedForLocation($player_id, $place_from)) {
            $this->notifyCounterChanged($place_from, ["nod" => true]);
        }
        if ($place_id != $place_from && $this->isCounterAllowedForLocation($player_id, $place_id)) {
            $this->notifyCounterChanged($place_id, ["nod" => true]);
        }
    }

    /**
     * This method will increase/descrease resource counter (as state)
     *
     * @param string $token_id
     *            - token key
     * @param int $num
     *            - increment of the change
     */
    function dbResourceInc(string $token_id, int $num, string $message = "*", array $args = [], ?int $player_id = null): int {
        $current = $this->getTokenState($token_id, 0);
        $value = $current + $num;

        $this->setTokenState($token_id, $value);

        if ($message == "*") {
            if ($num <= 0) {
                $message = clienttranslate('${player_name} pays ${token_div} x ${absInc} ${reason}');
            } else {
                $message = clienttranslate('${player_name} gains ${token_div} x ${absInc} ${reason}');
            }
        }

        $args = array_merge($args, [
            "inc" => $num,
            "absInc" => abs($num),
            "token_div" => $token_id,
        ]);

        $this->notifyCounterDirect($token_id, $value, $message, $args, $player_id);
        return $value;
    }

    function notifyCounterChanged(string $location, ?array $notifyArgs = null): void {
        $key = $this->counterNameOf($location);
        $value = $this->countTokensInLocation($location);
        $this->notifyCounterDirect($key, $value, "", $notifyArgs);
    }

    function notifyCounterDirect(string $key, mixed $value, string $message, ?array $notifyArgs = null, ?int $player_id = null): void {
        $args = ["name" => $key, "value" => $value];
        if ($notifyArgs != null) {
            $args = array_merge($notifyArgs, $args);
        }
        $this->game->notifyWithName("counter", $message, $args, $player_id);
    }

    function getTrackerValue(?string $color, string $type): int {
        $value = (int) $this->getTokenState($this->getTrackerId($color, $type));
        return $value;
    }
    function getTrackerIdAndValue(?string $color, string $type, ?array &$arr = null): array {
        $id = $this->getTrackerId($color, $type);
        $value = (int) $this->getTokenState($id);
        if ($arr) {
            $arr[$id] = $value;
        }
        return [$id, $value];
    }

    function incTrackerValue(string $color, string $type, int $delta): int {
        $id = $this->getTrackerId($color, $type);
        return $this->dbResourceInc($id, $delta);
    }

    function getTrackerId(string $color, string $type): string {
        if ($color === "") {
            $token_id = "tracker_{$type}";
        } else {
            if (!$color) {
                $color = $this->game->getActivePlayerColor();
            }
            $token_id = "tracker_{$type}_{$color}";
        }
        return $token_id;
    }

    function getTokensOfTypeInLocationWithChildren(?string $type, ?string $location = null, int|string|null $state = null, ?string $order_by = null): array {
        $tokens = $this->getTokensOfTypeInLocation($type, $location, $state, $order_by);
        // init children array
        foreach ($tokens as $key => $token) {
            $tokens[$key]["children"] = [];
        }
        $children = $this->getTokensOnLocations(array_keys($tokens));
        foreach ($children as $key => $child) {
            $parent = $child["location"];
            $tokens[$parent]["children"][$key] = $this->getTokenInfo($key);
        }
        return $tokens;
    }

    public function toJson(): array {
        return [];
    }

    public function fromJson(array $rows): void {}
}
