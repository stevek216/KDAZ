"""BoardGameArena → engine translation for the advisor (`advisor/DESIGN.md`).

The Chrome extension watches a live BGA Kingdomino table and POSTs a **snapshot**; this
module turns that snapshot into an engine position, asks the engine what is legal, and
turns the search's answer back into something executable at the table. It reimplements no
rule: coordinates, rotations, whose turn it is and what is left in the deck all come from
`kingdomino_engine::core::rebuild` via `kingdomino.Game.from_position` (CLAUDE §3).

Snapshot dialect (the extension's contract — `advisor/extension/page-agent.js` builds it):

    { "table": str,                  # BGA table id, groups the server-side recording
      "seat_bga": str,               # the BGA player id this client is playing
      "active_bga": str,             # the player BGA says is on the clock
      "state": str,                  # BGA gamestate name: chooseDomino | placeDomino | ...
      "grid_size": int,              # 7 under The Mighty Duel; anything else is unsupported
      "variants": {"harmony": bool, "middle_kingdom": bool,
                   "mighty_duel": bool, "lost_treasures": bool},
      "players": [{"bga_id": str, "name": str, "score": int, "color": str}, ...],
      "dominoes": {"<number>": {"location": "CURRENT"|"FUTURE"|"KINGDOM"|"DISCARD",
                                "owner": "<bga_id>"|null,
                                "x": int, "y": int, "rotation": int}},   # x/y/rot: KINGDOM only
      "bga_current": int|null,       # argsPlaceDomino.domino — the tile BGA says to place
      "bga_previews": [{"x": int, "y": int, "rotation": int, "score": int}, ...] }

`bga_previews` is BGA's own list of every legal placement **with the score it would give**.
It is the reference implementation handing us its answer, so every placement decision is
also a live differential audit of our placement and scoring rules — see `check_previews`.

Two BGA conventions matter and are handled here rather than anywhere else:

  * **BGA's CURRENT is the line being placed from, FUTURE the line being claimed** — the
    engine's `current_line` / `next_line`. In the starting round CURRENT is empty and the
    claims land in FUTURE, so that first line maps to the engine's `current_line` instead.
  * **A resolved domino leaves CURRENT.** The engine keeps its slot, so the position spec
    represents those slots as blanks; the rebuild derives the acting king from them.
"""
from __future__ import annotations

import json
from typing import Any

import kingdomino as kd

# The only table shape the engine models (CLAUDE §1): 2 players, all 48 dominoes, 7x7.
GRID = 7
LINE = 4
NUM_DOMINOES = 48
CENTER = GRID - 1  # engine backing-store centre: (2*GRID-1)//2

TERRAIN_NAMES = ["wheat", "forest", "lake", "grassland", "swamp", "mine"]
# BGA writes 'field' for wheat and 'mountain' for the mine terrain (docs/bga/README.md).
TERRAIN_SHORT = ["wheat", "forest", "lake", "grass", "swamp", "mine"]

_TABLE: list[dict] | None = None


def domino_table() -> list[dict]:
    """The static 48-domino table, indexed by `number - 1`. Loaded from the engine once."""
    global _TABLE
    if _TABLE is None:
        _TABLE = json.loads(kd.domino_table())
    return _TABLE


def face_desc(sq: dict) -> str:
    """One half of a tile, e.g. `lake+1`."""
    t = TERRAIN_SHORT[sq["terrain"]]
    return f"{t}+{sq['crowns']}" if sq["crowns"] else t


def domino_desc(number: int) -> str:
    """A tile as a human reads it, e.g. `#23 forest/lake+1`."""
    d = domino_table()[number - 1]
    return f"#{number} {face_desc(d['a'])}/{face_desc(d['b'])}"


# Where the tile's second square sits relative to its first, in BGA's rotations
# (0=+x, 1=-y, 2=-x, 3=+y) — and what that reads as on screen, where +y is up.
BGA_STEP = [(1, 0), (0, -1), (-1, 0), (0, 1)]
BGA_DIRECTION = ["to its right", "below it", "to its left", "above it"]


class Unsupported(Exception):
    """The table is not one the engine models. The panel says so instead of guessing."""


class CaptureError(Exception):
    """The snapshot does not describe a reachable position — a capture bug, not a table we
    cannot model. Distinct from `Unsupported` because the user can act on it (reload, dump)."""


