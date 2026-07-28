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
 * material.inc.php
 *
 * kingdomino game material description
 *
 * Here, you can describe the material of your game with PHP variables.
 *
 * This file is loaded in your game logic class constructor, ie these variables
 * are available everywhere in your game logic code.
 *
 */
$this->dominoes = array(
    1 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "field", "crowns" => 0)),
    2 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "field", "crowns" => 0)),
    3 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    4 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    5 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    6 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    7 => array(
        "left" => array("terrain" => "lake", "crowns" => 0),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    8 => array(
        "left" => array("terrain" => "lake", "crowns" => 0),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    9 => array(
        "left" => array("terrain" => "lake", "crowns" => 0),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    10 => array(
        "left" => array("terrain" => "grassland", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    11 => array(
        "left" => array("terrain" => "grassland", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    12 => array(
        "left" => array("terrain" => "swamp", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 0)),
    13 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    14 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    15 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    16 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 0)),
    17 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    18 => array(
        "left" => array("terrain" => "forest", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    19 => array(
        "left" => array("terrain" => "field", "crowns" => 1),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    20 => array(
        "left" => array("terrain" => "field", "crowns" => 1),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    21 => array(
        "left" => array("terrain" => "field", "crowns" => 1),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    22 => array(
        "left" => array("terrain" => "field", "crowns" => 1),
        "right" => array("terrain" => "swamp", "crowns" => 0)),
    23 => array(
        "left" => array("terrain" => "field", "crowns" => 1),
        "right" => array("terrain" => "mountain", "crowns" => 0)),
    24 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    25 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    26 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    27 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    28 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "lake", "crowns" => 0)),
    29 => array(
        "left" => array("terrain" => "forest", "crowns" => 1),
        "right" => array("terrain" => "grassland", "crowns" => 0)),
    30 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    31 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    32 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    33 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    34 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    35 => array(
        "left" => array("terrain" => "lake", "crowns" => 1),
        "right" => array("terrain" => "forest", "crowns" => 0)),
    36 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 1)),
    37 => array(
        "left" => array("terrain" => "lake", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 1)),
    38 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 1)),
    39 => array(
        "left" => array("terrain" => "grassland", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 1)),
    40 => array(
        "left" => array("terrain" => "mountain", "crowns" => 1),
        "right" => array("terrain" => "field", "crowns" => 0)),
    41 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 2)),
    42 => array(
        "left" => array("terrain" => "lake", "crowns" => 0),
        "right" => array("terrain" => "grassland", "crowns" => 2)),
    43 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 2)),
    44 => array(
        "left" => array("terrain" => "grassland", "crowns" => 0),
        "right" => array("terrain" => "swamp", "crowns" => 2)),
    45 => array(
        "left" => array("terrain" => "mountain", "crowns" => 2),
        "right" => array("terrain" => "field", "crowns" => 0)),
    46 => array(
        "left" => array("terrain" => "swamp", "crowns" => 0),
        "right" => array("terrain" => "mountain", "crowns" => 2)),
    47 => array(
        "left" => array("terrain" => "swamp", "crowns" => 0),
        "right" => array("terrain" => "mountain", "crowns" => 2)),
    48 => array(
        "left" => array("terrain" => "field", "crowns" => 0),
        "right" => array("terrain" => "mountain", "crowns" => 3)),
);

$this->terrains = [
    'forest' => clienttranslate('Forests'),
    'field' => clienttranslate('Fields'),
    'grassland' => clienttranslate('Grasslands'),
    'lake' => clienttranslate('Lakes'),
    'swamp' => clienttranslate('Swamps'),
    'mountain' => clienttranslate('Mountains')
];

// The Lost Treasures expansion gem tokens, keyed treasure_1..treasure_16.
// faces = [TL, TR, BR, BL]: n crowns, 0 nothing, -1 skull. The three tokens of a
// gem share type and faces.
$this->treasures = [
    'treasure_1' => ['type' => 'yellow', 'faces' => [2, 0, 1, 0]],
    'treasure_2' => ['type' => 'yellow', 'faces' => [2, 0, 1, 0]],
    'treasure_3' => ['type' => 'yellow', 'faces' => [2, 0, 1, 0]],
    'treasure_4' => ['type' => 'green', 'faces' => [2, 1, 0, 0]],
    'treasure_5' => ['type' => 'green', 'faces' => [2, 1, 0, 0]],
    'treasure_6' => ['type' => 'green', 'faces' => [2, 1, 0, 0]],
    'treasure_7' => ['type' => 'pink', 'faces' => [1, 1, 0, 1]],
    'treasure_8' => ['type' => 'pink', 'faces' => [1, 1, 0, 1]],
    'treasure_9' => ['type' => 'pink', 'faces' => [1, 1, 0, 1]],
    'treasure_10' => ['type' => 'blue', 'faces' => [3, 0, -1, 0]],
    'treasure_11' => ['type' => 'blue', 'faces' => [3, 0, -1, 0]],
    'treasure_12' => ['type' => 'blue', 'faces' => [3, 0, -1, 0]],
    'treasure_13' => ['type' => 'red', 'faces' => [1, 2, 0, 0]],
    'treasure_14' => ['type' => 'red', 'faces' => [1, 2, 0, 0]],
    'treasure_15' => ['type' => 'red', 'faces' => [1, 2, 0, 0]],
    'treasure_16' => ['type' => 'joker', 'faces' => [0, 0, 1, 0]],
];