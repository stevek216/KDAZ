"""Observation -> tensors for the AlphaZero net (see ../docs/feature-schema.md).

`encode(game)` turns the bridge's `observation()` + `legal_actions()` into:
  - `board`  [P, C, 13, 13] float32 — per-seat kingdom planes (seat-relative; self first),
  - `lines`  [8, F] float32 — the 8 draft slots (current_line 0..3 then next_line 0..3),
  - `glob`   [G] float32 — phase / round / deck / variants / per-seat summary,
  - an `Actions` batch aligned to `legal_actions()` order (apply index = row), describing
    each legal action for the policy heads: a place is (row, col, rot) on the 13x13 board,
    a claim is a line `slot`, discard is forced.

`encode_obs(obs, legal, table)` does the same from raw dicts, so a self-play corpus can
store the compact raw inputs and re-encode them at training time (schema-flexible).
"""
from __future__ import annotations

import json
from dataclasses import dataclass

import numpy as np

# --- board planes (channel-first for conv) ---------------------------------------
STORE = 13
CENTER = 6
GRID = 7
N_TERRAIN = 6
# plane layout: [0..5] terrain one-hot, [6..9] crowns one-hot(0..3), 10 castle, 11 empty,
# 12 placeable-region hint.
P_CASTLE, P_EMPTY, P_REACH = 10, 11, 12
N_PLANES = 13

# --- draft-line token features ---------------------------------------------------
# is_current, is_next, present, a_terrain[6], a_crowns, b_terrain[6], b_crowns, number,
# owner[self,other,none], is_place_target, claimable_now
LINE_FEATS = 1 + 1 + 1 + N_TERRAIN + 1 + N_TERRAIN + 1 + 1 + 3 + 1 + 1
N_LINE_TOKENS = 8

# --- global vector ---------------------------------------------------------------
# phase one-hot over player phases (start_claim, place, claim)
PHASE_INDEX = {"start_claim": 0, "place": 1, "claim": 2}
N_PHASE = 3
PER_SEAT_GLOBAL = 5  # filled, row_span, col_span, crown_score, largest_territory

# --- action types ----------------------------------------------------------------
A_PLACE, A_CLAIM, A_DISCARD = 0, 1, 2

_TABLE = None


def load_domino_table(table=None):
    """The static 48-domino table (terrain/crowns per half), cached. Pass a parsed list to
    avoid importing the engine (e.g. when re-encoding a corpus)."""
    global _TABLE
    if table is not None:
        return table
    if _TABLE is None:
        import kingdomino as kd

        _TABLE = json.loads(kd.domino_table())
    return _TABLE


@dataclass
class Actions:
    type_id: np.ndarray  # [A] int, A_PLACE / A_CLAIM / A_DISCARD (row = apply index)
    row: np.ndarray      # [A] int, place anchor row (else -1)
    col: np.ndarray      # [A] int, place anchor col (else -1)
    rot: np.ndarray      # [A] int, place rotation 0..3 (else -1)
    slot: np.ndarray     # [A] int, claim line slot 0..3 (else -1)


@dataclass
class Encoded:
    board: np.ndarray    # [P, N_PLANES, 13, 13] float32
    lines: np.ndarray    # [8, LINE_FEATS] float32
    glob: np.ndarray     # [G] float32
    actions: Actions
    player_count: int


def global_dim(player_count: int) -> int:
    # phase + round + cursor + deck_remaining + 2 variant flags + per-seat summary
    return N_PHASE + 1 + 1 + 1 + 2 + PER_SEAT_GLOBAL * player_count


def _seat_order(pc: int, to_act: int):
    """Absolute seat indices in self-relative order: [self, next, ...]."""
    return [(to_act + k) % pc for k in range(pc)]


def _board_planes(seat_obs) -> np.ndarray:
    b = np.zeros((N_PLANES, STORE, STORE), dtype=np.float32)
    b[P_CASTLE, CENTER, CENTER] = 1.0
    for cell in seat_obs["cells"]:
        r, c, t, cr = cell["r"], cell["c"], cell["terrain"], cell["crowns"]
        b[t, r, c] = 1.0
        b[N_TERRAIN + cr, r, c] = 1.0
    # empty = not castle and not any terrain
    occupied = b[:N_TERRAIN].sum(axis=0) + b[P_CASTLE]
    b[P_EMPTY] = (occupied == 0.0).astype(np.float32)
    # placeable-region hint: a cell that keeps both bbox spans < GRID if occupied.
    mnr, mxr = seat_obs["min_r"], seat_obs["max_r"]
    mnc, mxc = seat_obs["min_c"], seat_obs["max_c"]
    for r in range(STORE):
        span_r = max(mxr, r) - min(mnr, r)
        if span_r >= GRID:
            continue
        for c in range(STORE):
            span_c = max(mxc, c) - min(mnc, c)
            if span_c < GRID:
                b[P_REACH, r, c] = 1.0
    return b