# --------------------------------------------------------------------------- geometry
def cell_from_xy(x: int, y: int) -> tuple[int, int]:
    """BGA's castle-relative `(x, y)` (castle `(0,0)`, `+x` right, `+y` **up** the screen) to
    an engine backing-store `(row, col)`. Mirrors `rebuild::cell_from_xy`; the engine's tests
    pin the pair, and `test_bga.py` pins this copy against them."""
    return CENTER - y, CENTER + x


def xy_from_cell(r: int, c: int) -> tuple[int, int]:
    return c - CENTER, CENTER - r


def rot_from_bga(rotation: int) -> int:
    """BGA rotation (0=+x, 1=-y, 2=-x, 3=+y) to engine rot (0=up, 1=right, 2=down, 3=left)."""
    return (rotation + 1) % 4


def rot_to_bga(rot: int) -> int:
    return (rot + 3) % 4


# --------------------------------------------------------------------------- translation
def _check_supported(snap: dict) -> None:
    v = snap.get("variants") or {}
    players = snap.get("players") or []
    if v.get("lost_treasures"):
        raise Unsupported("Lost Treasures table — the engine does not model gems")
    if len(players) != 2:
        raise Unsupported(f"{len(players)}-player table — the engine plays 2p Mighty Duel")
    grid = int(snap.get("grid_size") or 0)
    if grid != GRID:
        raise Unsupported(
            f"{grid}x{grid} kingdom — the engine is built for The Mighty Duel ({GRID}x{GRID})"
        )


def _seats(snap: dict) -> tuple[list[dict], dict[str, int]]:
    """Assign engine seats to BGA players. Any consistent labelling works (the value head is
    seat-relative), so sort by BGA id for a stable, debuggable mapping across snapshots."""
    players = sorted(snap["players"], key=lambda p: int(p["bga_id"]))
    return players, {str(p["bga_id"]): i for i, p in enumerate(players)}


def snapshot_to_position(snap: dict) -> dict:
    """Translate a snapshot into the engine's position spec (`Game.from_position` input).

    Raises `Unsupported` for a table the engine does not model and `CaptureError` when the
    snapshot itself is incoherent. Everything derivable — the acting king, the round, the
    deck — is left to the engine; this only re-expresses what BGA showed.
    """
    _check_supported(snap)
    players, seat_of = _seats(snap)

    doms = snap.get("dominoes") or {}
    placed: list[list[dict]] = [[] for _ in players]
    current: list[tuple[int, int | None]] = []  # (number, owner seat)
    future: list[tuple[int, int | None]] = []
    discarded: list[int] = []

    for key, d in doms.items():
        number = int(key)
        if not 1 <= number <= NUM_DOMINOES:
            raise CaptureError(f"domino number {number} outside 1..{NUM_DOMINOES}")
        loc = d.get("location")
        owner_bga = d.get("owner")
        owner = seat_of.get(str(owner_bga)) if owner_bga not in (None, "") else None
        if loc == "KINGDOM":
            if owner is None:
                raise CaptureError(f"domino {number} is placed but has no owner")
            try:
                r, c = cell_from_xy(int(d["x"]), int(d["y"]))
            except (KeyError, TypeError, ValueError) as e:
                raise CaptureError(f"domino {number} is placed without coordinates ({e})") from e
            placed[owner].append(
                {"number": number, "r": r, "c": c, "rot": rot_from_bga(int(d["rotation"]))}
            )
        elif loc == "CURRENT":
            current.append((number, owner))
        elif loc == "FUTURE":
            future.append((number, owner))
        elif loc == "DISCARD":
            discarded.append(number)
        else:
            raise CaptureError(f"domino {number} has unknown location {loc!r}")

    current.sort()
    future.sort()
    any_resolved = any(seat for seat in placed) or bool(discarded)

    # BGA's starting round: nothing has been placed and CURRENT is still empty, so the first
    # line — sitting in FUTURE — is what the kings are claiming from. That is the engine's
    # `current_line` during `StartClaim`.
    starting = not current and not any_resolved
    state = str(snap.get("state") or "")
    if starting:
        if state != "chooseDomino":
            raise CaptureError(f"starting round but BGA state is {state!r}")
        phase = "start_claim"
        line_current = future
        line_next: list[tuple[int, int | None]] = []
    elif state == "placeDomino":
        if not current:
            # Nothing left to place: BGA deals the next line in the same request, so this can
            # only be a torn read.
            raise CaptureError("placement decision with an empty current line")
        phase, line_current, line_next = "place", current, future
    elif state == "chooseDomino":
        # CURRENT legitimately empties before the round's last claim: the fourth king has
        # placed its tile (which leaves the line) and is now claiming. `drawDominoes` only
        # rotates the lines afterwards, from `activateOwnerOfNextCurrentDomino`.
        phase, line_current, line_next = "claim", current, future
    else:
        raise CaptureError(f"no advice for BGA state {state!r}")

    def slots(line, pad_front: bool) -> list[dict | None]:
        """A 4-slot engine line. Resolved tiles have already left BGA's CURRENT, so a short
        current line is padded at the FRONT with blanks — play order is ascending number, so
        whatever is gone was always earlier than whatever is left."""
        out: list[dict | None] = [None] * (LINE - len(line)) if pad_front else []
        out += [{"number": n, "owner": o} for n, o in line]
        out += [None] * (LINE - len(out))
        return out[:LINE]

    to_act = seat_of.get(str(snap.get("active_bga")))
    if to_act is None:
        raise CaptureError(f"active player {snap.get('active_bga')!r} is not at this table")

    v = snap.get("variants") or {}
    return {
        "player_count": len(players),
        "variants": {
            "harmony": bool(v.get("harmony")),
            "middle_kingdom": bool(v.get("middle_kingdom")),
        },
        "phase": phase,
        "to_act": to_act,
        "seats": [{"placed": tiles} for tiles in placed],
        "current_line": slots(line_current, pad_front=not starting),
        "next_line": slots(line_next, pad_front=False),
        "discarded": sorted(discarded),
    }


