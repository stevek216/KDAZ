//! Rebuild a [`GameState`] from an **observed** position (`docs/engine-design.md` §10).
//!
//! Self-play always *grows* a state from `new_game` forward, so the engine never needed to
//! start from the middle. The advisor does: it watches a live BoardGameArena table, sees a
//! position, and must hand the search an engine state that plays on identically. This module
//! is that entry point — and it lives in the **engine**, not in the advisor, because deriving
//! whose turn it is, which king is acting, and what is left in the deck are *rules* answers
//! (CLAUDE §3: the single source of rules truth; the UI never reimplements a rule).
//!
//! # What the caller must supply vs. what is derived
//!
//! Supplied (all directly observable at a BGA table): each seat's placed dominoes with their
//! anchor cell and rotation, the two draft lines with their claims, the discard pile, and
//! **which decision is pending** ([`SpecPhase`]). Everything else is derived here:
//!
//! - `remaining` — the deck is exactly "every domino nobody has seen": `FULL_DECK` minus the
//!   placed, discarded and lined ones. The draw is a chance node over *membership* only
//!   (CLAUDE §3), so an observer who knows the seen set knows the deck exactly.
//! - `round` — from how many lines have been drawn (`(48 - |remaining|) / LINE`), which is a
//!   deck fact, not a bookkeeping guess.
//! - `turn_cursor` — play order is the current line in ascending number, and a resolved
//!   domino leaves the line, so the acting king is at `LINE - unresolved` (one earlier while
//!   its owner is still claiming).
//! - `to_act` — the acting king's owner; during the starting round, from `claim_order`.
//! - `claim_order` — 2p BGA is always the snake `A,B,B,A` (CLAUDE §6), so the whole order
//!   follows from who is on the clock and how many picks are gone.
//!
//! # Validation is the point
//!
//! Every derivation is cross-checked against an independent one and a mismatch is a hard
//! [`RebuildError`], never a silently-wrong state. A capture bug in the advisor must surface
//! as "I cannot read this position", because advice computed from a subtly wrong position
//! looks exactly as confident as advice computed from a right one.

use crate::components::{domino, DominoId, NO_DOMINO, NUM_DOMINOES};
use crate::core::state::{
    Board, GameState, Phase, Slot, Variants, CENTER, FULL_DECK, GRID, LINE, MAX_PLAYERS, NO_OWNER,
    STORE,
};
use crate::rules::place::{cell_of, DIRS};

/// Which decision is pending. The observer supplies this: `Place` and `Claim` are otherwise
/// indistinguishable from the board alone (a king that has placed but not yet claimed leaves
/// the same footprint as the next king about to place).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecPhase {
    /// Starting round: a seat is claiming from `current_line`.
    StartClaim,
    /// A seat must place (or discard) the domino its king claimed.
    Place,
    /// That same seat now claims from `next_line`.
    Claim,
    /// The game is over.
    GameOver,
}

/// One placed domino, in **engine backing-store coordinates**: square `a` at `(r, c)` and
/// square `b` toward `rot` — the same convention as [`crate::core::Action::Place`], so a
/// placement observed on a table and a placement chosen by the search are the same value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedDomino {
    /// Draft number, 1..=48 (not the 0-based [`DominoId`] — this is the number a human reads
    /// off the tile and the number BGA sends).
    pub number: u8,
    pub r: u8,
    pub c: u8,
    pub rot: u8,
}

/// One draft-line slot as observed.
///
/// **A blank `number` means the slot is already resolved this round** — its domino has been
/// placed or discarded and has left the line, which is exactly what BGA shows. That is the
/// signal the rebuild uses to find the acting king, so a slot whose domino is still in play
/// must always be named. `owner` may be known even for a resolved slot (the king is still
/// visible), and is kept when it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecSlot {
    pub number: Option<u8>,
    pub owner: Option<u8>,
}

impl SpecSlot {
    /// An empty slot (no domino drawn here).
    pub const EMPTY: SpecSlot = SpecSlot {
        number: None,
        owner: None,
    };

    /// A drawn, unclaimed slot.
    pub fn open(number: u8) -> SpecSlot {
        SpecSlot {
            number: Some(number),
            owner: None,
        }
    }

    /// A drawn slot claimed by `owner`.
    pub fn claimed(number: u8, owner: u8) -> SpecSlot {
        SpecSlot {
            number: Some(number),
            owner: Some(owner),
        }
    }
}

/// An observed position. Coordinates are engine backing-store cells; the caller is responsible
/// for translating from whatever the table speaks (see the advisor's `bga.py` for BGA's
/// castle-relative `(x, y, rotation)`).
#[derive(Clone, Debug)]
pub struct PositionSpec {
    pub player_count: u8,
    pub variants: Variants,
    /// Placed dominoes per seat, indexed by seat. Order within a seat is irrelevant.
    pub placed: Vec<Vec<PlacedDomino>>,
    /// The line being placed from this round. Resolved slots come first (their dominoes have
    /// left the line), so the filled slots are the *last* `k`.
    pub current_line: [SpecSlot; LINE],
    /// The line being claimed from this round; all-empty in the final place-only round.
    pub next_line: [SpecSlot; LINE],
    /// Dominoes discarded for want of a legal placement (draft numbers).
    pub discarded: Vec<u8>,
    pub phase: SpecPhase,
    /// The seat on the clock. Required for `StartClaim` (nothing on the board identifies it);
    /// cross-checked against the acting king's owner otherwise.
    pub to_act: Option<u8>,
}

