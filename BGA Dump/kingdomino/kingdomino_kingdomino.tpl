{OVERALL_GAME_HEADER}

<!--
--------
-- BGA framework: © Gregory Isabelli <gisabelli@boardgamearena.com> & Emmanuel Colin <ecolin@boardgamearena.com>
-- kingdomino implementation : © Alena Laskavaia <laskava@gmail.com> & Romain Fromi <romain.fromi@gmail.com>
--
-- This code has been produced on the BGA studio platform for use on http://boardgamearena.com.
-- See http://en.boardgamearena.com/#!doc/Studio for more information.
-------

    kingdomino_kingdomino.tpl
    
    This is the HTML template of your game.
    
    Everything you are writing in this file will be displayed in the HTML page of your game user interface,
    in the "main game zone" of the screen.
    
    You can use in this template:
    _ variables, with the format {MY_VARIABLE_ELEMENT}.
    _ HTML block, with the BEGIN/END format
    
    See your "view" PHP file to check how to set variables and control blocks
    
    Please REMOVE this comment before publishing your game on BGA
-->

<div id="thething" class="thething grid-size-{GRID_SIZE} {SPECTATOR}">
    <div id="dominoes_and_player_kingdom">
        <div id="info_and_dominoes">
            <div id="dominoes_displayed">
                <div id="current_dominoes_spaces">
                    <!-- BEGIN current_domino_space -->
                    <div id="current_domino_space_{NUMBER}" class="domino_space"></div>
                    <!-- END current_domino_space -->
                </div>
                <div id="future_dominoes_spaces">
                    <!-- BEGIN future_domino_space -->
                    <div id="future_domino_space_{NUMBER}" class="domino_space"></div>
                    <!-- END future_domino_space -->
                </div>
                <div id="treasure_supply" class="treasure_supply {LT_VISIBLE}">
                    <div id="treasure_supply_gems"></div>
                    <div id="treasure_chest" class="treasure treasure_back"><span id="treasure_chest_count"></span></div>
                </div>
            </div>
            <div id="info" class="info">
                <p>
                    <span id="dominoes_remaining">20 dominoes remaining (5 turns)</span>
                    <a href="#" id="moreInfoButton">More...</a>
                </p>
                <div id="more-information">
                    <p>
                        <a href="#" class="bgabutton bgabutton_gray" id="dominoesListButton">{DOMINOES_LIST}</a>
                    </p>
                    <p id="additional_rules">{ADDITIONAL_RULES}</p>
                    <p>
                        <a href="#" class="bgabutton bgabutton_gray {DYNASTY_VISIBLE}" id="dynastyButton">{DYNASTY}</a>
                        <a href="#" class="bgabutton bgabutton_gray {THE_MIDDLE_KINGDOM_VISIBLE}" id="middleKingdomButton">{THE_MIDDLE_KINGDOM}</a>
                        <a href="#" class="bgabutton bgabutton_gray {HARMONY_VISIBLE}" id="harmonyButton">{HARMONY}</a>
                        <a href="#" class="bgabutton bgabutton_gray {THE_MIGHTY_DUEL_VISIBLE}" id="mightyDuelButton">{THE_MIGHTY_DUEL}</a>
                        <a href="#" class="bgabutton bgabutton_gray {LT_VISIBLE}" id="lostTreasuresButton">{THE_LOST_TREASURES}</a>
                    </p>
                </div>
            </div>
            <div id="scoring_sheet">
                <div>
                    <div id="score_sheet_player_1" class="score-sheet-player"></div>
                    <div id="score_sheet_player_2" class="score-sheet-player"></div>
                    <div id="score_sheet_player_3" class="score-sheet-player"></div>
                    <div id="score_sheet_player_4" class="score-sheet-player"></div>
                </div>

                <div>
                    <div class="score-sheet-tile reverse" style="background-position: 44.5% 33.5%;"></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 22.5%;"><span id="score_forest_1"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 22.5%;"><span id="score_forest_2"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 55.5%;"><span id="score_forest_3"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 55.5%;"><span id="score_forest_4"></span></div>
                </div>

                <div>
                    <div class="score-sheet-tile reverse" style="background-position: 44.3% 22.5%;"></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 0.5%;"><span id="score_field_1"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 0.5%;"><span id="score_field_2"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 11.5%;"><span id="score_field_3"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 11.5%;"><span id="score_field_4"></span></div>
                </div>

                <div>
                    <div class="score-sheet-tile" style="background-position: 99.8% 0.5%;"></div>
                    <div class="score-sheet-tile" style="background-position: 22.7% 0.5%;"><span id="score_grassland_1"></span></div>
                    <div class="score-sheet-tile" style="background-position: 33.3% 0.5%;"><span id="score_grassland_2"></span></div>
                    <div class="score-sheet-tile" style="background-position: 0.7% 99.5%;"><span id="score_grassland_3"></span></div>
                    <div class="score-sheet-tile" style="background-position: 11.3% 99.5%;"><span id="score_grassland_4"></span></div>
                </div>

                <div>
                    <div class="score-sheet-tile reverse" style="background-position: 66.5% 0.5%;"></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 66.5%;"><span id="score_lake_1"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 66.5%;"><span id="score_lake_2"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 11.3% 77.5%;"><span id="score_lake_3"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 0.7% 77.5%;"><span id="score_lake_4"></span></div>
                </div>

                <div>
                    <div class="score-sheet-tile" style="background-position: 99.8% 22.5%;"></div>
                    <div class="score-sheet-tile" style="background-position: 22.5% 11.5%;"><span id="score_swamp_1"></span></div>
                    <div class="score-sheet-tile" style="background-position: 33.1% 11.5%;"><span id="score_swamp_2"></span></div>
                    <div class="score-sheet-tile" style="background-position: 22.5% 11.5%;"><span id="score_swamp_3"></span></div>
                    <div class="score-sheet-tile" style="background-position: 33.1% 11.5%;"><span id="score_swamp_4"></span></div>
                </div>

                <div>
                    <div class="score-sheet-tile reverse" style="background-position: 66.5% 99.5%;"></div>
                    <div class="score-sheet-tile reverse" style="background-position: 55.6% 22.5%;"><span id="score_mountain_1"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 55.6% 22.5%;"><span id="score_mountain_2"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 55.6% 22.5%;"><span id="score_mountain_3"></span></div>
                    <div class="score-sheet-tile reverse" style="background-position: 55.6% 22.5%;"><span id="score_mountain_4"></span></div>
                </div>

                <div>
                    <div id="score_sheet_total_options">
                        <div class="{THE_MIDDLE_KINGDOM_VISIBLE}"><span>{CENTERED}</span><span>+10</span></div>
                        <div class="{HARMONY_VISIBLE}"><span>{USED_ALL}</span><span>+5</span></div>
                    </div>
                    <div id="score_sheet_final_1" class="score-sheet-total">
                        <span id="score_sheet_bonus_1" class="score-sheet-bonus"></span>
                        <span id="score_sheet_total_1"></span>
                    </div>
                    <div id="score_sheet_final_2" class="score-sheet-total">
                        <span id="score_sheet_bonus_2" class="score-sheet-bonus"></span>
                        <span id="score_sheet_total_2"></span>
                    </div>
                    <div id="score_sheet_final_3" class="score-sheet-total">
                        <span id="score_sheet_bonus_3" class="score-sheet-bonus"></span>
                        <span id="score_sheet_total_3"></span></div>
                    <div id="score_sheet_final_4" class="score-sheet-total">
                        <span id="score_sheet_bonus_4" class="score-sheet-bonus"></span>
                        <span id="score_sheet_total_4"></span></div>
                </div>

            </div>
        </div>

        <div id="kingdom" class="kingdom" style="border: 2px solid #{COLOR}">
            <h3 id="kingdom_label" style="color: #{COLOR}">{KINGDOM_TITLE}</h3>
            <div id="map_container" style="width: {SIZE}px; min-height: {SIZE}px">
                <div id="map_scrollable"></div>
                <div id="map_surface"></div>
                <div id="map_scrollable_oversurface">
                    <div id="castle" class="castle"></div>
                </div>
                <a id="movetop" class="map_arrow" href="#"></a>
                <a id="moveleft" class="map_arrow" href="#"></a>
                <a id="moveright" class="map_arrow" href="#"></a>
                <a id="movedown" class="map_arrow" href="#"></a>
            </div>
        </div>

    </div>
    <div id="other_players_kingdoms">

    </div>