def build_game(snap: dict) -> tuple[Any, dict]:
    """Snapshot → a live `kingdomino.Game` at the pending decision, plus the seat mapping the
    panel needs to name people. Engine refusals are re-raised as `CaptureError` so the caller
    treats "I cannot read this" differently from "I do not support this table"."""
    position = snapshot_to_position(snap)
    try:
        game = kd.Game.from_position(json.dumps(position), 0)
    except ValueError as e:
        raise CaptureError(str(e)) from e
    players, seat_of = _seats(snap)
    meta = {
        "position": position,
        "names": [p.get("name") or f"seat {i + 1}" for i, p in enumerate(players)],
        "bga_ids": [str(p["bga_id"]) for p in players],
        "colors": [p.get("color") for p in players],
        "bga_scores": [int(p.get("score") or 0) for p in players],
        "you": seat_of.get(str(snap.get("seat_bga"))),
    }
    return game, meta


# --------------------------------------------------------------------------- the oracle
def check_previews(previews: list[dict], snap: dict) -> dict | None:
    """Compare the engine's legal placements against BGA's own `placementPreviews`.

    BGA sends every legal `(x, y, rotation)` **and the score it would produce**, so this is a
    free, continuous differential test of our placement *and* scoring rules against the
    reference implementation — the audit `tests/bga_parity.rs` does offline, running live on
    real positions. Returns `None` when they agree, else a description of the disagreement
    for the panel to show; advice from a position where the two disagree is not trustworthy.
    """
    bga = snap.get("bga_previews")
    if not bga:
        return None  # not a placement decision, or BGA sent no args (e.g. a forced discard)
    ours = {
        (p["x"], p["y"], p["bga_rotation"]): p["score"]
        for p in previews
        if p["type"] == "place"
    }
    theirs = {(int(p["x"]), int(p["y"]), int(p["rotation"])): int(p["score"]) for p in bga}
    missing = sorted(theirs.keys() - ours.keys())  # BGA allows, we do not
    extra = sorted(ours.keys() - theirs.keys())  # we allow, BGA does not
    mismatched = sorted(k for k in ours.keys() & theirs.keys() if ours[k] != theirs[k])
    if not (missing or extra or mismatched):
        return None
    fmt = lambda k: f"({k[0]},{k[1]}) rot {k[2]}"  # noqa: E731
    return {
        "engine_missing": [fmt(k) for k in missing[:6]],
        "engine_extra": [fmt(k) for k in extra[:6]],
        "score_mismatch": [
            f"{fmt(k)}: engine {ours[k]} vs BGA {theirs[k]}" for k in mismatched[:6]
        ],
        "counts": {"engine": len(ours), "bga": len(theirs)},
    }