/// Why a position could not be rebuilt. Every variant means "the observation is not a state
/// this game can be in" — a capture bug, an unsupported table, or a genuine rules divergence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebuildError {
    /// Player count unsupported by the engine (must divide the 4-wide draft line).
    PlayerCount(u8),
    /// A draft number outside 1..=48.
    BadNumber(u8),
    /// The same domino observed in two places at once.
    DuplicateDomino(u8),
    /// A seat index outside `0..player_count`.
    BadSeat(u8),
    /// A rotation outside 0..=3.
    BadRotation(u8),
    /// A placement whose cell (or partner cell) falls outside the backing store.
    OffGrid { number: u8 },
    /// Two squares claim the same cell.
    Overlap { number: u8, r: u8, c: u8 },
    /// A kingdom wider or taller than `GRID`.
    BoundExceeded { seat: u8 },
    /// The filled slots of `current_line` are not the trailing ones, or a line is not sorted
    /// ascending. Play order *is* the sort order, so this would silently reorder turns.
    LineShape { line: &'static str },
    /// The seen dominoes do not make up whole drawn lines — something was observed and lost.
    /// Usually an unrecorded discard: BGA never re-sends the discard pile, so a client that
    /// joined after a discard cannot know which tile it was.
    DeckGap { seen: u32, missing_to_line: u32 },
    /// The pending decision contradicts the board (e.g. a `Claim` with no claimable line).
    PhaseMismatch { detail: &'static str },
    /// `to_act` was needed but not supplied, or disagrees with the acting king's owner.
    ToActMismatch {
        expected: Option<u8>,
        got: Option<u8>,
    },
}

impl core::fmt::Display for RebuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RebuildError::PlayerCount(n) => {
                write!(f, "unsupported player count {n} (must be 2 or 4)")
            }
            RebuildError::BadNumber(n) => write!(f, "domino number {n} outside 1..=48"),
            RebuildError::DuplicateDomino(n) => write!(f, "domino {n} observed twice"),
            RebuildError::BadSeat(s) => write!(f, "seat {s} outside the live seats"),
            RebuildError::BadRotation(r) => write!(f, "rotation {r} outside 0..=3"),
            RebuildError::OffGrid { number } => {
                write!(f, "domino {number} placed outside the backing store")
            }
            RebuildError::Overlap { number, r, c } => {
                write!(f, "domino {number} overlaps an occupied cell ({r},{c})")
            }
            RebuildError::BoundExceeded { seat } => {
                write!(f, "seat {seat}'s kingdom exceeds the {GRID}x{GRID} bound")
            }
            RebuildError::LineShape { line } => write!(f, "{line} is not a valid draft line"),
            RebuildError::DeckGap {
                seen,
                missing_to_line,
            } => write!(
                f,
                "{seen} dominoes seen is not a whole number of drawn lines \
                 ({missing_to_line} unaccounted for — an unrecorded discard?)"
            ),
            RebuildError::PhaseMismatch { detail } => write!(f, "phase mismatch: {detail}"),
            RebuildError::ToActMismatch { expected, got } => {
                write!(
                    f,
                    "to_act {got:?} disagrees with the acting king {expected:?}"
                )
            }
        }
    }
}

/// Tracks which dominoes have been seen, so "observed twice" is caught rather than silently
/// dropped from the deck twice.
struct Seen(u64);

impl Seen {
    fn new() -> Self {
        Seen(0)
    }

    /// Record draft `number`, returning its [`DominoId`].
    fn add(&mut self, number: u8) -> Result<DominoId, RebuildError> {
        if number < 1 || number as usize > NUM_DOMINOES {
            return Err(RebuildError::BadNumber(number));
        }
        let id = number - 1;
        let bit = 1u64 << id;
        if self.0 & bit != 0 {
            return Err(RebuildError::DuplicateDomino(number));
        }
        self.0 |= bit;
        Ok(id)
    }
}

