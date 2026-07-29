"""Tests for the BGA → engine translation.

The centrepiece is `FakeBgaTable`: a replica of the *bookkeeping* BGA's backend does
(`kingdomino.game.php` — the `dominoes` table's DRAW_PILE / FUTURE / CURRENT / KINGDOM /
DISCARD locations and the king ownership column), driven in lockstep with a real engine
game. At every decision it emits the same snapshot shape the Chrome extension sends, and
the test asserts the translated position reproduces the engine's own state.

That covers the whole capture path offline: coordinate and rotation mapping, BGA's
CURRENT-is-the-placing-line convention, the starting round claiming out of FUTURE, tiles
leaving the line when resolved, discards, and the final place-only round.

Run: `../.venv/Scripts/python -m kdagent.test_bga` from the agent/ dir (or with pytest).
"""
from __future__ import annotations

import contextlib
import json
import random

import kingdomino as kd
from kdagent import bga

SEAT_IDS = ["1001", "1002"]  # BGA player ids; sorted numerically -> engine seats 0, 1


class _Caught:
    """Holds the raised exception, so a test can assert on its message (`.value`)."""

    value: BaseException | None = None


@contextlib.contextmanager
def raises(exc, match=None):
    """Minimal `pytest.raises` — the agent's tests run on the stdlib alone."""
    caught = _Caught()
    try:
        yield caught
    except exc as e:
        caught.value = e
        if match is not None and match not in str(e):
            raise AssertionError(f"expected {match!r} in {e!r}") from None
        return
    raise AssertionError(f"expected {exc.__name__}")


def fail(msg):
    raise AssertionError(msg)


class FakeBgaTable:
    """BGA's domino bookkeeping, mirrored. Locations follow `kingdomino.game.php`."""

    def __init__(self):
        # number -> {"location", "owner", "x", "y", "rotation"}
        self.dominoes: dict[int, dict] = {}
        self.pending: list[int] = []  # drawn but not yet dealt as a line of 4

    # -- the backend's own transitions -------------------------------------------------
    def draw(self, number: int) -> None:
        """One domino off the pile. BGA deals four at a time; the engine draws them one by
        one, so buffer until a full line then place it in FUTURE."""
        self.pending.append(number)
        if len(self.pending) == bga.LINE:
            for n in self.pending:
                self.dominoes[n] = {"location": "FUTURE", "owner": None}
            self.pending.clear()

    def promote(self) -> None:
        """`drawDominoes`: FUTURE becomes the line to place from."""
        for d in self.dominoes.values():
            if d["location"] == "FUTURE":
                d["location"] = "CURRENT"

    def claim(self, number: int, seat: int) -> None:
        self.dominoes[number]["owner"] = SEAT_IDS[seat]

    def place(self, number: int, x: int, y: int, rotation: int) -> None:
        self.dominoes[number].update(
            {"location": "KINGDOM", "x": x, "y": y, "rotation": rotation}
        )

    def discard(self, number: int) -> None:
        self.dominoes[number]["location"] = "DISCARD"

    def current(self) -> list[int]:
        return sorted(n for n, d in self.dominoes.items() if d["location"] == "CURRENT")

    def future(self) -> list[int]:
        return sorted(n for n, d in self.dominoes.items() if d["location"] == "FUTURE")

    # -- what the extension sends ------------------------------------------------------
    def snapshot(self, state: str, active_seat: int, **extra) -> dict:
        return {
            "table": "999",
            "seat_bga": SEAT_IDS[0],
            "active_bga": SEAT_IDS[active_seat],
            "state": state,
            "grid_size": 7,
            "variants": {"harmony": True, "middle_kingdom": True,
                         "mighty_duel": True, "lost_treasures": False},
            "players": [{"bga_id": pid, "name": f"p{i}", "score": 0, "color": "ff0000"}
                        for i, pid in enumerate(SEAT_IDS)],
            "dominoes": {str(n): dict(d) for n, d in self.dominoes.items()},
            **extra,
        }


def _place_lookup(game) -> dict:
    """Legal actions keyed by index, carrying BGA's `(x, y, rotation)` for placements."""
    return {p["index"]: p for p in json.loads(game.action_previews())}