# --------------------------------------------------------------------------- descriptions
def describe(action: dict, obs: dict) -> str:
    """One line a player can act on at the table.

    A placement names **which half goes where**, not a rotation number. The two flips of a
    tile occupy the same pair of cells, so "rot 2" vs "rot 0" is the entire difference between
    two moves that can be points apart — and it is the one part of the advice a player has to
    translate in their head. Naming the faces removes the translation.
    """
    kind = action["type"]
    if kind == "claim":
        n = action.get("number")
        return f"Claim {domino_desc(n)}" if n else f"Claim slot {action['slot'] + 1}"
    if kind == "place":
        cur = obs.get("current_domino") or {}
        num = cur.get("number")
        delta = action.get("score_delta")
        gain = f" (+{delta})" if delta else ""
        if not num:
            return f"Place at ({action['x']},{action['y']}){gain}"
        d = domino_table()[num - 1]
        rot = action["bga_rotation"]
        return (f"Place #{num} — {face_desc(d['a'])} at ({action['x']},{action['y']}), "
                f"{face_desc(d['b'])} {BGA_DIRECTION[rot]}{gain}")
    if kind == "discard":
        cur = obs.get("current_domino") or {}
        num = cur.get("number")
        return f"Discard {domino_desc(num)} — nowhere legal to put it" if num else "Discard"
    return kind


def highlight(action: dict, obs: dict) -> dict | None:
    """Where to draw attention on the BGA page, and **in what orientation**.

    Claims point at the tile in the draft line. Placements carry the two cells the tile would
    occupy, each tagged with the face that lands there, plus the tile number and rotation — so
    the extension can ghost the real tile art onto the board the way BGA would draw it, and
    the panel can colour the two cells by terrain. Cells alone are not enough: the two flips
    of a tile cover exactly the same squares.
    """
    if action["type"] == "claim" and action.get("number"):
        return {"kind": "domino", "number": action["number"]}
    if action["type"] == "place":
        x, y, rot = action["x"], action["y"], action["bga_rotation"]
        dx, dy = BGA_STEP[rot]
        cur = obs.get("current_domino") or {}
        num = cur.get("number")
        faces = domino_table()[num - 1] if num else {"a": None, "b": None}
        cell = lambda cx, cy, sq: {  # noqa: E731
            "x": cx, "y": cy,
            **({"terrain": sq["terrain"], "crowns": sq["crowns"]} if sq else {}),
        }
        return {
            "kind": "cells",
            "number": num,
            "rotation": rot,
            "anchor": [x, y],
            "cells": [cell(x, y, faces["a"]), cell(x + dx, y + dy, faces["b"])],
        }
    return None


def board_view(obs: dict) -> list[dict]:
    """The reconstructed kingdoms, as a compact grid the panel renders. Showing what the
    advisor *believes* the table looks like is the trust feature: a capture bug becomes
    visible at the table instead of silently steering the advice."""
    out = []
    for seat, b in enumerate(obs["seats"]):
        cells = {}
        for cell in b["cells"]:
            cells[f"{cell['r']},{cell['c']}"] = {
                "t": cell["terrain"],
                "k": cell["crowns"],
            }
        sc = obs["scores"][seat]
        out.append(
            {
                "cells": cells,
                "castle": obs["seats"][seat]["castle"],
                "bbox": [b["min_r"], b["max_r"], b["min_c"], b["max_c"]],
                "filled": b["filled"],
                # `crown_score` is the live score BGA shows. The bonuses only exist at final
                # scoring, so the total is named for what it is — a projection, true today
                # (an empty kingdom is trivially castle-symmetric, so it "has" +10) and very
                # possibly false by the end.
                "crown_score": sc["crown_score"],
                "total_if_final": sc["total"],
                "harmony": sc["harmony"],
                "middle_kingdom": sc["middle_kingdom"],
                "largest_territory": sc["largest_territory"],
            }
        )
    return out


def line_view(obs: dict) -> dict:
    """The two draft lines with tile descriptions, for the panel's context strip."""

    def one(line):
        out = []
        for slot in line:
            if slot.get("domino") is None:
                out.append(None)
            else:
                out.append(
                    {
                        "number": slot["number"],
                        "owner": slot["owner"],
                        "desc": domino_desc(slot["number"]),
                    }
                )
        return out

    return {"current": one(obs["current_line"]), "next": one(obs["next_line"])}