/// Rebuild a [`GameState`] from an observed position, or explain why it is not a reachable one.
///
/// The returned state is play-identical to the real one: `legal_actions`, `apply_action`,
/// `chance_outcomes` and `terminal_value` all behave as they would in a game grown from
/// `new_game`. Slots of `current_line` before the cursor may be blank when the observer could
/// not name them — the turn loop never reads them again.
pub fn from_position(spec: &PositionSpec) -> Result<GameState, RebuildError> {
    let pc = spec.player_count;
    if !(2..=MAX_PLAYERS as u8).contains(&pc) || LINE % pc as usize != 0 {
        return Err(RebuildError::PlayerCount(pc));
    }
    let mut seen = Seen::new();
    let mut gs = GameState::blank();
    gs.player_count = pc;
    gs.variants = spec.variants;

    // ---- boards ----
    for seat in 0..MAX_PLAYERS {
        gs.boards[seat] = if (seat as u8) < pc {
            Board::with_castle()
        } else {
            Board::empty()
        };
    }
    if spec.placed.len() > pc as usize {
        return Err(RebuildError::BadSeat(spec.placed.len() as u8 - 1));
    }
    for (seat, tiles) in spec.placed.iter().enumerate() {
        for t in tiles {
            let id = seen.add(t.number)?;
            place_observed(&mut gs.boards[seat], id, t)?;
        }
        let b = &gs.boards[seat];
        if b.max_r - b.min_r >= GRID as u8 || b.max_c - b.min_c >= GRID as u8 {
            return Err(RebuildError::BoundExceeded { seat: seat as u8 });
        }
    }

    // ---- draft lines ----
    gs.current_line = build_line(&spec.current_line, pc, &mut seen, "current_line", true)?;
    gs.next_line = build_line(&spec.next_line, pc, &mut seen, "next_line", false)?;

    // ---- discards ----
    for &n in &spec.discarded {
        seen.add(n)?;
    }

    // ---- deck: everything nobody has seen ----
    gs.remaining = FULL_DECK & !seen.0;
    let seen_count = seen.0.count_ones();
    if seen_count % LINE as u32 != 0 {
        return Err(RebuildError::DeckGap {
            seen: seen_count,
            missing_to_line: LINE as u32 - (seen_count % LINE as u32),
        });
    }
    let lines_drawn = seen_count / LINE as u32;

    // ---- round / cursor / to_act / phase ----
    let next_filled = gs.next_line.iter().any(|s| s.is_filled());
    // A play round places the line drawn at `round + 1` and claims the one drawn at `round + 2`;
    // the final round has no line to claim from, so it is one line behind.
    gs.round = if next_filled {
        lines_drawn.saturating_sub(2)
    } else {
        lines_drawn.saturating_sub(1)
    } as u8;

    let unresolved = gs.current_line.iter().filter(|s| s.is_filled()).count();
    match spec.phase {
        SpecPhase::StartClaim => {
            if lines_drawn != 1 || next_filled {
                return Err(RebuildError::PhaseMismatch {
                    detail: "starting claim outside the first drawn line",
                });
            }
            let claimed = gs.current_line.iter().filter(|s| s.is_claimed()).count();
            if claimed >= LINE {
                return Err(RebuildError::PhaseMismatch {
                    detail: "starting round already fully claimed",
                });
            }
            gs.turn_cursor = claimed as u8;
            gs.round = 0;
            gs.phase = Phase::StartClaim;
            let to_act = spec.to_act.ok_or(RebuildError::ToActMismatch {
                expected: None,
                got: None,
            })?;
            if to_act >= pc {
                return Err(RebuildError::BadSeat(to_act));
            }
            gs.claim_order = start_claim_order(pc, claimed as u8, to_act)?;
            gs.to_act = to_act;
        }
        SpecPhase::Place | SpecPhase::Claim => {
            if spec.phase == SpecPhase::Claim && !next_filled {
                return Err(RebuildError::PhaseMismatch {
                    detail: "claiming with no next line (the final round has no claim)",
                });
            }
            // A blank slot means "already resolved" (§ the `SpecSlot` contract), so the acting
            // king is simply the first one still holding a domino — and one slot earlier while
            // its owner, having just placed, is still claiming.
            let claiming = usize::from(spec.phase == SpecPhase::Claim);
            let cursor =
                (LINE - unresolved)
                    .checked_sub(claiming)
                    .ok_or(RebuildError::PhaseMismatch {
                        detail: "claiming before any king resolved a domino this round",
                    })?;
            if cursor >= LINE {
                return Err(RebuildError::PhaseMismatch {
                    detail: "every king in the current line has already acted",
                });
            }
            // Independent cross-check: each king claims once per round right after it places,
            // so the kings already past the cursor are exactly the claims sitting on the next
            // line. A disagreement means the two lines were captured out of step.
            let next_claims = gs.next_line.iter().filter(|s| s.is_claimed()).count();
            if next_filled && next_claims != cursor {
                return Err(RebuildError::PhaseMismatch {
                    detail: "claims on the next line disagree with the acting king",
                });
            }
            gs.turn_cursor = cursor as u8;
            gs.phase = if spec.phase == SpecPhase::Place {
                Phase::Place
            } else {
                Phase::Claim
            };
            // The acting king's owner: from its slot when the observer could name it, else
            // from whoever is on the clock (a claiming king's domino has left the line, so its
            // slot is anonymous — but the seat is never in doubt).
            let slot_owner = gs.current_line[cursor].owner;
            gs.to_act = match (slot_owner, spec.to_act) {
                (NO_OWNER, Some(t)) if t < pc => t,
                (NO_OWNER, Some(t)) => return Err(RebuildError::BadSeat(t)),
                (NO_OWNER, None) => {
                    return Err(RebuildError::PhaseMismatch {
                        detail: "the acting king is identified by neither its slot nor to_act",
                    })
                }
                (owner, Some(t)) if t != owner => {
                    return Err(RebuildError::ToActMismatch {
                        expected: Some(owner),
                        got: Some(t),
                    })
                }
                (owner, _) => owner,
            };
            // Record the seat on the anonymous slot too, so the rebuilt line is as complete as
            // the observation allows.
            gs.current_line[cursor].owner = gs.to_act;
        }
        SpecPhase::GameOver => {
            if unresolved != 0 || next_filled {
                return Err(RebuildError::PhaseMismatch {
                    detail: "dominoes still in play at game over",
                });
            }
            gs.phase = Phase::GameOver;
            gs.turn_cursor = 0;
            gs.round = lines_drawn as u8;
            gs.to_act = 0;
        }
    }
    Ok(gs)
}