def _drive(seed: int, moves: int | None = None):
    """Play a game, keeping a BGA mirror in step. Yields `(game, snapshot)` per decision."""
    game = kd.Game(seed, 2, True, True)
    table = FakeBgaTable()
    rng = random.Random(seed)

    played = 0
    while not game.is_terminal():
        if game.is_chance():
            outcome = json.loads(game.apply_chance())
            if outcome["type"] == "draw":
                table.draw(outcome["number"])
            continue

        obs = json.loads(game.observation())
        phase = obs["phase"]
        state = "placeDomino" if phase == "place" else "chooseDomino"
        yield game, table.snapshot(state, game.to_act())

        actions = _place_lookup(game)
        idx = rng.randrange(game.num_actions())
        act = actions[idx]
        if act["type"] == "claim":
            line = obs["current_line"] if phase == "start_claim" else obs["next_line"]
            table.claim(line[act["slot"]]["number"], game.to_act())
        elif act["type"] == "place":
            table.place(obs["current_domino"]["number"], act["x"], act["y"],
                        act["bga_rotation"])
        else:
            table.discard(obs["current_domino"]["number"])

        game.apply(idx)
        played += 1
        if moves is not None and played >= moves:
            return
        # Mirror BGA's line rotation. `drawDominoes` runs from `activateOwnerOfNextCurrentDomino`
        # only once the placing line is empty *and* every king has claimed from the next one —
        # i.e. at the round boundary, after the last king's claim, not after its placement.
        # The same rule promotes the first line out of FUTURE at the end of the starting round.
        future = table.future()
        if not table.current() and future and all(
            table.dominoes[n]["owner"] is not None for n in future
        ):
            table.promote()


def test_snapshot_translation_matches_the_engine_all_game():
    """Every position the mirror shows must rebuild into the state the engine is actually in."""
    checked = 0
    for seed in range(6):
        for game, snap in _drive(seed):
            rebuilt, meta = bga.build_game(snap)
            assert rebuilt.to_act() == game.to_act(), f"seed {seed}: to_act"
            assert rebuilt.phase() == game.phase(), f"seed {seed}: phase"
            assert rebuilt.round() == game.round(), f"seed {seed}: round"
            a, b = json.loads(game.observation()), json.loads(rebuilt.observation())
            assert b["remaining"] == a["remaining"], f"seed {seed}: deck"
            assert b["seats"] == a["seats"], f"seed {seed}: kingdoms"
            assert b["scores"] == a["scores"], f"seed {seed}: scores"
            assert b["current_domino"] == a["current_domino"], f"seed {seed}: tile to place"
            assert json.loads(rebuilt.legal_actions()) == json.loads(game.legal_actions())
            assert meta["you"] == 0
            checked += 1
    assert checked > 300, f"expected the full game at every node, saw {checked}"


def test_discards_are_carried_through_the_snapshot():
    """A discarded tile is invisible in BGA's `getAllDatas`; the extension tracks it from the
    notification stream. Confirm the translation uses it and that losing it is caught."""
    found = False
    for seed in range(40):
        for game, snap in _drive(seed):
            discards = [n for n, d in snap["dominoes"].items() if d["location"] == "DISCARD"]
            if not discards:
                continue
            found = True
            rebuilt, _ = bga.build_game(snap)
            assert rebuilt.to_act() == game.to_act()
            assert json.loads(rebuilt.observation())["remaining"] == \
                json.loads(game.observation())["remaining"]
            # Drop the discard record: the engine must refuse rather than advise from a deck
            # that still contains a tile nobody can draw.
            blind = dict(snap)
            blind["dominoes"] = {k: v for k, v in snap["dominoes"].items() if k not in discards}
            with raises(bga.CaptureError) as e:
                bga.build_game(blind)
            assert "unaccounted" in str(e.value) or "line" in str(e.value)
            break
        if found:
            break
    assert found, "no discard occurred in 40 random games — widen the search"


def test_bga_preview_oracle_agrees_with_the_engine():
    """BGA ships its own legal-placement list *with scores*. Feed the engine's answer back as
    if it were BGA's and confirm the checker is silent; then corrupt it and confirm it fires."""
    for game, snap in _drive(3, moves=60):
        if game.phase() != "place":
            continue
        previews = json.loads(game.action_previews())
        places = [p for p in previews if p["type"] == "place"]
        if len(places) < 3:
            continue
        snap = dict(snap)
        snap["bga_previews"] = [
            {"x": p["x"], "y": p["y"], "rotation": p["bga_rotation"], "score": p["score"]}
            for p in places
        ]
        assert bga.check_previews(previews, snap) is None

        # A placement BGA allows and we do not.
        short = dict(snap, bga_previews=snap["bga_previews"] + [
            {"x": 6, "y": 6, "rotation": 0, "score": 99}])
        assert bga.check_previews(previews, short)["engine_missing"]
        # A scoring disagreement on a shared placement.
        skewed = json.loads(json.dumps(snap))
        skewed["bga_previews"][0]["score"] += 7
        assert bga.check_previews(previews, skewed)["score_mismatch"]
        return
    fail("no placement decision with enough options found")