</div>

<script type="text/javascript">
  // Javascript HTML templates
  var jstpl_domino = '<div id="domino_${number}" class="domino"><div class="shadow"></div><div class="domino-background" style="background-position: ${x}px ${y}px"></div></div>';
  var jstpl_king = '<div id="${id}" class="king king_${color}" data-value="${domino}"></div>';
  var jstpl_manipulationArrows = '<img src="{GAMETHEMEURL}img/rotate.svg" class="manipulation-arrow rotate-left"/><img src="{GAMETHEMEURL}img/rotate.svg" class="manipulation-arrow rotate-right anticlockwise"/>';
  var jstpl_square = '<div id="square_${x}_${y}" class="square" style="left: ${left}px; top: ${top}px"></div>'
  var jstpl_kingdom = '<div class="player_view" style="width: ${sizeSmall}px; height: ${sizeSmall}px; border-color: #${color}"><div id="kingdom_${id}" class="kingdom" style="width: ${size}px; height: ${size}px" data-value="${color}"><div id="castle_${id}" class="castle castle-${color}"></div></div></div>';
  var jstpl_score = '<div class="territory-score scoring-hidden" style="left: ${left}px; top: ${top}px"><span>${crowns} x ${size}</span><span>= ${total}</span></div>'
  var jstpl_border = '<div class="border scoring-hidden" style="left: ${left}px; top: ${top}px; ${bordersStyle}"></div>'
  var jstpl_discard_button = '<div id="discard_button_mask"><a id="discard_button" href="#" class="bgabutton bgabutton_blue">${text}</a></div>'
  var jstpl_treasure = '<div id="${key}" class="treasure ${key}"></div>'
  var jstpl_placed_treasure = '<div id="${id}" class="treasure placed-treasure ${key}" style="left: ${left}px; top: ${top}px; transform: rotate(${angle}deg)"></div>'
  var jstpl_treasure_intersection = '<div id="treasure_intersection_${x}_${y}" class="treasure-intersection" style="left: ${left}px; top: ${top}px"></div>'
  var jstpl_treasure_rotate_arrow = '<img src="{GAMETHEMEURL}img/rotate.svg" class="manipulation-arrow rotate-right anticlockwise treasure-rotate-arrow"/>'
</script>

{OVERALL_GAME_FOOTER}