def _line_token(slot_obj, is_current, table, to_act, pc, place_target, claimable) -> np.ndarray:
    v = np.zeros(LINE_FEATS, dtype=np.float32)
    o = 0
    v[o] = 1.0 if is_current else 0.0
    v[o + 1] = 0.0 if is_current else 1.0
    o += 2
    present = slot_obj.get("domino") is not None
    v[o] = 1.0 if present else 0.0
    o += 1
    if present:
        d = table[slot_obj["domino"]]
        v[o + d["a"]["terrain"]] = 1.0
        o += N_TERRAIN
        v[o] = d["a"]["crowns"] / 3.0
        o += 1
        v[o + d["b"]["terrain"]] = 1.0
        o += N_TERRAIN
        v[o] = d["b"]["crowns"] / 3.0
        o += 1
        v[o] = slot_obj["number"] / 48.0
        o += 1
        owner = slot_obj.get("owner")
        rel = 2 if owner is None else (0 if owner == to_act else 1)
        v[o + rel] = 1.0
        o += 3
    else:
        o += N_TERRAIN + 1 + N_TERRAIN + 1 + 1 + 3
    v[o] = 1.0 if place_target else 0.0
    v[o + 1] = 1.0 if claimable else 0.0
    return v


def encode(game, table=None) -> Encoded:
    """Encode a live bridge `Game` at a player decision node."""
    return encode_obs(json.loads(game.observation()), json.loads(game.legal_actions()), table)


def encode_obs(obs: dict, legal: list, table=None) -> Encoded:
    table = load_domino_table(table)
    pc = obs["player_count"]
    to_act = obs["to_act"]
    order = _seat_order(pc, to_act)
    phase = obs["phase"]

    # --- board planes, seat-relative ---
    board = np.stack([_board_planes(obs["seats"][s]) for s in order], axis=0)

    # --- which line is claimable now, and which slots ---
    claim_slots = {a["slot"] for a in legal if a["type"] == "claim"}
    claim_line_is_current = phase == "start_claim"  # else "claim" -> next_line
    place_cursor = obs["turn_cursor"] if phase == "place" else -1

    # --- draft-line tokens: current_line (0..3) then next_line (4..7) ---
    line_rows = []
    for i, slot_obj in enumerate(obs["current_line"]):
        claimable = claim_line_is_current and i in claim_slots
        line_rows.append(_line_token(slot_obj, True, table, to_act, pc,
                                     place_target=(i == place_cursor), claimable=claimable))
    for i, slot_obj in enumerate(obs["next_line"]):
        claimable = (not claim_line_is_current) and i in claim_slots
        line_rows.append(_line_token(slot_obj, False, table, to_act, pc,
                                     place_target=False, claimable=claimable))
    lines = np.stack(line_rows, axis=0)

    # --- global vector ---
    g = np.zeros(global_dim(pc), dtype=np.float32)
    o = 0
    if phase in PHASE_INDEX:
        g[o + PHASE_INDEX[phase]] = 1.0
    o += N_PHASE
    g[o] = obs["round"] / 12.0
    g[o + 1] = obs["turn_cursor"] / 3.0
    g[o + 2] = obs["deck_remaining"] / 48.0
    o += 3
    g[o] = 1.0 if obs["variants"]["harmony"] else 0.0
    g[o + 1] = 1.0 if obs["variants"]["middle_kingdom"] else 0.0
    o += 2
    for s in order:
        seat, sc = obs["seats"][s], obs["scores"][s]
        g[o] = seat["filled"] / 48.0
        g[o + 1] = (seat["max_r"] - seat["min_r"]) / (GRID - 1)
        g[o + 2] = (seat["max_c"] - seat["min_c"]) / (GRID - 1)
        g[o + 3] = min(sc["crown_score"], 100) / 100.0
        g[o + 4] = sc["largest_territory"] / (GRID * GRID)
        o += PER_SEAT_GLOBAL

    # --- actions aligned to legal_actions order (apply index = position) ---
    n = len(legal)
    a_type = np.zeros(n, dtype=np.int64)
    a_row = np.full(n, -1, dtype=np.int64)
    a_col = np.full(n, -1, dtype=np.int64)
    a_rot = np.full(n, -1, dtype=np.int64)
    a_slot = np.full(n, -1, dtype=np.int64)
    for a in legal:
        i = a["index"]
        if a["type"] == "place":
            a_type[i] = A_PLACE
            a_row[i], a_col[i], a_rot[i] = a["row"], a["col"], a["rot"]
        elif a["type"] == "claim":
            a_type[i] = A_CLAIM
            a_slot[i] = a["slot"]
        else:  # discard
            a_type[i] = A_DISCARD

    return Encoded(
        board=board,
        lines=lines,
        glob=g,
        actions=Actions(a_type, a_row, a_col, a_rot, a_slot),
        player_count=pc,
    )
