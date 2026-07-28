/**
 * ------ BGA framework: © Gregory Isabelli <gisabelli@boardgamearena.com> & Emmanuel Colin <ecolin@boardgamearena.com> kingdomino
 * implementation : © Alena Laskavaia <laskava@gmail.com> & Romain Fromi <romain.fromi@gmail.com>
 *
 * This code has been produced on the BGA studio platform for use on http://boardgamearena.com. See
 * http://en.boardgamearena.com/#!doc/Studio for more information. -----
 *
 * kingdomino.js
 *
 * kingdomino user interface script
 *
 * In this file, you are describing the logic of your user interface, in Javascript language.
 *
 */

define(["dojo", "dojo/_base/declare", "ebg/core/gamegui", "ebg/counter", "ebg/scrollmap", "ebg/draggable"], function (dojo, declare) {
  return declare("bgagame.kingdomino", ebg.core.gamegui, {
    constructor: function () {
      // Here, you can init the global variables of your user interface
      // Example:
      // this.myGlobalValue = 0;

    },
    /*
     * setup:
     *
     * This method must set up the game user interface according to current game situation specified in parameters.
     *
     * The method is called each time the game interface is displayed to a player, ie: _ when the game starts _ when a player refreshes
     * the game page (F5)
     *
     * "gamedatas" argument contains all datas retrieved by your "getAllDatas" PHP method.
     */
    setup: function (gamedatas) {
      this.dontPreloadImage('tiles.jpg');
      this.dontPreloadImage('dominoes_list.webp');
      this.globMouse = {
        pageX: 0,
        pageY: 0
      };
      this.scrollmap = new ebg.scrollmap();

      this.kingId = 1;
      this.kingDraggable = {};
      try {
        this.gamedatas = gamedatas;
        var pkeys = Object.keys(gamedatas.players);
        var leading = this.player_id;
        if (!this.isSpectator) {
          this.player_color = gamedatas.players[this.player_id].color;
        } else {
          leading = pkeys[0];
          this.player_color = gamedatas.players[leading].color;
        }
        for (var playerId in gamedatas.players) {
          if (gamedatas.players.hasOwnProperty(playerId)) {
            gamedatas.players[playerId].kingdom = {minX: 0, minY: 0, maxX: 0, maxY: 0};
            if (parseInt(playerId) !== this.player_id) {
              var kingdom = this.format_block('jstpl_kingdom', {
                id: playerId,
                title: dojo.string.substitute(_("Kingdom of ${player}"), {player: gamedatas.players[playerId].name}),
                color: gamedatas.players[playerId].color,
                size: 100 * gamedatas.gridSize,
                sizeSmall: 80 * gamedatas.gridSize
              });
              dojo.place(kingdom, $('other_players_kingdoms'));
            }
          }
        }
        var current_domino_space = gamedatas.gamestate && gamedatas.gamestate.args && gamedatas.gamestate.args.currentPosition ? gamedatas.gamestate.args.currentPosition : 1;
        var future_domino_space = 1;
        for (var number in gamedatas.dominoes) {
          if (gamedatas.dominoes.hasOwnProperty(number)) {
            var domino = gamedatas.dominoes[number];
            if (domino.location === 'KINGDOM') {
              if (domino.owner_player === this.player_id.toString()) {
                this.createDomino(number);
                this.placeDomino(number, domino);
                dojo.place('domino_' + number, 'map_scrollable_oversurface');
              } else {
                this.placeOtherPlayerDomino(number, domino);
              }
              var kingdom = this.gamedatas.players[domino.owner_player].kingdom;
              var rotation = parseInt(domino.rotation);
              kingdom.minX = Math.min(kingdom.minX, rotation === 2 ? parseInt(domino.x) - 1 : parseInt(domino.x));
              kingdom.minY = Math.min(kingdom.minY, rotation === 1 ? parseInt(domino.y) - 1 : parseInt(domino.y));
              kingdom.maxX = Math.max(kingdom.maxX, rotation === 0 ? parseInt(domino.x) + 1 : parseInt(domino.x));
              kingdom.maxY = Math.max(kingdom.maxY, rotation === 3 ? parseInt(domino.y) + 1 : parseInt(domino.y));
            } else {
              this.createDomino(number);
              var location = domino.location === 'CURRENT' ? 'current_domino_space_' + current_domino_space : 'future_domino_space_' + future_domino_space;
              this.placeOnObject('domino_' + number, location);
              if (domino.location === 'CURRENT') {
                current_domino_space++;
              } else if (domino.location === 'FUTURE') {
                future_domino_space++;
              }
              if (domino.owner_player) {
                this.createKing(gamedatas.players[domino.owner_player].color, number);
              } else {
                dojo.connect($('domino_' + number), 'onclick', this, 'onDominoClick');
              }
            }
          }
        }

        for (var playerId in gamedatas.players) {
          if (gamedatas.players.hasOwnProperty(playerId)) {
            if (parseInt(playerId) !== this.player_id) {
              this.centerOtherKingdom(playerId);
            }
          }
        }

        this.updateDominoesRemaining();

        if (!this.isSpectator) {
          this.kingdom = this.gamedatas.players[this.player_id].kingdom;
          // Make map scrollable
          this.scrollmap.create($('map_container'), $('map_scrollable'), $('map_surface'), $('map_scrollable_oversurface'));
          dojo.connect($('movetop'), 'onclick', this, 'onMoveTop');
          dojo.connect($('moveleft'), 'onclick', this, 'onMoveLeft');
          dojo.connect($('moveright'), 'onclick', this, 'onMoveRight');
          dojo.connect($('movedown'), 'onclick', this, 'onMoveDown');
          this.scrollmap.disableScrolling();

          this.kingdomMapPosition = [0, 0];
          dojo.attr('kingdom', 'data-value', this.player_color);
          this.scrollmap.scrollto(-50, -50);
          this.moveKingdomIfNecessary();

          dojo.connect($('overall-content'), 'onmousemove', this, 'onMouseMove');
          dojo.connect($('overall-content'), 'ontouchmove', this, 'onMouseMove');
        }

        document.getElementById('dominoesListButton').addEventListener('click', () => {
          var dialog = new ebg.popindialog();
          dialog.create('dominoesListDialog');
          dialog.setTitle(_('Dominoes list'));
          dialog.setMaxWidth(1000);
          dialog.setContent('<div class="dominoes-list"></div>');
          dialog.show();
        });
        this.prepareAdditionalRuleInfo('dynastyButton', _('Dynasty'), _('Play 3 rounds in a row. At the end of the 3 rounds, the player with the highest number of points wins the game.'));
        this.prepareAdditionalRuleInfo('middleKingdomButton', _('The middle Kingdom'), _('Score 10 additional points if your castle is in the center of your territory.'));
        this.prepareAdditionalRuleInfo('harmonyButton', _('Harmony'), _('Score 5 additional points if your territory is complete (no discarded dominoes).'));
        this.prepareAdditionalRuleInfo('mightyDuelButton', _('The Mighty Duel'), _('Use ALL the dominoes to build a 7x7 grid.'));
        this.prepareAdditionalRuleInfo('lostTreasuresButton', _('The Lost Treasures'), _('Completing a 2x2 square of 4 different terrains lets you discover a treasure. Collecting all 5 treasure types wins the game immediately.'));
        dojo.connect($('moreInfoButton'), 'onclick', this, function (event) {
          dojo.stopEvent(event);
          if (dojo.hasClass('info', 'more-visible')) {
            dojo.removeClass('info', 'more-visible');
          } else {
            dojo.addClass('info', 'more-visible');
          }
          this.replaceAvailableDominoes();
        });

        if (gamedatas.scoring) {
          for (var territoryIndex = 0; territoryIndex < gamedatas.scoring.length; territoryIndex++) {
            this.scoreTerritory(gamedatas.scoring[territoryIndex]);
          }
          setTimeout(function () {
            dojo.query('.scoring-hidden').forEach(function (node) {
              dojo.removeClass(node, 'scoring-hidden');
            });
          }, 1);
          dojo.style('dominoes_displayed', 'height', '20px');
        }
        if (gamedatas.scoreSheet) {
          dojo.addClass('info_and_dominoes', 'scoring');
          for (var scoringIndex = 1; scoringIndex <= gamedatas.scoreSheet.length; scoringIndex++) {
            var scoreSheet = gamedatas.scoreSheet[scoringIndex - 1];
            $('score_sheet_player_' + scoringIndex).innerHTML = scoreSheet.name;
            dojo.style('score_sheet_player_' + scoringIndex, 'color', '#' + scoreSheet.color);
            dojo.attr('score_sheet_final_' + scoringIndex, 'data-value', scoreSheet.color);
            $('score_forest_' + scoringIndex).innerHTML = scoreSheet.forest;
            $('score_field_' + scoringIndex).innerHTML = scoreSheet.field;
            $('score_grassland_' + scoringIndex).innerHTML = scoreSheet.grassland;
            $('score_lake_' + scoringIndex).innerHTML = scoreSheet.lake;
            $('score_swamp_' + scoringIndex).innerHTML = scoreSheet.swamp;
            $('score_mountain_' + scoringIndex).innerHTML = scoreSheet.mountain;
            if (scoreSheet.displayBonus) {
              $('score_sheet_bonus_' + scoringIndex).innerHTML = '+' + scoreSheet.bonus;
            } else {
              dojo.destroy('score_sheet_bonus_' + scoringIndex);
            }
            $('score_sheet_total_' + scoringIndex).innerHTML = scoreSheet.score;
          }
        }

        if (gamedatas.treasures) {
          var supply = gamedatas.treasures.supply || [];
          for (var supplyIndex = 0; supplyIndex < supply.length; supplyIndex++) {
            var supplyKey = supply[supplyIndex];
            dojo.place(this.format_block('jstpl_treasure', {key: supplyKey}), 'treasure_supply_gems');
            this.addTooltipHtml(supplyKey, this.getTreasureTooltipHtml(supplyKey));
          }
          $('treasure_chest_count').innerHTML = gamedatas.treasures.boxCount;
          this.addTooltipHtml('treasure_chest', this.getTreasureChestTooltipHtml());

          var placedByPlayer = gamedatas.treasures.placed || {};
          for (var placedPlayerId in placedByPlayer) {
            if (placedByPlayer.hasOwnProperty(placedPlayerId)) {
              var gems = placedByPlayer[placedPlayerId];
              for (var gemIndex = 0; gemIndex < gems.length; gemIndex++) {
                var gem = gems[gemIndex];
                this.renderPlacedTreasure(placedPlayerId, gem.key, parseInt(gem.x), parseInt(gem.y), parseInt(gem.rotation));
              }
            }
          }
        }

        window.onresize = () => this.replaceAvailableDominoes();
        window.onfocus = () => this.replaceAvailableDominoes(); // Workaround for Firefox not playing animation before windows has focus (causing kings to be out of windows if tiles were not drawn yet: see https://boardgamearena.com/bug?id=13077)

        // Setup game notifications to handle (see "setupNotifications" method below)
        this.setupNotifications();
      } catch (e) {
        console.error("Exception thrown", e.stack);
        this.showMessage("Setup Error: Please raise a bug" + "\n" + e, "error");
      }
    },

    // Human-readable gem type names. Translated lazily so dojo's i18n picks them up.
    getTreasureTypeName: function (type) {
      var names = {
        yellow: _('Yellow Gem'),
        green: _('Green Gem'),
        pink: _('Pink Gem'),
        blue: _('Blue Gem'),
        red: _('Red Gem'),
        joker: _('Joker Gem'),
      };
      return names[type] || type;
    },

    // Render one gem face value: n crowns, 0 none, -1 skull.
    getTreasureFaceLabel: function (value) {
      if (value < 0) {
        return _('Skull');
      }
      if (value === 0) {
        return _('None');
      }
      // Singular/plural handled with a count substitution rather than two strings.
      return _('${n} Crown(s)').replace('${n}', value);
    },

    // Tooltip for a single treasure token. Works for any treasure_* id, supply or placed.
    getTreasureTooltipHtml: function (tokenKey) {
      var data = this.gamedatas.treasures.description[tokenKey];
      if (!data) {
        return '';
      }



      var explanation = _('Complete a 2x2 square of 4 different landscape types when placing a domino to discover this treasure. At end-game scoring its crown sides add crowns to the territory each side points at; a skull side makes that territory score 0.');
      var jokerNote = '';
      if (data.type === 'joker') {
        jokerNote = '<p>' + _('The Joker is wild: it counts as any gem type toward the "collect 5 different types = instant win" goal, but it still gives only its own crowns.') + '</p>';
      }
      const rows = data.faces.map(x=>this.getTreasureFaceLabel(x)).join(", ")
      return '<div class="treasure-tooltip tooltip-container">' +
        '<h3>' + this.getTreasureTypeName(data.type) + '</h3>' +
        '<div class="treasure ' + tokenKey + '"></div>' +
        '<p>' + _('Crowns per side:') + ' ' +
         rows  + 
        '</p>' +
        '<p>' + explanation + '</p>' +
        jokerNote +
        '</div>';
    },

    // Tooltip for the face-down chest pile in the supply.
    getTreasureChestTooltipHtml: function () {
      return '<div class="treasure-tooltip tooltip-container">' +
              '<h3>' + _('Treasure chests') + '</h3>' +
        '<div class="treasure treasure_back"></div>' +

        '<p>' + _('The remaining treasures are face down in the chest pile. Two gems are revealed and available to choose at a time. The number shown is how many treasures remain hidden.') + '</p>' +
        '</div>';
    },

    // The DOM node holding a player's kingdom tiles. The active player's tiles
    // live in the scrollable oversurface; everyone else has their own kingdom.
    getTreasureKingdomContainer: function (playerId) {
      if (!this.isSpectator && parseInt(playerId) === this.player_id) {
        return 'map_scrollable_oversurface';
      }
      return 'kingdom_' + playerId;
    },

    // Pixel point of the intersection identified by bottom-left tile (x,y): the
    // shared corner of tiles (x,y),(x+1,y),(x,y+1),(x+1,y+1).
    getTreasureIntersectionPixel: function (x, y) {
      return {left: (x + 1) * 100, top: -y * 100};
    },

    // animateFrom (optional): id of an on-screen node (the supply token) to slide
    // the placed gem out of. When omitted the gem appears instantly at its
    // intersection (used by setup for already-placed gems).
    renderPlacedTreasure: function (playerId, tokenKey, x, y, rotation, animateFrom) {
      const container = this.getTreasureKingdomContainer(playerId);
      if (!$(container)) {
        return;
      }
      const pixel = this.getTreasureIntersectionPixel(x, y);
      const id = 'placed_' + tokenKey;
      $(id)?.remove();
      $(container).insertAdjacentHTML('beforeend', this.format_block('jstpl_placed_treasure', {
        id: id,
        key: tokenKey,
        left: pixel.left,
        top: pixel.top,
        angle: rotation * 90
      }));
      this.addTooltipHtml(id, this.getTreasureTooltipHtml(tokenKey));
      // Slide in from the supply token: snap onto it (no transition), then restore
      // the resting left/top next frame so the CSS transition animates the move.
      if (animateFrom && $(animateFrom)) {
        const destLeft = $(id).style.left;
        const destTop = $(id).style.top;
        $(id).style.transition = 'none';
        this.placeOnObject(id, animateFrom);
        // Force reflow so the snapped position is committed before re-enabling
        // the transition, otherwise the browser collapses both writes.
        void $(id).offsetWidth;
        $(id).style.transition = '';
        $(id).style.left = destLeft;
        $(id).style.top = destTop;
      }
    },

    // /////////////////////////////////////////////////
    // // Game & client states
    // onEnteringState: this method is called each time we are entering into a new game state.
    // You can use this method to perform some user interface changes at this moment.
    //
    onEnteringState: function (stateName, state) {
      this.stateHandles = [];
      var self = this;
      switch (stateName) {
        case 'chooseDomino':
          if (this.isCurrentPlayerActive()) {
            const choosable = this.choosableDominoes();
            if (choosable.length === 1) {
              // No real choice left: pre-select the only domino and just ask to confirm.
              const number = choosable[0];
              dojo.addClass('domino_' + number, 'selected');
              this.bga.statusBar.addActionButton(_("Confirm"), () => {
                this.bga.actions.performAction('chooseDomino', {number: number});
              }, { id: "button_choose" });
            } else {
              this.highlightSelectableDominoes(false);
            }
          }
          break;
        case 'placeDomino':
          if (this.isCurrentPlayerActive()) {
            this.prepareBestPlacementSpots(state.args.kingdom, state.args);
            delete this.preselectedPosition;
            delete this.selectedPosition;
            this.placementDiscoversTreasure = false;
            if (Object.keys(this.bestPlacements).length === 4) {
              $("pagemaintitletext").innerHTML = dojo.string.substitute(_("${you} cannot place this domino"),
                {you: '<span id="pagemaintitletext"><span style="font-weight:bold;color:#' + this.gamedatas.players[this.player_id].color + ';">' + _('You') + '</span>'});
              var discardButton = this.format_block('jstpl_discard_button', {text: _("Discard domino")});
              dojo.place(discardButton, 'dominoes_displayed');
              dojo.connect($('discard_button'), 'onclick', this, 'onDiscardDomino');
              setTimeout(() => {
                this.placeOnObject('discard_button_mask', 'domino_' + state.args.domino);
              }, 1000);
              this.addActionButton('discard_button_bis', _('Discard domino'), 'onDiscardDomino');
            } else {
              this.draggableDomino = state.args.domino;
              this.preferredOrientation = 0;
              this.dominoDragged = false;
              var manipulationArrows = this.format_block('jstpl_manipulationArrows', {});
              dojo.query('#domino_' + state.args.domino + ' > .domino-background').forEach(function (node) {
                dojo.place(manipulationArrows, node);
              });
              dojo.query('#domino_' + state.args.domino + ' .manipulation-arrow').forEach(function (node) {
                self.stateHandles.push(dojo.connect(node, 'onclick', self, 'onManipulationArrowClick'));
                self.stateHandles.push(dojo.connect(node, 'ontouchend', self, 'onManipulationArrowClick'));
              });
              dojo.addClass('domino_' + state.args.domino, 'placing');
              this.addActionButton('domino_rotate_button', _('Rotate'), 'onDominoRotateButton');
              this.drag = new ebg.draggable();
              this.drag.create(this, 'domino_' + state.args.domino);
              this.drag.onEndDragging = () => this.onDominoDrop();
              this.drag.onDragging = (...args) => this.onDominoDrag(...args);
            }
          }
          break;
        case 'placeTreasure':
          if (this.isCurrentPlayerActive()) {
            this.enterPlaceTreasure(state.args);
          }
          break;
      }
    },
    // onLeavingState: this method is called each time we are leaving a game state.
    // You can use this method to perform some user interface changes at this moment.
    //
    onLeavingState: function (stateName) {
      switch (stateName) {
        case 'chooseDomino':
          if (this.isCurrentPlayerActive()) {
            this.removeFutureDominoHighlight();
            dojo.query('.domino.selected').forEach(node => dojo.removeClass(node, 'selected'));
          }
          break;
        case 'placeDomino':
          if (this.isCurrentPlayerActive()) {
            dojo.query('.manipulation-arrow').forEach(dojo.destroy);
            dojo.query('.square').forEach(dojo.destroy);
            if ($('domino_' + this.gamedatas.gamestate.args.domino)) {
              dojo.removeClass('domino_' + this.gamedatas.gamestate.args.domino, 'placing');
              dojo.removeClass('domino_' + this.gamedatas.gamestate.args.domino, 'preview');
            }
            if (this.drag) {
              this.drag.destroy();
            }
            delete this.selectedPosition;
            delete this.draggableDomino;
            dojo.destroy('discard_button_mask');
            dojo.query('.domino.selected').forEach(node => dojo.removeClass(node, 'selected'));
          }
          break;
        case 'placeTreasure':
          if (this.isCurrentPlayerActive()) {
            this.leavePlaceTreasure();
          }
          break;
      }
      for (var i = 0; i < this.stateHandles.length; i++) {
        dojo.disconnect(this.stateHandles[i]);
      }
    },
    // onUpdateActionButtons: in this method you can manage "action buttons" that are displayed in the
    // action status bar (ie: the HTML links in the status bar).
    //
    onUpdateActionButtons: function (stateName, args) {
    },

    // /////////////////////////////////////////////////
    // // Utility methods
    /*
     *
     * Here, you can defines some utility methods that you can use everywhere in your javascript script.
     *
     */

      
    onGameUserPreferenceChanged: function (prefId, prefValue) {
      switch (prefId) {
        case 201: // edition
          document.getElementsByTagName('html')[0].dataset.edition = prefValue;
          break;
      }
    },

    prepareBestPlacementSpots: function (kingdom, limits) {
      var gridSize = this.gamedatas.gridSize;
      this.bestPlacements = {minX: gridSize, minY: gridSize, maxX: -gridSize, maxY: -gridSize};
      for (var x = limits.xMin; x <= limits.xMax; x++) {
        for (var y = limits.yMin; y <= limits.yMax; y++) {
          if (!kingdom[x][y]) {
            var bestPlacements = this.findBestPlacementsForSquare(x, y, kingdom, limits);
            if (Object.keys(bestPlacements).length) {
              var square = this.format_block('jstpl_square', {x: x, y: y, left: x * 100, top: -y * 100})
              dojo.place(square, $('map_scrollable_oversurface'));
              this.stateHandles.push(dojo.connect($('square_' + x + '_' + y), 'onclick', this, 'onSquareClick'));
              this.stateHandles.push(dojo.connect($('square_' + x + '_' + y), 'onmouseenter', this, 'onSquareMouseEnter'));
              this.stateHandles.push(dojo.connect($('square_' + x + '_' + y), 'onmouseleave', this, 'onSquareMouseLeave'));
              if (!this.bestPlacements[x]) {
                this.bestPlacements[x] = {};
              }
              this.bestPlacements[x][y] = bestPlacements;
              this.bestPlacements.minX = Math.min(x, this.bestPlacements.minX);
              this.bestPlacements.minY = Math.min(y, this.bestPlacements.minY);
              this.bestPlacements.maxX = Math.max(x, this.bestPlacements.maxX);
              this.bestPlacements.maxY = Math.max(y, this.bestPlacements.maxY);
            }
          }
        }
      }
      // If all best placements are out of sight, move Kingdom map so that the player don't miss it
      // Fixes https://boardgamearena.com/bug?id=13156
      if (Object.keys(this.bestPlacements).length === 4) {
          return; // No adjustment needed if domino cannot be placed https://boardgamearena.com/bug?id=13743
      }
      if (this.bestPlacements.minX > this.kingdomMapPosition[0] + (gridSize - 1) / 2) {
        this.onMoveRight();
      } else if (this.bestPlacements.maxX < this.kingdomMapPosition[0] - (gridSize - 1) / 2) {
        this.onMoveLeft();
      }
      if (this.bestPlacements.minY > this.kingdomMapPosition[1] + (gridSize - 1) / 2) {
        this.onMoveTop();
      } else if (this.bestPlacements.maxY < this.kingdomMapPosition[1] - (gridSize - 1) / 2) {
        this.onMoveDown();
      }
    },

    findBestPlacementsForSquare: function (x, y, kingdom, limits) {
      // There are 8 ways to place a domino on a square, let's find the best way.
      var dominoDescription = this.gamedatas.dominoesDescription[this.gamedatas.gamestate.args.domino];
      var score;
      var bestPlacement = null;
      var bestPlacements = {};
      var hasAdjacentTerrainForLeftSquare = this.hasSameAdjacentTerrain(kingdom, x, y, dominoDescription.left.terrain)
      var hasAdjacentTerrainForRightSquare = this.hasSameAdjacentTerrain(kingdom, x, y, dominoDescription.right.terrain)
      if (x + 1 <= limits.xMax && !kingdom[x + 1][y]) {
        if (hasAdjacentTerrainForLeftSquare || this.hasSameAdjacentTerrain(kingdom, x + 1, y, dominoDescription.right.terrain)) {
          bestPlacements[0] = {x: x, y: y, rotation: 0};
          bestPlacement = bestPlacements[0];
        }
        if (hasAdjacentTerrainForRightSquare || this.hasSameAdjacentTerrain(kingdom, x + 1, y, dominoDescription.left.terrain)) {
          bestPlacements[2] = {x: x + 1, y: y, rotation: 2};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[2];
          }
        }
      }
      if (y + 1 <= limits.yMax && !kingdom[x][y + 1]) {
        if (hasAdjacentTerrainForLeftSquare || this.hasSameAdjacentTerrain(kingdom, x, y + 1, dominoDescription.right.terrain)) {
          bestPlacements[3] = {x: x, y: y, rotation: 3};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[3];
          }
        }
        if (hasAdjacentTerrainForRightSquare || this.hasSameAdjacentTerrain(kingdom, x, y + 1, dominoDescription.left.terrain)) {
          bestPlacements[1] = {x: x, y: y + 1, rotation: 1};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[1];
          }
        }
      }
      if (x - 1 >= limits.xMin && !kingdom[x - 1][y]) {
        if (!bestPlacements[2] && (hasAdjacentTerrainForLeftSquare || this.hasSameAdjacentTerrain(kingdom, x - 1, y, dominoDescription.right.terrain))) {
          bestPlacements[2] = {x: x, y: y, rotation: 2};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[2];
          }
        }
        if (!bestPlacements[0] && (hasAdjacentTerrainForRightSquare || this.hasSameAdjacentTerrain(kingdom, x - 1, y, dominoDescription.left.terrain))) {
          bestPlacements[0] = {x: x - 1, y: y, rotation: 0};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[0];
          }
        }
      }
      if (y - 1 >= limits.yMin && !kingdom[x][y - 1]) {
        if (!bestPlacements[1] && (hasAdjacentTerrainForLeftSquare || this.hasSameAdjacentTerrain(kingdom, x, y - 1, dominoDescription.right.terrain))) {
          bestPlacements[1] = {x: x, y: y, rotation: 1};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[1];
          }
        }
        if (!bestPlacements[3] && (hasAdjacentTerrainForRightSquare || this.hasSameAdjacentTerrain(kingdom, x, y - 1, dominoDescription.left.terrain))) {
          bestPlacements[3] = {x: x, y: y - 1, rotation: 3};
          if (!bestPlacement) {
            bestPlacement = bestPlacements[3];
          }
        }
      }
      if (bestPlacement) {
        bestPlacements.fallback = bestPlacement;
      }
      return bestPlacements;
    },

    hasSameAdjacentTerrain: function (kingdom, x, y, terrain) {
      return kingdom[x - 1][y] && (kingdom[x - 1][y].terrain === 'Castle' || kingdom[x - 1][y].terrain === terrain)
        || kingdom[x][y - 1] && (kingdom[x][y - 1].terrain === 'Castle' || kingdom[x][y - 1].terrain === terrain)
        || kingdom[x + 1][y] && (kingdom[x + 1][y].terrain === 'Castle' || kingdom[x + 1][y].terrain === terrain)
        || kingdom[x][y + 1] && (kingdom[x][y + 1].terrain === 'Castle' || kingdom[x][y + 1].terrain === terrain);
    },

    previewDominoPlace: function (number, position) {
      this.orientDomino(number, position.rotation, true);
      switch (position.rotation) {
        case 1:
          this.placeOnObjectPos('domino_' + number, 'square_' + position.x + '_' + position.y, 0, 50)
          break;
        case 2:
          this.placeOnObjectPos('domino_' + number, 'square_' + position.x + '_' + position.y, -50, 0)
          break;
        case 3:
          this.placeOnObjectPos('domino_' + number, 'square_' + position.x + '_' + position.y, 0, -50)
          break;
        default:
          this.placeOnObjectPos('domino_' + number, 'square_' + position.x + '_' + position.y, 50, 0)
          break;
      }
    },

    createDomino: function (number) {
      var x = -(Math.floor((number - 1) / 10) * 207 + 3);
      var y = -((number - 1) % 10 * 109 + 4);
      dojo.place(this.format_block('jstpl_domino', {number: number, x: x, y: y}), $('thething'));
    },

    placeDomino: function (number, position, favorClockwise) {
      this.orientDomino(number, position.rotation, favorClockwise);
      switch (parseInt(position.rotation)) {
        case 1:
          dojo.style('domino_' + number, 'left', position.x * 100 - 50 + 'px');
          dojo.style('domino_' + number, 'top', -position.y * 100 + 50 + 'px');
          break;
        case 2:
          dojo.style('domino_' + number, 'left', position.x * 100 - 100 + 'px');
          dojo.style('domino_' + number, 'top', -position.y * 100 + 'px');
          break;
        case 3:
          dojo.style('domino_' + number, 'left', position.x * 100 - 50 + 'px');
          dojo.style('domino_' + number, 'top', -position.y * 100 - 50 + 'px');
          break;
        default:
          dojo.style('domino_' + number, 'left', position.x * 100 + 'px');
          dojo.style('domino_' + number, 'top', -position.y * 100 + 'px');
          break;
      }
    },

    orientDomino: function (number, rotation, favorClockwise) {
      var dominoStyle = '';
      dojo.query('#domino_' + number + ' .domino-background').forEach(function (node) {
        dominoStyle = dojo.attr(node, 'style') || '';
      });
      var currentDominoRotation = dominoStyle.indexOf('rotate(') === -1 ? 0 : parseInt(dominoStyle.split('rotate(')[1].split('deg)')[0]);
      var rotationDelta = (rotation * 90 - currentDominoRotation) % 360;
      dojo.removeClass('domino_' + number, 'shadow-rotation-0');
      dojo.removeClass('domino_' + number, 'shadow-rotation-1');
      dojo.removeClass('domino_' + number, 'shadow-rotation-2');
      dojo.removeClass('domino_' + number, 'shadow-rotation-3');
      dojo.addClass('domino_' + number, 'shadow-rotation-' + rotation);
      if (rotationDelta === -270) {
        rotationDelta = 90;
      } else if (rotationDelta === 270) {
        rotationDelta = -90;
      } else if (rotationDelta === 180 && currentDominoRotation > 0 && !favorClockwise) {
        rotationDelta = -180
      } else if (rotationDelta === -180 && (currentDominoRotation < 0 || favorClockwise)) {
        rotationDelta = 180
      }
      currentDominoRotation += rotationDelta;
      dojo.query('#domino_' + number + ' > *').forEach(function (node) {
        dojo.style(node, 'transform', 'rotate(' + currentDominoRotation + 'deg)');
      });
    },

    placeOtherPlayerDomino: function (number, domino, owner, hidden) {
      var player_id = owner || domino.owner_player;
      var left = domino.x * 100;
      var top = -domino.y * 100;
      var rotation = parseInt(domino.rotation)
      switch (rotation) {
        case 1:
          left -= 50;
          top += 50;
          break;
        case 2:
          left -= 100;
          break;
        case 3:
          left -= 50;
          top -= 50;
          break;
      }
      var args = {
        number: number + '_placed',
        x: -(Math.floor((number - 1) / 10) * 207 + 3),
        y: -((number - 1) % 10 * 109 + 4)
      }
      dojo.place(this.format_block('jstpl_domino', args), $('kingdom_' + player_id));
      var dominoId = 'domino_' + number + '_placed';
      dojo.style(dominoId, 'left', left + 'px');
      dojo.style(dominoId, 'top', top + 'px');
      dojo.query('#' + dominoId + ' > *').forEach(function (node) {
        dojo.style(node, 'transform', 'rotate(' + domino.rotation * 90 + 'deg)');
      });
      dojo.addClass(dominoId, 'shadow-rotation-' + domino.rotation);
      if (hidden) {
        dojo.addClass(dominoId, 'hidden');
      }
    },

    centerOtherKingdom: function (player_id) {
      var kingdom = this.gamedatas.players[player_id].kingdom;
      var gridSize = this.gamedatas.gridSize;
      dojo.style('kingdom_' + player_id, 'left', 80 * (gridSize - kingdom.minX - kingdom.maxX - 1) / 2 + 'px');
      dojo.style('kingdom_' + player_id, 'top', 80 * (gridSize + kingdom.minY + kingdom.maxY - 1) / 2 + 'px');
    },

    updateManipulationArrows: function (position) {
      dojo.addClass(dojo.query('.rotate-left')[0], 'hidden');
      dojo.addClass(dojo.query('.rotate-right')[0], 'hidden');
      /*var rightTerrainCoordinates = this.getRightTerrainCoordinates(position);
      this.updateManipulationArrow(dojo.query('.rotate-left')[0], position, rightTerrainCoordinates);
      this.updateManipulationArrow(dojo.query('.rotate-right')[0], rightTerrainCoordinates, position);*/
    },

    updateManipulationArrow: function (arrow, coordinates, otherCoordinates) {
      if (this.firstClockwiseRotationAvailable(coordinates, otherCoordinates) || this.secondClockwiseRotationAvailable(coordinates, otherCoordinates)) {
        dojo.removeClass(arrow, 'anticlockwise');
        dojo.removeClass(arrow, 'hidden');
      } else if (this.anticlockwiseRotationAvailable(coordinates, otherCoordinates)) {
        dojo.addClass(arrow, 'anticlockwise');
        dojo.removeClass(arrow, 'hidden');
      } else {
        dojo.addClass(arrow, 'hidden');
      }
    },

    firstClockwiseRotationAvailable: function (arrowCoordinates, otherCoordinates) {
      var x = arrowCoordinates.x + otherCoordinates.y - arrowCoordinates.y;
      var y = arrowCoordinates.y + arrowCoordinates.x - otherCoordinates.x;
      return Math.abs(x) < 7 && Math.abs(y) < 7 && $('square_' + x + '_' + y) != null;
    },

    secondClockwiseRotationAvailable: function (arrowCoordinates, otherCoordinates) {
      var x = 2 * arrowCoordinates.x - otherCoordinates.x;
      var y = 2 * arrowCoordinates.y - otherCoordinates.y;
      return Math.abs(x) < 7 && Math.abs(y) < 7 && $('square_' + x + '_' + y) != null;
    },

    anticlockwiseRotationAvailable: function (arrowCoordinates, otherCoordinates) {
      var x = arrowCoordinates.x + arrowCoordinates.y - otherCoordinates.y;
      var y = arrowCoordinates.y + otherCoordinates.x - arrowCoordinates.x;
      return Math.abs(x) < 7 && Math.abs(y) < 7 && $('square_' + x + '_' + y) != null;
    },

    getRightTerrainCoordinates: function (position) {
      switch (position.rotation) {
        case 0:
          return {x: position.x + 1, y: position.y};
        case 1:
          return {x: position.x, y: position.y - 1};
        case 2:
          return {x: position.x - 1, y: position.y};
        default:
          return {x: position.x, y: position.y + 1};
      }
    },

    highlightSelectableDominoes: function (updateTitle = true) {
      var dominoAvailable = false;
      for (var number in this.gamedatas.dominoes) {
        if (this.gamedatas.dominoes.hasOwnProperty(number)) {
          var domino = this.gamedatas.dominoes[number];
          if (!domino.owner_player && domino.location !== 'DISCARD') {
            dojo.addClass('domino_' + number, 'selectable');
            dominoAvailable = true;
          }
        }
      }

      if (updateTitle) {
          $("pagemaintitletext").innerHTML = _("${you} must choose a domino").replace('${you}', this.divYou());
      }
      return dominoAvailable;
    },

    // Dominoes still available to choose in the chooseDomino state: the unclaimed draft.
    choosableDominoes: function () {
      const result = [];
      for (const number in this.gamedatas.dominoes) {
        const domino = this.gamedatas.dominoes[number];
        if (domino.location === 'FUTURE' && !domino.owner_player) {
          result.push(number);
        }
      }
      return result;
    },

    removeFutureDominoHighlight: function () {
      for (var number in this.gamedatas.dominoes) {
        if (this.gamedatas.dominoes.hasOwnProperty(number)) {
          var domino = this.gamedatas.dominoes[number];
          if (domino.location === 'FUTURE')
            dojo.removeClass('domino_' + number, 'selectable');
        }
      }
    },

    updateDominoesRemaining: function () {
      var dominoesRemaining = this.gamedatas.turnsLeft > 1 ? (this.gamedatas.turnsLeft - 1) * 4 : 0;
      var text;
      switch (this.gamedatas.turnsLeft) {
        case 0:
          text = _("Last turn!");
          break;
        case 1:
          text = _("${quantity} dominoes remaining (1 turn left)");
          break;
        default:
          text = _("${quantity} dominoes remaining (${number} turns left)");
      }
      $("dominoes_remaining").innerHTML = dojo.string.substitute(text, {quantity: dominoesRemaining, number: this.gamedatas.turnsLeft});
    },

    prepareAdditionalRuleInfo: function (buttonId, title, description) {
      dojo.connect($(buttonId), 'onclick', this, function (event) {
        dojo.stopEvent(event);
        var dialog = new ebg.popindialog();
        dialog.create(buttonId + 'Info');
        dialog.setTitle(title);
        dialog.setMaxWidth(500);
        dialog.setContent(description);
        dialog.show();
      });
    },

    replaceAvailableDominoes: function () {
      this.resizeToDo = true;
      setTimeout(() => {
        if (this.resizeToDo) {
          this.doReplaceAvailableDominoes();
          this.resizeToDo = false;
        }
      }, 100);
    },

    doReplaceAvailableDominoes: function () {
      var current_domino_space = this.gamedatas.gamestate && this.gamedatas.gamestate.args && this.gamedatas.gamestate.args.currentPosition ? this.gamedatas.gamestate.args.currentPosition : 1;
      var future_domino_space = 1;
      for (var number in this.gamedatas.dominoes) {
        if (this.gamedatas.dominoes.hasOwnProperty(number)) {
          var domino = this.gamedatas.dominoes[number];
          if (domino.location !== 'KINGDOM') {
            var location = domino.location === 'CURRENT' ? 'current_domino_space_' + current_domino_space : 'future_domino_space_' + future_domino_space;
            if ($('domino_' + number)) {
              if (number !== this.draggableDomino || !dojo.query('#kingdom #domino_' + this.draggableDomino).length) {
                this.placeOnObject('domino_' + number, location);
              }
              if (domino.location === 'CURRENT') {
                current_domino_space++;
              } else if (domino.location === 'FUTURE') {
                future_domino_space++;
              }
            }
            if (domino.owner_player) {
              var king = dojo.query('.king[data-value="' + number + '"]')[0];
              if (king) {
                this.placeOnObject(king.id, location);
              }
            }
          }
        }
      }
      if ($('discard_button_mask')) {
        setTimeout(() => {
          this.placeOnObject('discard_button_mask', 'domino_' + this.gamedatas.gamestate.args.domino);
        }, 1000);
      }
    },

    createKing: function (color, domino, playerId) {
      var id = 'king_' + this.kingId;
      var king = this.format_block('jstpl_king', {color: color, domino: domino, id: id})
      dojo.place(king, $('thething'));
      if (playerId) {
        this.placeOnObject(id, 'player_board_' + playerId);
      }
      this.placeOnObject(id, 'domino_' + domino);
      this.kingId++;
      this.kingDraggable[id] = new ebg.draggable();
      this.kingDraggable[id].create(this, id);
      this.kingDraggable[id].onDragging = () => this.onKingDragging();
      this.kingDraggable[id].onEndDragging = () => this.onKingDrop();
      this.kingDraggable[id].disable();
    },

    // /////////////////////////////////////////////////
    // // Lost Treasures: discovery placement

    enterPlaceTreasure: function (args) {
      var self = this;
      this.treasurePlacement = {
        supply: args.supply || [],
        intersections: args.intersections || [],
        faces: args.faces || {},
        selectedKey: null,
        selectedIntersection: null,
        rotation: 0
      };

      // Same bound ref so it can be removed in leavePlaceTreasure (gems persist).
      this.treasureSupplyClickHandler = this.onTreasureSupplyClick.bind(this);
      document.querySelectorAll('#treasure_supply_gems .treasure').forEach(function (node) {
        node.classList.add('selectable');
        node.addEventListener('click', self.treasureSupplyClickHandler);
      });

      var container = this.getTreasureKingdomContainer(this.player_id);
      for (var i = 0; i < this.treasurePlacement.intersections.length; i++) {
        var cell = this.treasurePlacement.intersections[i];
        var pixel = this.getTreasureIntersectionPixel(parseInt(cell.x), parseInt(cell.y));
        $(container).insertAdjacentHTML('beforeend', this.format_block('jstpl_treasure_intersection', {
          x: cell.x, y: cell.y, left: pixel.left, top: pixel.top
        }));
        var markerId = 'treasure_intersection_' + cell.x + '_' + cell.y;
        // Markers are destroyed in leavePlaceTreasure, so listeners drop with them.
        $(markerId).addEventListener('click', this.onTreasureIntersectionClick.bind(this));
        $(markerId).addEventListener('mouseenter', this.onTreasureIntersectionEnter.bind(this));
        $(markerId).addEventListener('mouseleave', this.onTreasureIntersectionLeave.bind(this));
      }

      // Auto-select the first gem so the player can immediately preview/place.
      if (this.treasurePlacement.supply.length > 0) {
        this.selectTreasureGem(this.treasurePlacement.supply[0].key);
      }

      // Rotate from the action bar before any intersection is chosen (mirrors tiles).
      if (this.treasurePlacement.selectedKey) {
        this.addActionButton('treasure_rotate_button', _('Rotate'), 'onTreasureRotateArrowClick');
      }
    },

    leavePlaceTreasure: function () {
      var self = this;
      document.querySelectorAll('.treasure-intersection').forEach(function (node) { node.remove(); });
      document.querySelectorAll('.placed-treasure.treasure-preview').forEach(function (node) { node.remove(); });
      document.querySelectorAll('.treasure-rotate-arrow').forEach(function (node) { node.remove(); });
      document.querySelectorAll('#treasure_supply_gems .treasure').forEach(function (node) {
        node.classList.remove('selectable');
        node.classList.remove('selected');
        node.style.transform = '';
        if (self.treasureSupplyClickHandler) {
          node.removeEventListener('click', self.treasureSupplyClickHandler);
        }
      });
      this.treasureSupplyClickHandler = null;
      $('treasure_place_button')?.remove();
      $('treasure_rotate_button')?.remove();
      delete this.treasurePlacement;
    },

    onTreasureSupplyClick: function (event) {
      event.preventDefault();
      event.stopPropagation();
      if (!this.treasurePlacement) {
        return;
      }
      var tokenKey = event.currentTarget.id;
      if (this.treasurePlacement.selectedKey !== tokenKey) {
        this.selectTreasureGem(tokenKey);
      } else {
        // Re-clicking the selected gem rotates it in place.
        this.rotateSelectedTreasureGem();
      }
    },

    selectTreasureGem: function (tokenKey) {
      this.treasurePlacement.selectedKey = tokenKey;
      this.treasurePlacement.rotation = 0;
      document.querySelectorAll('#treasure_supply_gems .treasure').forEach(function (node) {
        node.classList.remove('selected');
        node.style.transform = '';
      });
      document.querySelectorAll('#treasure_supply_gems .treasure-rotate-arrow').forEach(function (node) { node.remove(); });
      if ($(tokenKey)) {
        $(tokenKey).classList.add('selected');
        this.addTreasureRotateArrow(tokenKey); // rotate icon on the staged gem, like the domino
      }
      this.refreshTreasureSupplyVisual();
      this.refreshTreasurePreview();
    },

    // Place the shared rotate icon inside a gem element and wire it to rotation.
    addTreasureRotateArrow: function (parentNode) {
      var parent = (typeof parentNode === 'string') ? $(parentNode) : parentNode;
      parent.insertAdjacentHTML('beforeend', this.format_block('jstpl_treasure_rotate_arrow', {}));
      var arrow = parent.lastElementChild;
      // Arrow lives inside a gem; it is destroyed before re-add, so listeners drop with it.
      arrow.addEventListener('click', this.onTreasureRotateArrowClick.bind(this));
      arrow.addEventListener('touchend', this.onTreasureRotateArrowClick.bind(this));
      return arrow;
    },

    onTreasureRotateArrowClick: function (event) {
      if (event) {
        event.preventDefault();
        event.stopPropagation();
      }
      if (!this.treasurePlacement || !this.treasurePlacement.selectedKey) {
        return;
      }
      this.rotateSelectedTreasureGem();
    },

    rotateSelectedTreasureGem: function () {
      this.treasurePlacement.rotation = (this.treasurePlacement.rotation + 1) % 4;
      this.refreshTreasureSupplyVisual();
      this.refreshTreasurePreview();
    },

    refreshTreasureSupplyVisual: function () {
      var key = this.treasurePlacement.selectedKey;
      if (key && $(key)) {
        var angle = this.treasurePlacement.rotation * 90;
        $(key).style.transform = 'rotate(' + angle + 'deg)';
        // Counter-rotate the icon so its glyph stays upright as the gem spins.
        document.querySelectorAll('#' + key + ' .treasure-rotate-arrow').forEach(function (node) {
          node.style.transform = 'rotate(' + (-angle) + 'deg)';
        });
      }
    },

    onTreasureIntersectionEnter: function (event) {
      if (!this.treasurePlacement || this.treasurePlacement.selectedIntersection) {
        return;
      }
      var cell = this.parseTreasureIntersectionId(event.currentTarget.id);
      this.showTreasurePreview(cell.x, cell.y);
    },

    onTreasureIntersectionLeave: function () {
      if (!this.treasurePlacement || this.treasurePlacement.selectedIntersection) {
        return;
      }
      document.querySelectorAll('.placed-treasure.treasure-preview').forEach(function (node) { node.remove(); });
    },

    onTreasureIntersectionClick: function (event) {
      event.preventDefault();
      event.stopPropagation();
      if (!this.treasurePlacement || !this.treasurePlacement.selectedKey) {
        this.showMessage(_('Please choose an available gem first'), 'info');
        return;
      }
      var cell = this.parseTreasureIntersectionId(event.currentTarget.id);
      this.treasurePlacement.selectedIntersection = cell;
      document.querySelectorAll('.treasure-intersection').forEach(function (node) {
        node.classList.remove('selected');
      });
      event.currentTarget.classList.add('selected');
      this.showTreasurePreview(cell.x, cell.y);

      // Confirm step so the gem is not placed on a stray click; Cancel un-picks
      // the spot so a different intersection/rotation can be chosen.
      $('treasure_place_button')?.remove();
      this.addActionButton('treasure_place_button', _('Place Gem'), 'onTreasurePlaceButton');
      this.addCancelButton(undefined,  ()=> this.onTreasureCancelButton());
    },

    onTreasureCancelButton: function () {
      this.restoreServerGameState();
      if (!this.treasurePlacement) {
        return;
      }
      this.treasurePlacement.selectedIntersection = null;
      document.querySelectorAll('.treasure-intersection').forEach(function (node) {
        node.classList.remove('selected');
      });
      document.querySelectorAll('.placed-treasure.treasure-preview').forEach(function (node) { node.remove(); });
    },

    onTreasurePlaceButton: function () {
      if (!this.treasurePlacement || !this.treasurePlacement.selectedKey || !this.treasurePlacement.selectedIntersection) {
        return;
      }
      if (!this.checkAction('placeTreasure')) {
        return;
      }
      var cell = this.treasurePlacement.selectedIntersection;
      this.bga.actions.performAction('placeTreasure', {
        tokenKey: this.treasurePlacement.selectedKey,
        x: cell.x,
        y: cell.y,
        rotation: this.treasurePlacement.rotation
      });
    },

    refreshTreasurePreview: function () {
      var cell = this.treasurePlacement.selectedIntersection;
      if (cell) {
        this.showTreasurePreview(cell.x, cell.y);
      }
    },

    showTreasurePreview: function (x, y) {
      if (!this.treasurePlacement || !this.treasurePlacement.selectedKey) {
        return;
      }
      document.querySelectorAll('.placed-treasure.treasure-preview').forEach(function (node) { node.remove(); });
      var container = this.getTreasureKingdomContainer(this.player_id);
      var pixel = this.getTreasureIntersectionPixel(x, y);
      $(container).insertAdjacentHTML('beforeend', this.format_block('jstpl_placed_treasure', {
        id: 'treasure_preview',
        key: this.treasurePlacement.selectedKey,
        left: pixel.left,
        top: pixel.top,
        angle: this.treasurePlacement.rotation * 90
      }));
      $('treasure_preview').classList.add('treasure-preview');
    },

    parseTreasureIntersectionId: function (id) {
      var split = id.split('_');
      return {x: parseInt(split[2]), y: parseInt(split[3])};
    },

    // /////////////////////////////////////////////////
    // // Player's action
    /*
     *
     * Here, you are defining methods to handle player's action (ex: results of mouse click on game objects).
     *
     * Most of the time, these methods: _ check the action is possible at this game state. _ make a call to the game server
     *
     */

    onDominoClick: function (event) {
      dojo.stopEvent(event);
      if (!event.currentTarget.id || !event.currentTarget.id.startsWith('domino_')) {
        return;
      }
      this.selectDomino(event.currentTarget.id.substr(7));
    },

    selectDomino: function (number) {
      var domino = this.gamedatas.dominoes[number];
      if (!domino.owner_player) {
        if (this.checkPossibleActions('chooseDomino')) {
          this.bga.actions.performAction('chooseDomino', {number: number});
        } else if (this.selectedPosition) {
          this.selectedPosition.nextDomino = number;
          this.lastDominoPlaced = number;
          this.bga.actions.performAction('placeDomino', this.selectedPosition);
          this.drag.disable();
        }
      }
    },

    onKingDragging: function () {
      this.kingDragged = true;
    },

    onKingDrop: function () {
      this.kingDragged = false;
      var domino = dojo.query('.domino.drop-target')[0];
      if (domino) {
        this.selectDomino(domino.id.split('domino_')[1]);
        dojo.removeClass(domino, 'drop-target');
      } else {
        this.doReplaceAvailableDominoes();
      }
    },

    onSquareClick: function (event) {
      event.preventDefault();
      var splitId = event.currentTarget.id.split('_')
      var x = parseInt(splitId[1]);
      var y = parseInt(splitId[2]);
      var number = this.gamedatas.gamestate.args.domino
      if (number && this.bestPlacements[x] && this.bestPlacements[x][y]) {
        dojo.addClass('domino_' + number, 'preview');
        var bestPlacements = this.bestPlacements[x][y];
        var bestPlacement = bestPlacements[this.preferredOrientation] || bestPlacements.fallback;
        this.selectDominoPlace(number, bestPlacement);
        this.updateManipulationArrows(bestPlacement);
      }
    },

    onSquareMouseEnter: function (event) {
      if (!this.dominoDragged) {
        var splitId = event.target.id.split('_');
        var bestPlacements = this.bestPlacements[splitId[1]][splitId[2]];
        var bestPlacement = bestPlacements[this.preferredOrientation] || bestPlacements.fallback;
        var number = this.gamedatas.gamestate.args.domino;
        var x = -(Math.floor((number - 1) / 10) * 207 + 3);
        var y = -((number - 1) % 10 * 109 + 4);
        var dominoPreview = this.format_block('jstpl_domino', {number: 'preview', x: x, y: y})
        dojo.place(dominoPreview, event.target);
        dojo.addClass('domino_preview', 'preview');
        var position = { ...bestPlacement };
        position.x -= splitId[1];
        position.y -= splitId[2];
        this.placeDomino('preview', position);
      }
    },

    onSquareMouseLeave: function () {
      if (!this.dominoDragged) {
        dojo.destroy('domino_preview');
      }
    },

    onDominoDrag: function (domino_id, left, top, dx, dy) {
      if (!this.dominoDragged) {
        var number = this.gamedatas.gamestate.args.domino;
        if (Math.abs(dx) + Math.abs(dy) > 5) {
          this.dominoDragged = true;
          dojo.addClass('domino_' + number, 'dragging');
        }
      }
    },

    onMouseMove: function (event) {
      if (this.dominoDragged) {
        var square = document.elementFromPoint(event.clientX, event.clientY);
        var number = this.gamedatas.gamestate.args.domino;
        if (square && square.id.startsWith('square_')) {
          var splitId = square.id.split('_');
          var bestPlacements = this.bestPlacements[splitId[1]][splitId[2]];
          var bestPlacement = bestPlacements[this.preferredOrientation] || bestPlacements.fallback;
          dojo.addClass('domino_' + number, 'preview');
          this.preselectedPosition = bestPlacement;
          this.updateManipulationArrows(bestPlacement);
          this.previewDominoPlace(number, bestPlacement);
          dojo.stopEvent(event);
        } else {
          dojo.removeClass('domino_' + number, 'preview');
          this.orientDomino(number, this.preferredOrientation);
        }
      } else if (this.kingDragged) {
        var element = document.elementFromPoint(event.clientX, event.clientY);
        if (element && element.parentNode && element.parentNode.id.startsWith('domino_') && dojo.hasClass(element.parentNode.id, 'selectable')) {
          dojo.addClass(element.parentNode.id, 'drop-target');
        } else {
          dojo.query('.domino.drop-target').forEach(function (node) {
            dojo.removeClass(node, 'drop-target');
          });
        }
      }
    },

    onDominoDrop: function () {
      if (!this.dominoDragged) {
        return;
      }
      var number = this.gamedatas.gamestate.args.domino;
      this.dominoDragged = false;
      dojo.removeClass('domino_' + number, 'dragging');
      if (this.preselectedPosition) {
        this.selectDominoPlace(number, this.preselectedPosition);
      } else if (this.selectedPosition) {
        this.placeDomino(number, this.selectedPosition);
      } else {
        this.doReplaceAvailableDominoes();
      }
    },

    selectDominoPlace: function (number, selectedPosition) {
      if (!this.selectedPosition) {
        this.addCancelButton();
        dojo.destroy('domino_rotate_button'); // rotate before placing; Cancel restores it
        dojo.place('domino_' + number, 'map_scrollable_oversurface');
        this.highlightSelectableDominoes(false);
        var king = dojo.query('.king[data-value="' + this.gamedatas.gamestate.args.domino + '"]')[0];
        this.kingDraggable[king.id].enable();
      }
      this.placeDomino(number, selectedPosition);
      this.selectedPosition = { ...selectedPosition };
      this.moveKingdomIfNecessary(selectedPosition);
      this.refreshPlacementPreview();
    },

    // Update the staged placement's live score and treasure-discovery flag from state
    // args (no server call): the server precomputes both for every valid move
    // (args.placementPreviews). A discovery is irreversible and routes to placeTreasure,
    // so while a treasure would be discovered the future dominoes are not offered as a
    // one-click commit; the player confirms it with the dedicated commit button instead.
    refreshPlacementPreview: function () {
      if (!this.selectedPosition) {
        return;
      }
      const preview = this.placementPreviewFor(this.selectedPosition);
      if (preview) {
        this.scoreCtrl[this.player_id].toValue(preview.score);
      }
      this.placementDiscoversTreasure = !!preview?.discoversTreasure;

      if (this.placementDiscoversTreasure) {
        this.removeFutureDominoHighlight();
        $("pagemaintitletext").innerHTML = dojo.string.substitute(
          _("${you} will discover a treasure here - this placement cannot be undone"), {you: this.divYou()});
      } else {
        this.highlightSelectableDominoes();
      }
      this.updateCommitButton();
    },

    placementPreviewFor: function (position) {
      const previews = this.gamedatas.gamestate.args.placementPreviews || [];
      return previews.find(p => p.x === position.x && p.y === position.y && p.rotation === position.rotation);
    },

    divYou: function () {
       return '<span style="font-weight:bold;color:#' + this.gamedatas.players[this.player_id].color + ';">' + _('You') + '</span>';
    },

    // Show an explicit commit button only when the staged placement cannot be committed
    // by simply choosing the next domino: a treasure discovery (irreversible, routes to
    // placeTreasure) or the final domino of the game (no next domino to pick). Otherwise
    // the placement commits when the player picks their next domino, so no button.
    updateCommitButton: function () {
      $("button_commit")?.remove();
      dojo.query('.domino.selected').forEach(node => dojo.removeClass(node, 'selected'));
      const currentNumber = this.gamedatas.gamestate.args.domino;
      const king = dojo.query('.king[data-value="' + currentNumber + '"]')[0];
      if (this.placementDiscoversTreasure) {
        // Discovery commits standalone (no next domino) and routes into placeTreasure.
        this.placeKingOnDomino(king, currentNumber);
        this.bga.statusBar.addActionButton(_("Confirm"), () => this.commitPlacement(), { id: "button_commit" });
        this.addCancelButton();
      } else if (this.gamedatas.turnsLeft === 0) {
        // Final domino: no next domino to pick, so the placement is committed standalone.
        this.bga.statusBar.addActionButton(_("Confirm"), () => this.commitPlacement(), { id: "button_commit" });
      } else {
        const choosable = this.choosableDominoes();
        if (choosable.length === 1) {
          // No real choice left for the next domino: pre-select it (highlight + move the
          // king onto it, as a drag would) and just ask to confirm.
          const number = choosable[0];
          dojo.addClass('domino_' + number, 'selected');
          this.placeKingOnDomino(king, number);
          this.bga.statusBar.addActionButton(_("Confirm"), () => this.selectDomino(number), { id: "button_commit" });
        }
      }
    },

    placeKingOnDomino: function (king, number) {
      if (king) {
        this.placeOnObject(king.id, 'domino_' + number);
      }
    },

    // Commit the staged placement standalone (no next domino chosen).
    commitPlacement: function () {
      this.lastDominoPlaced = this.gamedatas.gamestate.args.domino;
      this.bga.actions.performAction('placeDomino', this.selectedPosition);
    },

    // All tiles must be visible
    moveKingdomIfNecessary: function (selectedPosition) {
      var gridSize = this.gamedatas.gridSize;
      var minX = selectedPosition ? Math.min(this.kingdom.minX, selectedPosition.rotation === 2 ? selectedPosition.x - 1 : selectedPosition.x) : this.kingdom.minX;
      var minY = selectedPosition ? Math.min(this.kingdom.minY, selectedPosition.rotation === 1 ? selectedPosition.y - 1 : selectedPosition.y) : this.kingdom.minY;
      var maxX = selectedPosition ? Math.max(this.kingdom.maxX, selectedPosition.rotation === 0 ? selectedPosition.x + 1 : selectedPosition.x) : this.kingdom.maxX;
      var maxY = selectedPosition ? Math.max(this.kingdom.maxY, selectedPosition.rotation === 3 ? selectedPosition.y + 1 : selectedPosition.y) : this.kingdom.maxY;
      while (minX < this.kingdomMapPosition[0] - (gridSize - 1) / 2) {
        this.onMoveLeft();
      }
      while (maxX > this.kingdomMapPosition[0] + (gridSize - 1) / 2) {
        this.onMoveRight();
      }
      while (minY < this.kingdomMapPosition[1] - (gridSize - 1) / 2) {
        this.onMoveDown();
      }
      while (maxY > this.kingdomMapPosition[1] + (gridSize - 1) / 2) {
        this.onMoveTop();
      }
      if (this.kingdomMapPosition[0] >= minX + (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('moveright', 'hidden');
      } else {
        dojo.removeClass('moveright', 'hidden');
      }
      if (this.kingdomMapPosition[0] <= maxX - (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('moveleft', 'hidden');
      } else {
        dojo.removeClass('moveleft', 'hidden');
      }
      if (this.kingdomMapPosition[1] >= minY + (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('movetop', 'hidden');
      } else {
        dojo.removeClass('movetop', 'hidden');
      }
      if (this.kingdomMapPosition[1] <= maxY - (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('movedown', 'hidden');
      } else {
        dojo.removeClass('movedown', 'hidden');
      }
    },

    onManipulationArrowClick: function (event) {
      if (this.dominoDragged) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      var number = this.gamedatas.gamestate.args.domino;
      this.drag.onMouseUp(event);
      if (this.selectedPosition) {
        if (dojo.hasClass(event.target, 'reverse')) {
          this.selectedPosition.rotation = (this.selectedPosition.rotation + 2) % 4;
          if (this.selectedPosition.rotation === 0) {
            this.selectedPosition.x--;
          } else if (this.selectedPosition.rotation === 1) {
            this.selectedPosition.y++;
          } else if (this.selectedPosition.rotation === 2) {
            this.selectedPosition.x++;
          } else {
            this.selectedPosition.y--;
          }
          this.placeDomino(number, this.selectedPosition, true);
          this.updateManipulationArrows(this.selectedPosition);
          this.moveKingdomIfNecessary(this.selectedPosition);
          this.refreshPlacementPreview();
        } else {
          this.rotatePlacedDominoClockwise(dojo.hasClass(event.target, 'rotate-right'));
        }
      } else {
        if (dojo.hasClass(event.target, 'rotate-left')) {
          this.preferredOrientation = (this.preferredOrientation + 1) % 4;
        } else if (dojo.hasClass(event.target, 'rotate-right')) {
          this.preferredOrientation = (this.preferredOrientation + 3) % 4;
        } else {
          this.preferredOrientation = (this.preferredOrientation + 2) % 4;
        }
        this.orientDomino(number, this.preferredOrientation);
      }
    },

    // Rotate the already-placed domino around an anchor half. rotateRightTerrain picks
    // which half stays fixed (matches the rotate-right vs rotate-left arrows).
    rotatePlacedDominoClockwise: function (rotateRightTerrain) {
      var number = this.gamedatas.gamestate.args.domino;
      var arrowCoordinates, otherCoordinates;
      if (rotateRightTerrain) {
        arrowCoordinates = this.getRightTerrainCoordinates(this.selectedPosition);
        otherCoordinates = {x: this.selectedPosition.x, y: this.selectedPosition.y};
      } else {
        arrowCoordinates = {x: this.selectedPosition.x, y: this.selectedPosition.y};
        otherCoordinates = this.getRightTerrainCoordinates(this.selectedPosition);
      }
      if (this.firstClockwiseRotationAvailable(arrowCoordinates, otherCoordinates)) {
        this.selectedPosition.rotation = (this.selectedPosition.rotation + 1) % 4;
        if (rotateRightTerrain) {
          this.selectedPosition.x = arrowCoordinates.x + otherCoordinates.y - arrowCoordinates.y;
          this.selectedPosition.y = arrowCoordinates.y + arrowCoordinates.x - otherCoordinates.x;
        }
      } else if (this.secondClockwiseRotationAvailable(arrowCoordinates, otherCoordinates)) {
        this.selectedPosition.rotation = (this.selectedPosition.rotation + 2) % 4;
        if (rotateRightTerrain) {
          this.selectedPosition.x = 2 * arrowCoordinates.x - otherCoordinates.x;
          this.selectedPosition.y = 2 * arrowCoordinates.y - otherCoordinates.y;
        }
      } else {
        this.selectedPosition.rotation = (this.selectedPosition.rotation + 3) % 4;
        if (rotateRightTerrain) {
          this.selectedPosition.x = arrowCoordinates.x + arrowCoordinates.y - otherCoordinates.y;
          this.selectedPosition.y = arrowCoordinates.y + otherCoordinates.x - arrowCoordinates.x;
        }
      }
      this.placeDomino(number, this.selectedPosition, true);
      this.updateManipulationArrows(this.selectedPosition);
      this.moveKingdomIfNecessary(this.selectedPosition);
      this.refreshPlacementPreview();
    },

    // Action-bar Rotate, available before the domino is placed (the button is
    // destroyed once a position is staged; rotation then uses the on-board arrows).
    onDominoRotateButton: function () {
      this.preferredOrientation = (this.preferredOrientation + 1) % 4;
      this.orientDomino(this.gamedatas.gamestate.args.domino, this.preferredOrientation);
    },

    addCancelButton(name, handler) {
        if (!name)
            name = _("Cancel");
        if (!handler)
            handler = () => this.onCancel();
        if ($("button_cancel"))
            $("button_cancel").remove();
        this.bga.statusBar.addActionButton(name, handler, { id: "button_cancel", color: "alert" });
    },

    onCancel: function () {
      const state = this.gamedatas.gamestate;
      const number = state.args.domino;
      const dominoId = `domino_${number}`;
      const dominoElement = $(dominoId);
      $("button_commit")?.remove();
      dojo.query('.domino.selected').forEach(node => dojo.removeClass(node, 'selected'));
      this.drag.disable();
      this.restoreServerGameState();
      document.querySelectorAll('.square').forEach(node => node.remove());

      dominoElement.classList.remove('placing', 'preview');
      dominoElement.querySelectorAll(':scope > *').forEach(node => {
        node.style.transform = 'none';
      });
      document.querySelectorAll('.king.hidden').forEach(node => node.classList.remove('hidden'));
      $('thething').appendChild(dominoElement);
      this.placeOnObject(dominoId, `current_domino_space_${state.args.currentPosition}`);
      this.drag.enable();
      delete this.preselectedPosition;
      delete this.selectedPosition;
      this.placementDiscoversTreasure = false;
      this.prepareBestPlacementSpots(state.args.kingdom, state.args);
      this.orientDomino(number, this.preferredOrientation);
      dominoElement.classList.add('placing');

      const rotateRight = document.querySelector('.rotate-right');
      const rotateLeft = document.querySelector('.rotate-left');
      rotateRight.classList.add('anticlockwise');
      rotateRight.classList.remove('hidden');
      rotateLeft.classList.remove('anticlockwise', 'hidden');



      const king = document.querySelector(`.king[data-value="${number}"]`);
      this.kingDraggable[king.id].disable();
      this.placeOnObject(king.id, 'current_domino_space_' + state.args.currentPosition);
      this.removeFutureDominoHighlight();
      this.moveKingdomIfNecessary();
      this.scoreCtrl[this.player_id].toValue(this.gamedatas.players[this.player_id]['score']);
    },

    onDiscardDomino: function () {
      this.bga.actions.performAction('discardDomino');
    },

    onMoveTop: function (event) {
      if (event) {
        dojo.stopEvent(event);
      }
      this.scrollmap.scroll(0, 100);
      this.kingdomMapPosition[1]++;
      if (this.kingdomMapPosition[1] >= this.kingdom.minY + (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('movetop', 'hidden');
      }
      dojo.removeClass('movedown', 'hidden');
    },
    onMoveLeft: function (event) {
      if (event) {
        dojo.stopEvent(event);
      }
      this.scrollmap.scroll(100, 0);
      this.kingdomMapPosition[0]--;
      if (this.kingdomMapPosition[0] <= this.kingdom.maxX - (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('moveleft', 'hidden');
      }
      dojo.removeClass('moveright', 'hidden');
    },
    onMoveRight: function (event) {
      if (event) {
        dojo.stopEvent(event);
      }
      this.scrollmap.scroll(-100, 0);
      this.kingdomMapPosition[0]++;
      if (this.kingdomMapPosition[0] >= this.kingdom.minX + (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('moveright', 'hidden');
      }
      dojo.removeClass('moveleft', 'hidden');
    },
    onMoveDown: function (event) {
      if (event) {
        dojo.stopEvent(event);
      }
      this.scrollmap.scroll(0, -100);
      this.kingdomMapPosition[1]--;
      if (this.kingdomMapPosition[1] <= this.kingdom.maxY - (this.gamedatas.gridSize - 1) / 2) {
        dojo.addClass('movedown', 'hidden');
      }
      dojo.removeClass('movetop', 'hidden');
    },

    // /////////////////////////////////////////////////
    // // Reaction to cometD notifications
    /*
     * setupNotifications:
     *
     * In this method, you associate each of your game notifications with your local method to handle it.
     *
     * Note: game notification names correspond to "notifyAllPlayers" and "notifyPlayer" calls in your kingdomino.game.php file.
     *
     */
    setupNotifications: function () {
      dojo.subscribe('dominoChosen', this, 'notif_dominoChosen');
      dojo.subscribe('dominoesDrawn', this, 'notif_dominoesDrawn');
      dojo.subscribe('dominoPlaced', this, 'notif_dominoPlaced');
      dojo.subscribe('dominoDiscarded', this, 'notif_dominoDiscarded');
      dojo.subscribe('score', this, 'notif_score');
      dojo.subscribe('startPlayerScoring', this, 'notif_startPlayerScoring');
      dojo.subscribe('endPlayerScoring', this, 'notif_endPlayerScoring');
      dojo.subscribe('scoreTerrain', this, 'notif_scoreTerrain');
      dojo.subscribe('scoreTerritory', this, 'notif_scoreTerritory');
      dojo.subscribe('treasurePlaced', this, 'notif_treasurePlaced');
      dojo.subscribe('treasureRevealed', this, 'notif_treasureRevealed');
      dojo.subscribe('immediateVictory', this, 'notif_immediateVictory');
      this.notifqueue.setSynchronous('animate', 1000);
      this.notifqueue.setSynchronous('dominoesDrawn', 1500);
      this.notifqueue.setSynchronous('dominoChosen', 500);
      this.notifqueue.setSynchronous('dominoPlaced');
      this.notifqueue.setSynchronous('startPlayerScoring', 1000);
      this.notifqueue.setSynchronous('endPlayerScoring', 1000);
      this.notifqueue.setSynchronous('scoreTerrain', 1000);
    },

    notif_dominoChosen: function (notification) {
      var playerColor = this.gamedatas.players[notification.args.playerId].color
      var king = null;
      var kingId = null;
      var self = this;
      dojo.query('.king_' + playerColor).forEach(function (node) {
        var kingDomino = dojo.attr(node, 'data-value');
        var location = self.gamedatas.dominoes[kingDomino].location
        if (location === 'KINGDOM' || location === 'DISCARD') {
          king = node;
          kingId = king.id;
          dojo.attr(node, 'data-value', notification.args.number);
        }
      });
      if (!king) {
        this.createKing(playerColor, notification.args.number, notification.args.playerId);
      } else {
        this.placeOnObject(kingId, 'domino_' + notification.args.number, 500);
        this.kingDraggable[kingId].disable();
      }
      this.gamedatas.dominoes[notification.args.number].owner_player = notification.args.playerId;
    },

    notif_dominoesDrawn: function (notification) {
      var current_domino_space = 1;
      for (var number in this.gamedatas.dominoes) {
        if (this.gamedatas.dominoes.hasOwnProperty(number)) {
          var domino = this.gamedatas.dominoes[number];
          if (domino.location === 'FUTURE') {
            domino.location = 'CURRENT';
            this.placeOnObject('domino_' + number, 'current_domino_space_' + current_domino_space);
            var king = dojo.query('.king[data-value="' + number + '"]')[0];
            this.placeOnObject(king.id, 'current_domino_space_' + current_domino_space);
            current_domino_space++;
          }
        }
      }
      this.gamedatas.turnsLeft = parseInt(this.gamedatas.turnsLeft) - 1;
      this.updateDominoesRemaining();
      var self = this;
      setTimeout(function () {
        var future_domino_space = 1;
        for (var i = 0; i < notification.args.dominoes.length; i++) {
          self.gamedatas.dominoes[notification.args.dominoes[i]] = {number: notification.args.dominoes[i], location: 'FUTURE'};
          self.createDomino(notification.args.dominoes[i]);
          dojo.style('domino_' + notification.args.dominoes[i], 'top', '-1000px')
          self.placeOnObject('domino_' + notification.args.dominoes[i], 'future_domino_space_' + future_domino_space);
          future_domino_space++;
          dojo.connect($('domino_' + notification.args.dominoes[i]), 'onclick', self, 'onDominoClick');
        }
      }, 500);
    },

    notif_dominoPlaced: function (notification) {
      var position = notification.args.position
      var rotation = parseInt(position.rotation);
      var playerId = notification.args.playerId
      var kingdom = this.gamedatas.players[playerId].kingdom;
      kingdom.minX = Math.min(kingdom.minX, rotation === 2 ? parseInt(position.x) - 1 : parseInt(position.x));
      kingdom.minY = Math.min(kingdom.minY, rotation === 1 ? parseInt(position.y) - 1 : parseInt(position.y));
      kingdom.maxX = Math.max(kingdom.maxX, rotation === 0 ? parseInt(position.x) + 1 : parseInt(position.x));
      kingdom.maxY = Math.max(kingdom.maxY, rotation === 3 ? parseInt(position.y) + 1 : parseInt(position.y));
      var number = notification.args.number
      this.gamedatas.dominoes[number].location = 'KINGDOM';
      this.gamedatas.players[playerId]['score'] = notification.args.score;
      this.scoreCtrl[playerId].toValue(notification.args.score);
      if (toint(playerId) !== this.player_id) {
        dojo.style('domino_' + number, 'z-index', 200);
        dojo.style('domino_' + number, 'transform', 'scale(0.8)');
        this.placeOtherPlayerDomino(number, position, playerId, true);
        this.orientDomino(number, position.rotation, false);
        this.placeOnObject('domino_' + number, 'domino_' + number + '_placed');
        setTimeout(() => {
          dojo.removeClass('domino_' + number + '_placed', 'hidden');
          dojo.destroy('domino_' + number);
          this.centerOtherKingdom(playerId);
        }, 500);
        dojo.query('#kingdom_' + playerId + ' .last-placed').forEach(function (node) {
          dojo.removeClass(node, 'last-placed');
        });
        dojo.addClass('domino_' + number + '_placed', 'last-placed');
        setTimeout(endnotif, 1000)
      } else if (this.lastDominoPlaced !== number) {
        this.placeDomino(number, position);
        dojo.place('domino_' + number, 'map_scrollable_oversurface');
        endnotif();
      } else {
        endnotif();
      }
      if (this.gamedatas.turnsLeft === 0) {
        dojo.query('.king[data-value="' + number + '"]').forEach(dojo.destroy);
      }
    },

    notif_dominoDiscarded: function (notification) {
      this.gamedatas.dominoes[notification.args.number].location = 'DISCARD';
      dojo.destroy('domino_' + notification.args.number);
      if (this.gamedatas.turnsLeft === 0) {
        dojo.query('.king[data-value="' + notification.args.number + '"]').forEach(dojo.destroy);
      }
    },

    notif_treasurePlaced: function (notification) {
      const args = notification.args;
      // Slide the placed gem out of the supply token, then remove the token.
      this.renderPlacedTreasure(args.playerId, args.tokenKey, parseInt(args.x), parseInt(args.y), parseInt(args.rotation), args.tokenKey);
      $(args.tokenKey)?.remove();
      if (args.score !== undefined) {
        this.gamedatas.players[args.playerId]['score'] = args.score;
        this.scoreCtrl[args.playerId].toValue(args.score);
      }
      if (this.gamedatas.treasures) {
        var supply = this.gamedatas.treasures.supply || [];
        this.gamedatas.treasures.supply = supply.filter(function (key) {
          return key !== args.tokenKey;
        });
        if (!this.gamedatas.treasures.placed) {
          this.gamedatas.treasures.placed = {};
        }
        if (!this.gamedatas.treasures.placed[args.playerId]) {
          this.gamedatas.treasures.placed[args.playerId] = [];
        }
        this.gamedatas.treasures.placed[args.playerId].push({key: args.tokenKey, x: args.x, y: args.y, rotation: args.rotation});
      }
    },

    notif_treasureRevealed: function (notification) {
      var args = notification.args;
      $('treasure_chest_count').innerHTML = args.boxCount;
      if (args.tokenKey) {
        $('treasure_supply_gems').insertAdjacentHTML('beforeend', this.format_block('jstpl_treasure', {key: args.tokenKey}));
        this.addTooltipHtml(args.tokenKey, this.getTreasureTooltipHtml(args.tokenKey));
        if (this.gamedatas.treasures) {
          if (!this.gamedatas.treasures.supply) {
            this.gamedatas.treasures.supply = [];
          }
          this.gamedatas.treasures.supply.push(args.tokenKey);
          this.gamedatas.treasures.boxCount = args.boxCount;
        }
      } else if (this.gamedatas.treasures) {
        this.gamedatas.treasures.boxCount = args.boxCount;
      }
    },

    notif_immediateVictory: function (notification) {
      this.showMessage(notification.args.player_name + ' ' + _('wins!'), 'info');
    },

    notif_score: function (notification) {
      this.scoreCtrl[notification.args.playerId].incValue(notification.args.score);
    },

    notif_startPlayerScoring: function (notification) {
      var playerId = notification.args.playerId;
      var player = this.gamedatas.players[playerId];
      if (!this.finalScoring) {
        this.finalScoring = [];
        dojo.addClass('info_and_dominoes', 'scoring');
      }
      this.finalScoring.push(playerId);
      scoringIndex = this.finalScoring.length;
      $('score_sheet_player_' + scoringIndex).innerHTML = player.name;
      dojo.style('score_sheet_player_' + scoringIndex, 'color', '#' + player.color);
      dojo.attr('score_sheet_final_' + scoringIndex, 'data-value', player.color);
    },

    notif_endPlayerScoring: function (notification) {
      var playerId = notification.args.playerId;
      var scoringIndex = this.finalScoring.indexOf(playerId) + 1;
      if (notification.args.displayBonus) {
        $('score_sheet_bonus_' + scoringIndex).innerHTML = '+' + notification.args.bonus;
      } else {
        dojo.destroy('score_sheet_bonus_' + scoringIndex);
      }
      $('score_sheet_total_' + scoringIndex).innerHTML = notification.args.score;
      this.scoreCtrl[playerId].toValue(notification.args.score);
    },

    notif_scoreTerrain: function (notification) {
      var playerId = notification.args.playerId;
      var scoringIndex = this.finalScoring.indexOf(playerId) + 1;
      $('score_' + notification.args.terrain + '_' + scoringIndex).innerHTML = notification.args.score;
    },

    notif_scoreTerritory: function (notification) {
      var playerId = notification.args.playerId;
      var player = this.gamedatas.players[playerId];
      if (playerId === this.player_id) {
        $("pagemaintitletext").innerHTML = dojo.string.substitute(_("${you} score ${score} points for ${terrain}"),
          {
            you: '<span id="pagemaintitletext"><span style="font-weight:bold;color:#' + this.gamedatas.players[this.player_id].color + ';">' + _('You') + '</span>',
            score: notification.args.totalScoreForTerrain, terrain: _(notification.args.terrain)
          });
      } else {
        $("pagemaintitletext").innerHTML = dojo.string.substitute(_("${player_name} scores ${score} points for ${terrain}"),
          {
            player_name: '<span id="pagemaintitletext"><span style="font-weight:bold;color:#' + player.color + ';">' + player.name + '</span>',
            score: notification.args.totalScoreForTerrain, terrain: _(notification.args.terrain)
          });
      }
      this.scoreTerritory(notification.args);
      setTimeout(function () {
        dojo.query('.scoring-hidden').forEach(function (node) {
          dojo.removeClass(node, 'scoring-hidden');
        });
      }, 1)
    },

    scoreTerritory: function (args) {

      // Let's find the most adequate square to display the scoring for this territory
      var territory = args.territory;
      var borders = {};
      var xSum = 0, ySum = 0;
      var x, y;
      for (var i = 0; i < territory.locations.length; i++) {
        x = territory.locations[i].x
        y = territory.locations[i].y
        xSum += x;
        ySum += y;
        if (!borders[x]) borders[x] = {}
        borders[x][y] = {left: true, top: true, right: true, bottom: true};
      }
      var xBest = xSum / territory.locations.length;
      var yBest = ySum / territory.locations.length;
      var bestScoreLocation, bestLocationDistance = null;
      for (var j = 0; j < territory.locations.length; j++) {
        x = territory.locations[j].x
        y = territory.locations[j].y
        var locationDistance = Math.sqrt(Math.pow(territory.locations[j].x - xBest, 2) + Math.pow(territory.locations[j].y - yBest, 2));
        if (bestLocationDistance === null || locationDistance < bestLocationDistance) {
          bestScoreLocation = territory.locations[j];
          bestLocationDistance = locationDistance;
        }
        if (borders[x - 1] && borders[x - 1][y])
          borders[x - 1][y].right = false;
        if (borders[x + 1] && borders[x + 1][y])
          borders[x + 1][y].left = false;
        if (borders[x][y + 1])
          borders[x][y + 1].bottom = false;
        if (borders[x][y - 1])
          borders[x][y - 1].top = false;
      }
      for (x in borders) {
        for (y in borders[x]) {
          var style = '';
          for (var direction in borders[x][y]) {
            if (borders[x][y][direction]) {
              style += 'border-' + direction + ': 5px dashed white; '
            }
          }
          var border = this.format_block('jstpl_border', {left: x * 100, top: -y * 100, bordersStyle: style});
          if (args.playerId === this.player_id) {
            dojo.place(border, $('map_scrollable_oversurface'));
          } else {
            dojo.place(border, $('kingdom_' + args.playerId));
          }
        }
      }
      var gemCrowns = territory.gemCrowns || 0;
      var skulled = !!territory.skulled;
      var crowns = territory.crowns + gemCrowns;
      var total = skulled ? 0 : (territory.score !== undefined ? territory.score : crowns * territory.size);
      var score = this.format_block('jstpl_score', {
        left: bestScoreLocation.x * 100,
        top: -bestScoreLocation.y * 100,
        crowns: skulled ? _('Skull') : crowns,
        size: territory.size,
        total: total
      });
      if (args.playerId === this.player_id) {
        dojo.place(score, $('map_scrollable_oversurface'));
      } else {
        dojo.place(score, $('kingdom_' + args.playerId));
      }
    }
  });
});
