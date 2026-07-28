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
 * kingdomino.action.php
 *
 * kingdomino main action entry point
 *
 *
 * In this file, you are describing all the methods that can be called from your
 * user interface logic (javascript).
 *
 * If you define a method "myAction" here, then you can call it from your javascript code with:
 * this.ajaxcall( "/kingdomino/kingdomino/myAction.html", ...)
 *
 */
class action_kingdomino extends APP_GameAction
{

    // Constructor: please do not modify
    public function __default()
    {
        if (self::isArg('notifwindow')) {
            $this->view = "common_notifwindow";
            $this->viewArgs ['table'] = self::getArg("table", AT_posint, true);
        } else {
            $this->view = "kingdomino_kingdomino";
            self::trace("Complete reinitialization of board game");
        }
    }

    function chooseDomino()
    {
        self::setAjaxMode();
        $number = self::getArg('number', AT_posint, true);
        $this->game->chooseDomino($number);
        self::ajaxResponse();
    }

    function placeDomino()
    {
        self::setAjaxMode();
        $x = self::getArg('x', AT_int, true);
        $y = self::getArg('y', AT_int, true);
        $rotation = self::getArg('rotation', AT_posint, true);
        $nextDomino = self::getArg('nextDomino', AT_posint);
        $this->game->placeDomino(array('x' => $x, 'y' => $y, 'rotation' => $rotation), $nextDomino);
        self::ajaxResponse();
    }

    function discardDomino()
    {
        self::setAjaxMode();
        $this->game->discardDomino();
        self::ajaxResponse();
    }

    function placeTreasure()
    {
        self::setAjaxMode();
        $tokenKey = self::getArg('tokenKey', AT_alphanum_dash, true);
        $x = self::getArg('x', AT_int, true);
        $y = self::getArg('y', AT_int, true);
        $rotation = self::getArg('rotation', AT_posint, true);
        $this->game->placeTreasure($tokenKey, $x, $y, $rotation);
        self::ajaxResponse();
    }
}
  