/// Reverse of [`from_position`]: project a state the engine already holds back to the spec an
/// observer at the table would produce. Only meaningful at player decision nodes — chance
/// nodes are mid-draw and have no observable counterpart. Used by the round-trip tests and by
/// tooling that wants to hand a live position to another process.
///
/// The projection is deliberately **lossy in exactly one place**: the current line's resolved
/// slots come back blank, because that is all anyone watching can know (BGA drops a domino
/// from the line the moment it is placed or discarded). Those slots are never read again by
/// the turn loop, so the rebuilt state still plays identically.
pub fn to_position(gs: &GameState) -> Option<PositionSpec> {
    let phase = match gs.phase {
        Phase::StartClaim => SpecPhase::StartClaim,
        Phase::Place => SpecPhase::Place,
        Phase::Claim => SpecPhase::Claim,
        Phase::GameOver => SpecPhase::GameOver,
        Phase::Draw | Phase::StartOrder => return None,
    };
    let pc = gs.player_count as usize;
    // How much of the current line this round has already consumed. The observer sees exactly
    // these slots as blank. The starting round consumes nothing: its cursor counts claims,
    // which land in any slot order and leave every domino on the table.
    let resolved = match phase {
        SpecPhase::StartClaim => 0,
        SpecPhase::GameOver => LINE,
        SpecPhase::Place => gs.turn_cursor as usize,
        SpecPhase::Claim => gs.turn_cursor as usize + 1,
    }
    .min(LINE);

    let slot_of = |s: &Slot| SpecSlot {
        number: if s.is_filled() {
            Some(s.domino + 1)
        } else {
            None
        },
        owner: if s.is_claimed() { Some(s.owner) } else { None },
    };
    let mut current_line: [SpecSlot; LINE] = core::array::from_fn(|i| slot_of(&gs.current_line[i]));
    for slot in current_line.iter_mut().take(resolved) {
        slot.number = None;
    }
    let next_line: [SpecSlot; LINE] = core::array::from_fn(|i| slot_of(&gs.next_line[i]));

    // The board stores squares, not dominoes, so each tile has to be recovered by matching its
    // two squares against the static table. Candidates are only the dominoes that have left
    // the deck and are not still *pending* in a line — that constraint is what makes the
    // recovery agree with reality when several identical tiles exist (six plain forests, for
    // instance). Note that a resolved slot's domino stays in the engine's line but is on a
    // board, so it must remain a candidate. The advisor never needs any of this: BGA hands it
    // the numbers outright.
    let mut pool = !gs.remaining & FULL_DECK;
    for line in [&current_line, &next_line] {
        for n in line.iter().filter_map(|s| s.number) {
            pool &= !(1u64 << (n - 1));
        }
    }
    let placed = recover_placements(&gs.boards[..pc], &mut pool)?;
    // Whatever has left the deck but sits on no kingdom and in no visible line was discarded.
    // Resolved slots are blank above, so a tile discarded this round lands here rather than
    // vanishing — that accounting is what lets the rebuild reconstruct the deck exactly. The
    // recovery consumed every placed tile from `pool`, so what remains is exactly the pile.
    let discarded = (0..NUM_DOMINOES as u8)
        .filter(|id| pool & (1u64 << id) != 0)
        .map(|id| id + 1)
        .collect();
    Some(PositionSpec {
        player_count: gs.player_count,
        variants: gs.variants,
        placed,
        current_line,
        next_line,
        discarded,
        phase,
        to_act: Some(gs.to_act),
    })
}

/// Recover which domino sits where on every board: find the tiling of each kingdom's squares
/// into domino-shaped pairs drawn from `pool` (the tiles known to have left the deck),
/// consuming what it uses.
///
/// A greedy sweep is not enough — a locally valid pairing can leave the rest unsolvable — so
/// this backtracks, and it does so **across boards together**: the seats share one pool, and a
/// tiling of one kingdom that starves another has to be undone. The search stays cheap because
/// it is tightly constrained: the first unclaimed square in scan order has its left and upper
/// neighbours already claimed, so only two partners and two orientations are ever in play.
fn recover_placements(boards: &[Board], pool: &mut u64) -> Option<Vec<Vec<PlacedDomino>>> {
    let mut used = vec![[[false; STORE]; STORE]; boards.len()];
    let mut out = vec![Vec::new(); boards.len()];
    // A guard against pathological blowup on a hand-built board; real kingdoms resolve in a
    // handful of steps because the tile set constrains almost every pairing.
    let mut budget = 200_000usize;
    if solve_tiling(boards, 0, &mut used, pool, &mut out, &mut budget) {
        Some(out)
    } else {
        None
    }
}

