"""Encoder tests: shapes, plane correctness, action alignment, and raw re-encode round-trip.
Run: `../.venv/Scripts/python -m kdagent.test_encoder` from the agent/ dir (or with pytest)."""
import json
import random

import numpy as np

import kingdomino as kd
from kdagent.encoder import (
    A_CLAIM, A_DISCARD, A_PLACE, CENTER, GRID, LINE_FEATS, N_LINE_TOKENS, N_PLANES, N_TERRAIN,
    P_CASTLE, P_EMPTY, STORE, encode, encode_obs, global_dim,
)


def advance_to_player(g):
    while g.is_chance() and not g.is_terminal():
        g.apply_chance()
    return g


def test_shapes_and_planes():
    g = advance_to_player(kd.Game(7, 2))
    enc = encode(g)
    pc = enc.player_count
    assert enc.board.shape == (pc, N_PLANES, STORE, STORE), enc.board.shape
    assert enc.lines.shape == (N_LINE_TOKENS, LINE_FEATS), enc.lines.shape
    assert enc.glob.shape == (global_dim(pc),), enc.glob.shape

    obs = json.loads(g.observation())
    # self seat is obs["to_act"]; it is board index 0 (seat-relative ordering).
    self_seat = obs["seats"][obs["to_act"]]
    b0 = enc.board[0]
    # Castle present exactly once at the center.
    assert b0[P_CASTLE, CENTER, CENTER] == 1.0
    assert b0[P_CASTLE].sum() == 1.0
    # Terrain planes account for exactly the seat's filled cells.
    assert b0[:N_TERRAIN].sum() == self_seat["filled"]
    # Every cell is exactly one of {castle, a terrain, empty}.
    cover = b0[:N_TERRAIN].sum(axis=0) + b0[P_CASTLE] + b0[P_EMPTY]
    assert np.all(cover == 1.0), "planes must partition every cell"
    print("  shapes + planes OK")


def test_action_alignment():
    rng = random.Random(0)
    g = kd.Game(3, 2)
    seen_place = seen_claim = False
    for _ in range(400):
        if g.is_terminal():
            break
        if g.is_chance():
            g.apply_chance()
            continue
        legal = json.loads(g.legal_actions())
        enc = encode(g)
        assert len(enc.actions.type_id) == len(legal)
        # type counts match the engine's legal set
        n_place = sum(1 for a in legal if a["type"] == "place")
        n_claim = sum(1 for a in legal if a["type"] == "claim")
        n_disc = sum(1 for a in legal if a["type"] == "discard")
        assert int((enc.actions.type_id == A_PLACE).sum()) == n_place
        assert int((enc.actions.type_id == A_CLAIM).sum()) == n_claim
        assert int((enc.actions.type_id == A_DISCARD).sum()) == n_disc
        # each action row matches the engine action at the same apply index
        for a in legal:
            i = a["index"]
            if a["type"] == "place":
                seen_place = True
                assert (enc.actions.row[i], enc.actions.col[i], enc.actions.rot[i]) == (
                    a["row"], a["col"], a["rot"])
                assert 0 <= a["row"] < STORE and 0 <= a["col"] < STORE and 0 <= a["rot"] < 4
            elif a["type"] == "claim":
                seen_claim = True
                assert enc.actions.slot[i] == a["slot"] and 0 <= a["slot"] < 4
        g.apply(rng.choice(legal)["index"])
    assert seen_place and seen_claim, "exercise both place and claim actions"
    print("  action alignment OK (place + claim seen)")


def test_raw_reencode_roundtrip():
    g = advance_to_player(kd.Game(11, 2))
    table = json.loads(kd.domino_table())
    live = encode(g)
    raw = encode_obs(json.loads(g.observation()), json.loads(g.legal_actions()), table)
    assert np.array_equal(live.board, raw.board)
    assert np.array_equal(live.lines, raw.lines)
    assert np.array_equal(live.glob, raw.glob)
    assert np.array_equal(live.actions.type_id, raw.actions.type_id)
    assert np.array_equal(live.actions.row, raw.actions.row)
    print("  raw re-encode round-trip OK")


def test_finite_and_bounded():
    rng = random.Random(7)
    for seed in range(8):
        g = kd.Game(seed, 2)
        for _ in range(2000):
            if g.is_terminal():
                break
            if g.is_chance():
                g.apply_chance()
                continue
            enc = encode(g)
            for arr in (enc.board, enc.lines, enc.glob):
                assert np.all(np.isfinite(arr)) and float(arr.max(initial=0.0)) <= 1.0001
            g.apply(rng.choice(json.loads(g.legal_actions()))["index"])
    print("  features finite & in [0,1] across random play OK")


if __name__ == "__main__":
    print("kdagent encoder tests")
    test_shapes_and_planes()
    test_action_alignment()
    test_raw_reencode_roundtrip()
    test_finite_and_bounded()
    print("ALL OK")
