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
 * kingdomino.view.php
 *
 * This is your "view" file.
 *
 * The method "build_page" below is called each time the game interface is displayed to a player, ie:
 * _ when the game starts
 * _ when a player refreshes the game page (F5)
 *
 * "build_page" method allows you to dynamically modify the HTML generated for the game interface. In
 * particular, you can set here the values of variables elements defined in kingdomino_kingdomino.tpl (elements
 * like {MY_VARIABLE_ELEMENT}), and insert HTML block elements (also defined in your HTML template file)
 *
 * Note: if the HTML of your game interface is always the same, you don't have to place anything here.
 *
 */
require_once(APP_BASE_PATH . "view/common/game.view.php");

class view_kingdomino_kingdomino extends game_view
{

    function getGameName()
    {
        return "kingdomino";
    }

    function getTemplateName()
    {
        return self::getGameName() . "_" . self::getGameName();
    }

    function build_page($viewArgs)
    {
        $currentPlayerId = $this->getCurrentPlayerId();
        $players = $this->game->loadPlayersBasicInfos();
        if (isset($players[$currentPlayerId])) {
            $player = $players[$currentPlayerId];
            $this->tpl['COLOR'] = $player['player_color'];
            $this->tpl['SPECTATOR'] = '';
        } else {
            $this->tpl['SPECTATOR'] = 'spectator';
            $this->tpl['COLOR'] = '';
        }
        $template = self::getTemplateName();
        $dynasty = $this->game->getGameStateValue('dynasty');
        $theMiddleKingdom = $this->game->getGameStateValue('the_middle_kingdom');
        $harmony = $this->game->getGameStateValue('harmony');
        $isMightyDuel = $this->game->isMightyDuel();
        $isLostTreasures = $this->game->isLostTreasures();
        $gridSize = $isMightyDuel ? 7 : 5;
        $this->tpl['SIZE'] = $gridSize * 100 + 40;
        if ($dynasty != 0 || $theMiddleKingdom != 0 || $harmony != 0 || $isMightyDuel || $isLostTreasures) {
            $this->tpl['ADDITIONAL_RULES'] = self::_("Additional rules:");
        } else {
            $this->tpl['ADDITIONAL_RULES'] = self::_("No additional rules");
        }
        $this->tpl['KINGDOM_TITLE'] = self::_("Your Kingdom");
        $this->tpl['DOMINOES_LIST'] = self::_("Dominoes list");
        $this->tpl['GRID_SIZE'] = $gridSize;
        $this->tpl['DYNASTY_VISIBLE'] = $dynasty != 0 ? 'visible' : 'hidden';
        $this->tpl['DYNASTY'] = self::_("Dynasty");
        $this->tpl['THE_MIDDLE_KINGDOM_VISIBLE'] = $theMiddleKingdom != 0 ? 'visible' : 'hidden';
        $this->tpl['THE_MIDDLE_KINGDOM'] = self::_("The middle Kingdom");
        $this->tpl['CENTERED'] = self::_("Centered");
        $this->tpl['HARMONY_VISIBLE'] = $harmony != 0 ? 'visible' : 'hidden';
        $this->tpl['HARMONY'] = self::_("Harmony");
        $this->tpl['USED_ALL'] = self::_("Used all");
        $this->tpl['THE_MIGHTY_DUEL_VISIBLE'] = $isMightyDuel ? 'visible' : 'hidden';
        $this->tpl['THE_MIGHTY_DUEL'] = self::_("The Mighty Duel");
        $this->tpl['LT_VISIBLE'] = $isLostTreasures ? 'visible' : 'hidden';
        $this->tpl['THE_LOST_TREASURES'] = self::_("The Lost Treasures");
        $this->page->begin_block($template, "current_domino_space");
        $this->page->begin_block($template, "future_domino_space");
        $dominoesAvailableEachTurn = count($players) != 3 || $this->game->getGameStateValue('3_players_v2') == 1 ? 4 : 3;
        for ($i = 1; $i <= $dominoesAvailableEachTurn; $i++) {
            $this->page->insert_block("current_domino_space", array('NUMBER' => $i));
            $this->page->insert_block("future_domino_space", array('NUMBER' => $i));
        }
    }
}
  