fn solve_tiling(
    boards: &[Board],
    seat: usize,
    used: &mut [[[bool; STORE]; STORE]],
    pool: &mut u64,
    out: &mut [Vec<PlacedDomino>],
    budget: &mut usize,
) -> bool {
    if seat >= boards.len() {
        return true;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    let b = &boards[seat];
    // The first square still needing a partner; none left means this kingdom is fully tiled
    // and the next seat can start.
    let Some((r, c)) = (b.present)
        .then(|| {
            (b.min_r..=b.max_r)
                .flat_map(|r| (b.min_c..=b.max_c).map(move |c| (r, c)))
                .find(|&(r, c)| {
                    let cell = b.cell(r, c);
                    !cell.is_empty() && !cell.is_castle() && !used[seat][r as usize][c as usize]
                })
        })
        .flatten()
    else {
        return solve_tiling(boards, seat + 1, used, pool, out, budget);
    };
    let cell = b.cell(r, c);
    let Some(ta) = cell.terrain_of() else {
        return false;
    };
    for &rot in &[1u8, 2u8] {
        let (dr, dc) = DIRS[rot as usize];
        let (nr, nc) = (r as i16 + dr as i16, c as i16 + dc as i16);
        if nr < 0 || nc < 0 || nr >= STORE as i16 || nc >= STORE as i16 {
            continue;
        }
        let (nr, nc) = (nr as u8, nc as u8);
        let other = b.cell(nr, nc);
        if other.is_empty() || other.is_castle() || used[seat][nr as usize][nc as usize] {
            continue;
        }
        let Some(tb) = other.terrain_of() else {
            continue;
        };
        // Either square can be the domino's `a`; the anchor and rotation follow from which.
        for &(a_here, anchor, arot) in &[
            (true, (r, c), rot),
            (false, (nr, nc), (rot + 2) % 4), // anchored at the partner, pointing back
        ] {
            let ((fa, ca), (fb, cb)) = if a_here {
                ((ta, cell.crowns()), (tb, other.crowns()))
            } else {
                ((tb, other.crowns()), (ta, cell.crowns()))
            };
            for id in 0..NUM_DOMINOES as u8 {
                if *pool & (1u64 << id) == 0 {
                    continue;
                }
                let d = domino(id);
                if d.a.terrain != fa || d.a.crowns != ca || d.b.terrain != fb || d.b.crowns != cb {
                    continue;
                }
                *pool &= !(1u64 << id);
                used[seat][r as usize][c as usize] = true;
                used[seat][nr as usize][nc as usize] = true;
                out[seat].push(PlacedDomino {
                    number: id + 1,
                    r: anchor.0,
                    c: anchor.1,
                    rot: arot,
                });
                if solve_tiling(boards, seat, used, pool, out, budget) {
                    return true;
                }
                out[seat].pop();
                used[seat][r as usize][c as usize] = false;
                used[seat][nr as usize][nc as usize] = false;
                *pool |= 1u64 << id;
                // Identical tiles are interchangeable, so if this id failed so will its twins;
                // stop at the first candidate of each distinct face pair.
                break;
            }
        }
    }
    false
}

/// Apply one observed placement to a board, checking only what an observer can check: the
/// cells exist and are free. Connection is *not* re-checked — it was legal when played, and a
/// finished kingdom carries no record of the order that made it legal.
fn place_observed(b: &mut Board, id: DominoId, t: &PlacedDomino) -> Result<(), RebuildError> {
    if t.rot > 3 {
        return Err(RebuildError::BadRotation(t.rot));
    }
    if t.r as usize >= STORE || t.c as usize >= STORE {
        return Err(RebuildError::OffGrid { number: t.number });
    }
    let (dr, dc) = DIRS[t.rot as usize];
    let (br, bc) = (t.r as i16 + dr as i16, t.c as i16 + dc as i16);
    if br < 0 || bc < 0 || br >= STORE as i16 || bc >= STORE as i16 {
        return Err(RebuildError::OffGrid { number: t.number });
    }
    let (br, bc) = (br as u8, bc as u8);
    for &(r, c) in &[(t.r, t.c), (br, bc)] {
        if !b.cell(r, c).is_empty() {
            return Err(RebuildError::Overlap {
                number: t.number,
                r,
                c,
            });
        }
    }
    let def = domino(id);
    b.place_square(t.r, t.c, def.a.terrain, def.a.crowns);
    b.place_square(br, bc, def.b.terrain, def.b.crowns);
    Ok(())
}

/// Turn observed slots into an engine line, enforcing the shape the turn loop assumes: filled
/// slots ascending by number, no holes, and — for `current_line` — any blanks packed at the
/// *front*, because a resolved domino always had a smaller number than every one still waiting.
fn build_line(
    spec: &[SpecSlot; LINE],
    pc: u8,
    seen: &mut Seen,
    name: &'static str,
    leading_blanks_ok: bool,
) -> Result<[Slot; LINE], RebuildError> {
    let mut out = [Slot::EMPTY; LINE];
    let mut last: Option<u8> = None;
    let mut started = false;
    for s in spec.iter() {
        match s.number {
            // A blank after a filled slot is a hole — never valid. A blank before one is a
            // resolved slot, which only the current line can have.
            None if started => return Err(RebuildError::LineShape { line: name }),
            None if !leading_blanks_ok && spec.iter().any(|o| o.number.is_some()) => {
                return Err(RebuildError::LineShape { line: name })
            }
            None => {}
            Some(n) => {
                started = true;
                if last.is_some_and(|prev| n <= prev) {
                    return Err(RebuildError::LineShape { line: name });
                }
                last = Some(n);
                seen.add(n)?;
            }
        }
    }
    // Second pass, once the whole line is known to be well-shaped: slots keep the indices the
    // observer gave them, so the cursor finds the acting king where the turn loop expects it.
    // A resolved slot keeps its owner when known — the king is still on the table even though
    // its domino has gone.
    for (i, s) in spec.iter().enumerate() {
        let owner = match s.owner {
            None => NO_OWNER,
            Some(o) if o < pc => o,
            Some(o) => return Err(RebuildError::BadSeat(o)),
        };
        out[i] = Slot {
            domino: s.number.map_or(NO_DOMINO, |n| n - 1),
            owner,
        };
    }
    Ok(out)
}

/// The starting-round claim order, given how many picks are gone and who is on the clock.
///
/// 2p BGA is always the snake `A,B,B,A` — `activateOwnerOfNextKing()` skips the player advance
/// after the second claim, so the first player takes picks 1 and 4 (CLAUDE §6, `turn.rs`
/// `nth_start_order`). That makes the whole order recoverable from a single observation.
fn start_claim_order(pc: u8, claimed: u8, to_act: u8) -> Result<[u8; LINE], RebuildError> {
    if pc != 2 {
        // 4p claims once per seat in the random seating order; a single (cursor, to_act)
        // observation does not determine it. The advisor targets 2p (CLAUDE §1).
        return Err(RebuildError::PlayerCount(pc));
    }
    // Snake positions 0 and 3 belong to the first seat, 1 and 2 to the second.
    let first = if claimed == 0 || claimed == 3 {
        to_act
    } else {
        1 - to_act
    };
    let second = 1 - first;
    Ok([first, second, second, first])
}

/// Convert a castle-relative `(x, y)` — BGA's coordinates, castle at `(0,0)`, `+x` right and
/// `+y` **up** the screen — to an engine backing-store cell. Rows grow downward, so the
/// engine's row-major grid renders exactly as the table looks.
pub fn cell_from_xy(x: i16, y: i16) -> Option<(u8, u8)> {
    let r = CENTER as i16 - y;
    let c = CENTER as i16 + x;
    if r < 0 || c < 0 || r >= STORE as i16 || c >= STORE as i16 {
        return None;
    }
    Some((r as u8, c as u8))
}

/// Inverse of [`cell_from_xy`].
pub fn xy_from_cell(r: u8, c: u8) -> (i16, i16) {
    (c as i16 - CENTER as i16, CENTER as i16 - r as i16)
}

/// Convert BGA's rotation (`0`=+x, `1`=−y, `2`=−x, `3`=+y — `getRightTerrainCoordinates` in
/// `kingdomino.game.php`) to the engine's `rot` (`0`=up, `1`=right, `2`=down, `3`=left).
pub fn rot_from_bga(rotation: u8) -> u8 {
    (rotation + 1) % 4
}

/// Inverse of [`rot_from_bga`].
pub fn rot_to_bga(rot: u8) -> u8 {
    (rot + 3) % 4
}

/// Decode a [`crate::core::Action::Place`] anchor into BGA's `(x, y, rotation)` — what the
/// advisor must say out loud for a recommendation to be executable at the table.
pub fn place_to_bga(anchor: u16, rot: u8) -> (i16, i16, u8) {
    let (r, c) = cell_of(anchor);
    let (x, y) = xy_from_cell(r, c);
    (x, y, rot_to_bga(rot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::setup::new_game;
    use crate::core::turn::{
        apply_chance, chance_outcomes, current_decision, is_chance, is_terminal, legal_actions,
    };
    use crate::core::{Action, Decision};
    use crate::rules::place::anchor_of;
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Everything an observer can see must come back identical; the resolved slots of the
    /// current line are the one documented blind spot.
    fn assert_observationally_equal(rebuilt: &GameState, gs: &GameState, ctx: &str) {
        assert_eq!(rebuilt.player_count, gs.player_count, "{ctx}: player_count");
        assert_eq!(rebuilt.variants, gs.variants, "{ctx}: variants");
        assert_eq!(rebuilt.phase, gs.phase, "{ctx}: phase");
        assert_eq!(rebuilt.to_act, gs.to_act, "{ctx}: to_act");
        assert_eq!(rebuilt.round, gs.round, "{ctx}: round");
        assert_eq!(rebuilt.turn_cursor, gs.turn_cursor, "{ctx}: turn_cursor");
        assert_eq!(rebuilt.remaining, gs.remaining, "{ctx}: deck");
        assert_eq!(rebuilt.boards, gs.boards, "{ctx}: boards");
        assert_eq!(rebuilt.next_line, gs.next_line, "{ctx}: next_line");
        assert_eq!(rebuilt.draw_count, gs.draw_count, "{ctx}: draw_count");
        assert_eq!(rebuilt.claim_order, gs.claim_order, "{ctx}: claim_order");
        // The pending part of the current line — the slots that still decide play — must match
        // exactly, owners included. A king in `Claim` has already spent its own domino, so its
        // slot counts as resolved along with everything before it.
        let from = (rebuilt.turn_cursor as usize + usize::from(gs.phase == Phase::Claim)).min(LINE);
        assert_eq!(
            rebuilt.current_line[from..],
            gs.current_line[from..],
            "{ctx}: pending current_line"
        );
    }

    /// Play whole games with random legal moves and, at *every* player node, project the state
    /// to a position spec and rebuild it. The rebuilt state must be observationally identical
    /// **and** offer exactly the same legal actions — the strongest available statement that
    /// the advisor reasons about the same game the table is playing.
    #[test]
    fn round_trip_at_every_node() {
        let mut a_buf = Vec::new();
        let mut b_buf = Vec::new();
        let mut checked = 0usize;
        for seed in 0..40u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut gs = new_game(2);
            loop {
                match current_decision(&gs) {
                    Decision::Chance => {
                        apply_chance(&mut gs, &mut rng);
                        continue;
                    }
                    Decision::Terminal => break,
                    Decision::Player(_) => {}
                }
                let spec = to_position(&gs).expect("player node projects to a spec");
                let rebuilt = from_position(&spec)
                    .unwrap_or_else(|e| panic!("seed {seed}: rebuild failed: {e}"));
                assert_observationally_equal(&rebuilt, &gs, &format!("seed {seed}"));
                legal_actions(&gs, &mut a_buf);
                legal_actions(&rebuilt, &mut b_buf);
                assert_eq!(a_buf, b_buf, "seed {seed}: legal actions diverged");
                checked += 1;

                let a = a_buf[rng.gen_range(0..a_buf.len())];
                crate::core::apply_action(&mut gs, a);
            }
            // Terminal states project and rebuild too — that is where the advisor reads scores.
            let spec = to_position(&gs).expect("terminal projects");
            let rebuilt = from_position(&spec).unwrap();
            assert_observationally_equal(&rebuilt, &gs, &format!("seed {seed} terminal"));
            assert_eq!(
                crate::core::terminal_value(&rebuilt),
                crate::core::terminal_value(&gs)
            );
        }
        assert!(checked > 2000, "expected thousands of nodes, saw {checked}");
    }

    /// Rebuilding is not just a snapshot: the rebuilt state must keep playing the same game.
    /// Drive both forward through a full game applying identical actions and identical sampled
    /// draws, re-deriving nothing — any divergence in the hidden bookkeeping shows up here.
    #[test]
    fn rebuilt_state_plays_a_whole_game_in_lockstep() {
        for seed in 0..8u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(1000 + seed);
            let mut gs = new_game(2);
            // Advance to the first player node, then fork a rebuilt copy and run both.
            while matches!(current_decision(&gs), Decision::Chance) {
                apply_chance(&mut gs, &mut rng);
            }
            let mut forked = from_position(&to_position(&gs).unwrap()).unwrap();
            let (mut a_buf, mut b_buf) = (Vec::new(), Vec::new());
            loop {
                assert_eq!(
                    is_terminal(&gs),
                    is_terminal(&forked),
                    "terminality diverged"
                );
                if is_terminal(&gs) {
                    break;
                }
                if is_chance(&gs) {
                    assert!(is_chance(&forked), "chance nodes diverged");
                    // Same outcome into both, so any difference is bookkeeping, not luck.
                    let outs = chance_outcomes(&gs);
                    let (a, _) = outs[rng.gen_range(0..outs.len())];
                    crate::core::apply_action(&mut gs, a);
                    crate::core::apply_action(&mut forked, a);
                    continue;
                }
                legal_actions(&gs, &mut a_buf);
                legal_actions(&forked, &mut b_buf);
                assert_eq!(a_buf, b_buf, "seed {seed}: legal actions diverged mid-game");
                assert_eq!(gs.to_act, forked.to_act, "seed {seed}: to_act diverged");
                let a = a_buf[rng.gen_range(0..a_buf.len())];
                crate::core::apply_action(&mut gs, a);
                crate::core::apply_action(&mut forked, a);
            }
            assert_eq!(
                crate::core::terminal_value(&gs),
                crate::core::terminal_value(&forked),
                "seed {seed}: final scores diverged"
            );
        }
    }

    fn base_spec() -> PositionSpec {
        PositionSpec {
            player_count: 2,
            variants: Variants::MIGHTY_DUEL,
            placed: vec![Vec::new(), Vec::new()],
            current_line: [
                SpecSlot::open(1),
                SpecSlot::open(2),
                SpecSlot::open(3),
                SpecSlot::open(4),
            ],
            next_line: [SpecSlot::EMPTY; LINE],
            discarded: Vec::new(),
            phase: SpecPhase::StartClaim,
            to_act: Some(0),
        }
    }

    #[test]
    fn starting_position_matches_a_dealt_game() {
        let gs = from_position(&base_spec()).unwrap();
        assert_eq!(gs.phase, Phase::StartClaim);
        assert_eq!(gs.round, 0);
        assert_eq!(gs.turn_cursor, 0);
        assert_eq!(gs.to_act, 0);
        assert_eq!(gs.deck_remaining(), 44);
        // BGA's snake: first player takes picks 1 and 4.
        assert_eq!(gs.claim_order, [0, 1, 1, 0]);
        assert!(gs.boards[0].present && gs.boards[1].present);
        assert_eq!(gs.boards[0].filled, 0);
    }

    #[test]
    fn snake_order_recovered_from_any_pick() {
        // Seat 1 first: order 1,0,0,1. Observed at each cursor, the whole order comes back.
        for (claimed, to_act) in [(0u8, 1u8), (1, 0), (2, 0), (3, 1)] {
            let mut spec = base_spec();
            spec.to_act = Some(to_act);
            let owners = [1u8, 0, 0, 1];
            for (slot, &owner) in spec
                .current_line
                .iter_mut()
                .zip(owners.iter())
                .take(claimed as usize)
            {
                slot.owner = Some(owner);
            }
            let gs = from_position(&spec).unwrap();
            assert_eq!(gs.claim_order, [1, 0, 0, 1], "claimed={claimed}");
            assert_eq!(gs.turn_cursor, claimed);
            assert_eq!(gs.to_act, to_act);
        }
    }

    #[test]
    fn rejects_a_domino_seen_twice() {
        let mut spec = base_spec();
        spec.placed[0].push(PlacedDomino {
            number: 1,
            r: CENTER,
            c: CENTER + 1,
            rot: 1,
        });
        assert_eq!(from_position(&spec), Err(RebuildError::DuplicateDomino(1)));
    }

    #[test]
    fn rejects_an_unrecorded_discard() {
        // A resolved slot whose domino was never recorded on a kingdom or in the discard pile:
        // the seen set is no longer a whole number of drawn lines. That is exactly the blind
        // spot of a client that joined after a discard — better to refuse than to advise from
        // a deck that still contains a tile nobody can draw.
        let mut spec = base_spec();
        spec.phase = SpecPhase::Place;
        spec.current_line[0] = SpecSlot::EMPTY;
        spec.current_line[1].owner = Some(0);
        match from_position(&spec) {
            Err(RebuildError::DeckGap { seen, .. }) => assert_eq!(seen, 3),
            other => panic!("expected a deck gap, got {other:?}"),
        }
    }

    #[test]
    fn rejects_overlapping_placements() {
        let mut spec = base_spec();
        spec.phase = SpecPhase::Place;
        spec.placed[0].push(PlacedDomino {
            number: 5,
            r: CENTER,
            c: CENTER + 1,
            rot: 1,
        });
        spec.placed[0].push(PlacedDomino {
            number: 6,
            r: CENTER,
            c: CENTER + 2,
            rot: 1,
        });
        assert!(matches!(
            from_position(&spec),
            Err(RebuildError::Overlap { number: 6, .. })
        ));
    }

    #[test]
    fn rejects_a_kingdom_wider_than_the_grid() {
        let mut spec = base_spec();
        spec.phase = SpecPhase::Place;
        spec.current_line[0].owner = Some(0);
        // Two tiles far apart on the same row: columns 0..8 span 9, past the 7-wide bound,
        // while both stay inside the 13-wide backing store (so this is a bound failure, not
        // an off-grid one).
        spec.placed[0].push(PlacedDomino {
            number: 5,
            r: CENTER,
            c: CENTER + 2,
            rot: 1,
        });
        spec.placed[0].push(PlacedDomino {
            number: 6,
            r: CENTER,
            c: CENTER - 5,
            rot: 3,
        });
        assert_eq!(
            from_position(&spec),
            Err(RebuildError::BoundExceeded { seat: 0 })
        );
    }

    #[test]
    fn rejects_an_out_of_order_line() {
        let mut spec = base_spec();
        spec.current_line[1] = SpecSlot::open(9);
        spec.current_line[2] = SpecSlot::open(2);
        assert_eq!(
            from_position(&spec),
            Err(RebuildError::LineShape {
                line: "current_line"
            })
        );
    }

    #[test]
    fn bga_coordinate_mapping_round_trips() {
        // The castle is the origin in both systems.
        assert_eq!(cell_from_xy(0, 0), Some((CENTER, CENTER)));
        // +x is right (column up), +y is up the screen (row down).
        assert_eq!(cell_from_xy(1, 0), Some((CENTER, CENTER + 1)));
        assert_eq!(cell_from_xy(0, 1), Some((CENTER - 1, CENTER)));
        for x in -6i16..=6 {
            for y in -6i16..=6 {
                let (r, c) = cell_from_xy(x, y).unwrap();
                assert_eq!(xy_from_cell(r, c), (x, y));
            }
        }
        // BGA rotation 0 (+x) is the engine's "right"; each maps to the same neighbour cell.
        for bga_rot in 0..4u8 {
            let rot = rot_from_bga(bga_rot);
            assert_eq!(rot_to_bga(rot), bga_rot);
            let (x, y) = (1i16, -1i16);
            let (r, c) = cell_from_xy(x, y).unwrap();
            let (dr, dc) = DIRS[rot as usize];
            let bga_partner = match bga_rot {
                0 => (x + 1, y),
                1 => (x, y - 1),
                2 => (x - 1, y),
                _ => (x, y + 1),
            };
            assert_eq!(
                cell_from_xy(bga_partner.0, bga_partner.1),
                Some(((r as i16 + dr as i16) as u8, (c as i16 + dc as i16) as u8))
            );
        }
    }

    #[test]
    fn place_action_translates_to_bga_coordinates() {
        let anchor = anchor_of(CENTER, CENTER + 1);
        let (x, y, rotation) = place_to_bga(anchor, 1);
        assert_eq!((x, y), (1, 0));
        assert_eq!(rotation, 0); // engine "right" is BGA's +x
                                 // And an engine Place round-trips through the BGA form.
        let (r, c) = cell_from_xy(x, y).unwrap();
        assert_eq!(
            Action::Place {
                anchor: anchor_of(r, c),
                rot: rot_from_bga(rotation)
            },
            Action::Place { anchor, rot: 1 }
        );
    }
}