def test_unsupported_tables_are_named_not_guessed():
    _, snap = next(_drive(0))
    with raises(bga.Unsupported, match="Mighty Duel"):
        bga.build_game(dict(snap, grid_size=5))
    with raises(bga.Unsupported, match="Lost Treasures"):
        bga.build_game(dict(snap, variants=dict(snap["variants"], lost_treasures=True)))
    with raises(bga.Unsupported, match="2p"):
        bga.build_game(dict(snap, players=snap["players"] + [
            {"bga_id": "1003", "name": "p2", "score": 0, "color": "00ff00"}]))


def test_coordinate_helpers_match_the_engine():
    """The Python mapping is a copy of `rebuild::cell_from_xy`; pin it against the engine's
    own translation, which comes back through `action_previews`."""
    assert bga.cell_from_xy(0, 0) == (bga.CENTER, bga.CENTER)
    assert bga.cell_from_xy(1, 0) == (bga.CENTER, bga.CENTER + 1)  # +x is right
    assert bga.cell_from_xy(0, 1) == (bga.CENTER - 1, bga.CENTER)  # +y is up the screen
    for x in range(-6, 7):
        for y in range(-6, 7):
            assert bga.xy_from_cell(*bga.cell_from_xy(x, y)) == (x, y)
    for rot in range(4):
        assert bga.rot_from_bga(bga.rot_to_bga(rot)) == rot

    for game, _ in _drive(5, moves=40):
        if game.phase() != "place":
            continue
        for p in json.loads(game.action_previews()):
            if p["type"] != "place":
                continue
            # The engine's own (row, col) and its BGA (x, y) must be the same cell.
            assert bga.cell_from_xy(p["x"], p["y"]) == (p["row"], p["col"])
            assert bga.rot_from_bga(p["bga_rotation"]) == p["rot"]
        return


def test_capture_errors_are_distinguishable_from_unsupported_tables():
    _, snap = next(_drive(1))
    broken = json.loads(json.dumps(snap))
    first = next(iter(broken["dominoes"]))
    broken["dominoes"][first]["location"] = "NOWHERE"
    with raises(bga.CaptureError, match="unknown location"):
        bga.build_game(broken)

    stray = json.loads(json.dumps(snap))
    stray["active_bga"] = "4242"
    with raises(bga.CaptureError, match="not at this table"):
        bga.build_game(stray)


def test_descriptions_and_highlights_are_actionable():
    for game, _ in _drive(7, moves=40):
        if game.phase() != "place":
            continue
        obs = json.loads(game.observation())
        for p in json.loads(game.action_previews()):
            text = bga.describe(p, obs)
            hl = bga.highlight(p)
            if p["type"] == "place":
                assert f"({p['x']},{p['y']})" in text
                assert hl["kind"] == "cells" and len(hl["cells"]) == 2
                # The two highlighted cells are exactly the tile's two squares.
                (x1, y1), (x2, y2) = hl["cells"]
                assert (x1, y1) == (p["x"], p["y"])
                assert bga.cell_from_xy(x2, y2) != bga.cell_from_xy(x1, y1)
                assert abs(x1 - x2) + abs(y1 - y2) == 1
            elif p["type"] == "claim":
                assert text.startswith("Claim #")
                assert hl["kind"] == "domino"
        return


if __name__ == "__main__":
    print("kdagent BGA translation tests")
    for fn in (
        test_snapshot_translation_matches_the_engine_all_game,
        test_discards_are_carried_through_the_snapshot,
        test_bga_preview_oracle_agrees_with_the_engine,
        test_unsupported_tables_are_named_not_guessed,
        test_coordinate_helpers_match_the_engine,
        test_capture_errors_are_distinguishable_from_unsupported_tables,
        test_descriptions_and_highlights_are_actionable,
    ):
        fn()
        print(f"  {fn.__name__} OK")
    print("ALL OK")
