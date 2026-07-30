//! Packed binary corpus records - the on-disk training corpus.
//!
//! A record is a [`GameState`] plus its training targets. The observation and the
//! legal-action list are **pure functions of the state**, so neither is stored: the loader
//! re-derives them with the same `legal_actions` the generator used. That is what makes this
//! ~46x smaller in memory than the JSONL-of-dicts it replaces (434 B/record + 4 B/action vs
//! 18.4 KiB as Python objects), and it makes feature-schema drift impossible - improving the
//! encoder never invalidates a corpus.
//!
//! The stored `n_actions` is the integrity check: the policy target is a distribution over
//! `legal_actions` **in enumeration order**, so if the engine's action enumeration ever
//! changes, the re-derived count stops matching and the loader fails loudly instead of
//! training on silently misaligned targets.
//!
//! # File layout (little-endian)
//!
//! ```text
//! [ 32-byte header ][ n_records * record_size ][ total_actions * f32 policy ]
//!
//! header:   0  magic "KDC1"    (4)      16  n_records u64      (8)
//!           4  version u32     (4)      24  policy_offset u64  (8)
//!           8  player_count u32(4)
//!          12  record_size u32 (4)      = record_size(player_count)
//!
//! record:   0  player_count u8          18  current_line (domino u8, owner u8) x4  (8)
//!           1  phase u8                 26  next_line                              (8)
//!           2  to_act u8                34  claim_order                            (4)
//!           3  round u8                 38  draw_buf                               (4)
//!           4  turn_cursor u8           42  value  f32 x4                          (16)
//!           5  draw_count u8            58  scores f32 x4                          (16)
//!           6  flags u8                 74  game_id u64                            (8)
//!           7  (pad)                    82  boards (176 each x player_count)
//!           8  n_actions u16
//!          10  remaining u64
//!
//! flags:  bit0 harmony, bit1 middle_kingdom, bit2 has_scores
//! board (176): 169 cells, then min_r, max_r, min_c, max_c, filled u16, present u8
//! cell byte:   0 = empty, 1..=6 = terrain index + 1, 7 = castle;  crowns in bits 3-4
//! ```
//!
//! Policies are stored ragged (each record contributes `n_actions` floats, concatenated in
//! record order), so the reader recovers offsets with a cumulative sum over the `n_actions`
//! column - no per-record index needs storing.

use kingdomino_engine::components::{Terrain, NO_DOMINO};
use kingdomino_engine::core::{
    Board, Cell, GameState, Phase, Slot, Variants, LINE, MAX_PLAYERS, STORE,
};

/// Bumped whenever the byte layout changes. v2 appends the MCTS root value AFTER the boards,
/// so a v1 record is a byte-identical prefix of a v2 one and every v1 reader path still works.
pub const FORMAT_VERSION: u32 = 2;
pub const MAGIC: &[u8; 4] = b"KDC1";
pub const HEADER_BYTES: usize = 32;

const BOARD_BYTES: usize = STORE * STORE + 4 + 2 + 1; // cells + bbox + filled + present
const REC_HEAD: usize = 82;
/// v2 tail: the search's backed-up root value, f32 per seat.
const ROOT_BYTES: usize = 4 * MAX_PLAYERS;

// Field offsets within a record.
const O_PC: usize = 0;
const O_PHASE: usize = 1;
const O_TO_ACT: usize = 2;
const O_ROUND: usize = 3;
const O_CURSOR: usize = 4;
const O_DRAW_COUNT: usize = 5;
const O_FLAGS: usize = 6;
const O_N_ACTIONS: usize = 8;
const O_REMAINING: usize = 10;
const O_CUR_LINE: usize = 18;
const O_NEXT_LINE: usize = 26;
const O_CLAIM_ORDER: usize = 34;
const O_DRAW_BUF: usize = 38;
const O_VALUE: usize = 42;
const O_SCORES: usize = 58;
const O_GAME: usize = 74;
const O_BOARDS: usize = 82;

const F_HARMONY: u8 = 1;
const F_MIDDLE: u8 = 2;
const F_HAS_SCORES: u8 = 4;
const F_HAS_ROOT: u8 = 8;

/// Bytes per record for a `pc`-seat corpus at the current format (450 for the 2-player target).
pub fn record_size(pc: usize) -> usize {
    record_size_v(pc, FORMAT_VERSION)
}

/// Bytes per record for a given format version — v1 is 434 for 2p, v2 adds the 16-byte root
/// value. Kept explicit so the reader can still map corpora written before v2.
pub fn record_size_v(pc: usize, version: u32) -> usize {
    REC_HEAD + BOARD_BYTES * pc + if version >= 2 { ROOT_BYTES } else { 0 }
}

/// Offset of the v2 root-value tail for a `pc`-seat record.
fn root_offset(pc: usize) -> usize {
    REC_HEAD + BOARD_BYTES * pc
}

// --------------------------------------------------------------------------- primitives

fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn get_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn get_u64(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}
fn put_f32(b: &mut [u8], at: usize, v: f32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn get_f32(b: &[u8], at: usize) -> f32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    f32::from_le_bytes(a)
}

fn phase_code(p: Phase) -> u8 {
    match p {
        Phase::Draw => 0,
        Phase::StartOrder => 1,
        Phase::StartClaim => 2,
        Phase::Place => 3,
        Phase::Claim => 4,
        Phase::GameOver => 5,
    }
}

fn phase_from(code: u8) -> Phase {
    match code {
        0 => Phase::Draw,
        1 => Phase::StartOrder,
        2 => Phase::StartClaim,
        3 => Phase::Place,
        4 => Phase::Claim,
        _ => Phase::GameOver,
    }
}

/// Our own cell byte (deliberately *not* the engine's internal `Cell` packing, so a change to
/// the engine's bit layout can never silently reinterpret existing corpus files).
fn cell_byte(c: Cell) -> u8 {
    if c.is_castle() {
        7
    } else if let Some(t) = c.terrain_of() {
        (t.index() + 1) | (c.crowns() << 3)
    } else {
        0
    }
}

fn cell_from(byte: u8) -> Cell {
    let code = byte & 0b111;
    let crowns = (byte >> 3) & 0b11;
    match code {
        0 => Cell::EMPTY,
        7 => Cell::CASTLE,
        t => match Terrain::from_index(t - 1) {
            Some(terrain) => Cell::terrain(terrain, crowns),
            None => Cell::EMPTY,
        },
    }
}

// --------------------------------------------------------------------------- board

fn put_board(out: &mut [u8], b: &Board) {
    for r in 0..STORE {
        for c in 0..STORE {
            out[r * STORE + c] = cell_byte(b.cell(r as u8, c as u8));
        }
    }
    let o = STORE * STORE;
    out[o] = b.min_r;
    out[o + 1] = b.max_r;
    out[o + 2] = b.min_c;
    out[o + 3] = b.max_c;
    put_u16(out, o + 4, b.filled);
    out[o + 6] = b.present as u8;
}

fn get_board(src: &[u8]) -> Board {
    let mut b = Board::empty();
    for r in 0..STORE {
        for c in 0..STORE {
            b.cells[r][c] = cell_from(src[r * STORE + c]);
        }
    }
    let o = STORE * STORE;
    b.min_r = src[o];
    b.max_r = src[o + 1];
    b.min_c = src[o + 2];
    b.max_c = src[o + 3];
    b.filled = get_u16(src, o + 4);
    b.present = src[o + 6] != 0;
    b
}

// --------------------------------------------------------------------------- record

/// Serialize one training record into `out` (exactly [`record_size`] bytes are written).
///
/// `value` and `scores` are **absolute per-seat** vectors (seat `i` = seat `i`); the loader
/// rotates them to be seat-relative using the record's `to_act`. `scores` is `None` for
/// corpora generated before the auxiliary final-score target existed. `game_id` groups the
/// positions of one game so the trainer can hold out whole games.
pub fn pack_record(
    out: &mut [u8],
    gs: &GameState,
    value: &[f32; MAX_PLAYERS],
    scores: Option<&[f32; MAX_PLAYERS]>,
    n_actions: u16,
    game_id: u64,
    root_value: Option<&[f32; MAX_PLAYERS]>,
) {
    let pc = gs.player_count as usize;
    // A v1-sized buffer is legal as long as no root value is being written — v2 only appends,
    // so the shorter buffer simply omits the tail.
    debug_assert!(
        out.len() >= record_size_v(pc, 1),
        "record buffer {} too small for {pc}p",
        out.len()
    );
    debug_assert!(
        root_value.is_none() || out.len() >= record_size(pc),
        "root value needs a v2-sized buffer"
    );
    out.fill(0);

    out[O_PC] = gs.player_count;
    out[O_PHASE] = phase_code(gs.phase);
    out[O_TO_ACT] = gs.to_act;
    out[O_ROUND] = gs.round;
    out[O_CURSOR] = gs.turn_cursor;
    out[O_DRAW_COUNT] = gs.draw_count;
    let mut flags = 0u8;
    if gs.variants.harmony {
        flags |= F_HARMONY;
    }
    if gs.variants.middle_kingdom {
        flags |= F_MIDDLE;
    }
    if scores.is_some() {
        flags |= F_HAS_SCORES;
    }
    if root_value.is_some() {
        flags |= F_HAS_ROOT;
    }
    out[O_FLAGS] = flags;
    put_u16(out, O_N_ACTIONS, n_actions);
    put_u64(out, O_REMAINING, gs.remaining);

    for (i, s) in gs.current_line.iter().enumerate() {
        out[O_CUR_LINE + i * 2] = s.domino;
        out[O_CUR_LINE + i * 2 + 1] = s.owner;
    }
    for (i, s) in gs.next_line.iter().enumerate() {
        out[O_NEXT_LINE + i * 2] = s.domino;
        out[O_NEXT_LINE + i * 2 + 1] = s.owner;
    }
    out[O_CLAIM_ORDER..O_CLAIM_ORDER + 4].copy_from_slice(&gs.claim_order);
    out[O_DRAW_BUF..O_DRAW_BUF + 4].copy_from_slice(&gs.draw_buf);

    put_u64(out, O_GAME, game_id);
    for k in 0..MAX_PLAYERS {
        put_f32(out, O_VALUE + k * 4, value[k]);
        put_f32(out, O_SCORES + k * 4, scores.map_or(0.0, |s| s[k]));
    }
    for seat in 0..pc {
        let at = O_BOARDS + seat * BOARD_BYTES;
        put_board(&mut out[at..at + BOARD_BYTES], &gs.boards[seat]);
    }
    if let Some(rv) = root_value {
        let at = root_offset(pc);
        for (k, v) in rv.iter().enumerate() {
            put_f32(out, at + k * 4, *v);
        }
    }
}

/// The search's backed-up root value for this record, or `None` for a v1 record (or one
/// written without it). Absolute per seat, same [0,1] win-probability convention as `value`.
pub fn unpack_root_value(rec: &[u8]) -> Option<[f32; MAX_PLAYERS]> {
    let pc = rec[O_PC] as usize;
    let at = root_offset(pc);
    if rec[O_FLAGS] & F_HAS_ROOT == 0 || rec.len() < at + ROOT_BYTES {
        return None;
    }
    let mut rv = [0f32; MAX_PLAYERS];
    for (k, v) in rv.iter_mut().enumerate() {
        *v = get_f32(rec, at + k * 4);
    }
    Some(rv)
}

/// The `GameState` stored in `rec`. Seats beyond `player_count` come back as absent boards,
/// matching how `new_game` builds them.
pub fn unpack_state(rec: &[u8]) -> GameState {
    let pc = rec[O_PC] as usize;
    // Built from public fields rather than the engine's crate-private `blank()`.
    let mut gs = GameState {
        player_count: rec[O_PC],
        variants: Variants::NONE,
        phase: phase_from(rec[O_PHASE]),
        to_act: rec[O_TO_ACT],
        round: rec[O_ROUND],
        current_line: [Slot::EMPTY; LINE],
        next_line: [Slot::EMPTY; LINE],
        turn_cursor: rec[O_CURSOR],
        claim_order: [0; LINE],
        remaining: get_u64(rec, O_REMAINING),
        draw_buf: [NO_DOMINO; LINE],
        draw_count: rec[O_DRAW_COUNT],
        boards: [Board::empty(); MAX_PLAYERS],
    };
    let flags = rec[O_FLAGS];
    gs.variants = Variants {
        harmony: flags & F_HARMONY != 0,
        middle_kingdom: flags & F_MIDDLE != 0,
    };

    for i in 0..LINE {
        gs.current_line[i] = Slot {
            domino: rec[O_CUR_LINE + i * 2],
            owner: rec[O_CUR_LINE + i * 2 + 1],
        };
        gs.next_line[i] = Slot {
            domino: rec[O_NEXT_LINE + i * 2],
            owner: rec[O_NEXT_LINE + i * 2 + 1],
        };
    }
    gs.claim_order
        .copy_from_slice(&rec[O_CLAIM_ORDER..O_CLAIM_ORDER + 4]);
    gs.draw_buf
        .copy_from_slice(&rec[O_DRAW_BUF..O_DRAW_BUF + 4]);

    for seat in 0..pc {
        let at = O_BOARDS + seat * BOARD_BYTES;
        gs.boards[seat] = get_board(&rec[at..at + BOARD_BYTES]);
    }
    gs
}

/// `(value, scores, has_scores, n_actions)` - both vectors absolute per seat.
pub fn unpack_targets(rec: &[u8]) -> ([f32; MAX_PLAYERS], [f32; MAX_PLAYERS], bool, u16) {
    let mut value = [0f32; MAX_PLAYERS];
    let mut scores = [0f32; MAX_PLAYERS];
    for (k, (v, s)) in value.iter_mut().zip(scores.iter_mut()).enumerate() {
        *v = get_f32(rec, O_VALUE + k * 4);
        *s = get_f32(rec, O_SCORES + k * 4);
    }
    (
        value,
        scores,
        rec[O_FLAGS] & F_HAS_SCORES != 0,
        get_u16(rec, O_N_ACTIONS),
    )
}

/// `n_actions` alone (the loader's cumulative-sum pass wants only this column).
pub fn record_n_actions(rec: &[u8]) -> u16 {
    get_u16(rec, O_N_ACTIONS)
}

/// The generating game's id (its per-game seed) - unique within and across corpora, so the
/// trainer can hold out whole *games* rather than correlated positions from the same game.
pub fn record_game_id(rec: &[u8]) -> u64 {
    get_u64(rec, O_GAME)
}

// --------------------------------------------------------------------------- file header

/// Corpus file header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub player_count: u32,
    pub record_size: u32,
    pub n_records: u64,
    pub policy_offset: u64,
}

impl Header {
    pub fn to_bytes(self) -> [u8; HEADER_BYTES] {
        let mut b = [0u8; HEADER_BYTES];
        b[0..4].copy_from_slice(MAGIC);
        b[4..8].copy_from_slice(&self.version.to_le_bytes());
        b[8..12].copy_from_slice(&self.player_count.to_le_bytes());
        b[12..16].copy_from_slice(&self.record_size.to_le_bytes());
        put_u64(&mut b, 16, self.n_records);
        put_u64(&mut b, 24, self.policy_offset);
        b
    }

    pub fn from_bytes(b: &[u8]) -> Result<Header, String> {
        if b.len() < HEADER_BYTES {
            return Err(format!("corpus header truncated ({} bytes)", b.len()));
        }
        if &b[0..4] != MAGIC {
            return Err("not a packed Kingdomino corpus (bad magic)".into());
        }
        let version = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        if version == 0 || version > FORMAT_VERSION {
            return Err(format!(
                "corpus format version {version}, this build reads up to {FORMAT_VERSION}"
            ));
        }
        let player_count = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
        let rec_size = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
        let expect = record_size_v(player_count as usize, version) as u32;
        if rec_size != expect {
            return Err(format!(
                "record_size {rec_size} disagrees with player_count {player_count} (expected {expect})"
            ));
        }
        Ok(Header {
            version,
            player_count,
            record_size: rec_size,
            n_records: get_u64(b, 16),
            policy_offset: get_u64(b, 24),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kingdomino_engine::core::{
        apply_action, apply_chance, current_decision, legal_actions, new_game, terminal_value,
        Decision,
    };
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Every player-node state of a real game survives a pack/unpack round trip, and
    /// re-deriving `legal_actions` from the unpacked state reproduces the stored count and the
    /// exact same action list (the invariant the policy targets rely on).
    #[test]
    fn round_trips_every_state_of_a_game() {
        for seed in 0..12u64 {
            let mut gs = new_game(2);
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut buf = Vec::new();
            let mut rec = vec![0u8; record_size(2)];
            let mut states = 0;
            loop {
                match current_decision(&gs) {
                    Decision::Terminal => break,
                    Decision::Chance => {
                        apply_chance(&mut gs, &mut rng);
                    }
                    Decision::Player(_) => {
                        legal_actions(&gs, &mut buf);
                        let value = [0.5, 0.5, 0.0, 0.0];
                        let scores = [61.0, 74.0, 0.0, 0.0];
                        let root = [0.61f32, 0.39, 0.0, 0.0];
                        pack_record(
                            &mut rec,
                            &gs,
                            &value,
                            Some(&scores),
                            buf.len() as u16,
                            0xABCD_1234,
                            Some(&root),
                        );

                        let back = unpack_state(&rec);
                        assert_eq!(back, gs, "state round trip (seed {seed})");
                        let (v, s, has, na) = unpack_targets(&rec);
                        assert_eq!(v, value);
                        assert_eq!(s, scores);
                        assert!(has);
                        assert_eq!(na as usize, buf.len());
                        assert_eq!(record_game_id(&rec), 0xABCD_1234);
                        assert_eq!(unpack_root_value(&rec), Some(root));

                        // The targets-align-with-actions invariant.
                        let mut again = Vec::new();
                        legal_actions(&back, &mut again);
                        assert_eq!(again, buf, "re-derived actions differ (seed {seed})");

                        states += 1;
                        let pick = buf[rng.gen_range(0..buf.len())];
                        apply_action(&mut gs, pick);
                    }
                }
            }
            assert!(
                states > 50,
                "expected a full game's decisions, got {states}"
            );
            assert!(terminal_value(&gs).is_some());
        }
    }

    #[test]
    fn absent_scores_are_flagged() {
        let gs = new_game(2);
        let mut rec = vec![0u8; record_size(2)];
        pack_record(&mut rec, &gs, &[1.0, 0.0, 0.0, 0.0], None, 4, 7, None);
        let (_, scores, has, _) = unpack_targets(&rec);
        assert!(!has, "no scores supplied -> flag clear");
        assert_eq!(scores, [0.0; MAX_PLAYERS]);
        assert_eq!(record_game_id(&rec), 7);
        assert_eq!(
            unpack_root_value(&rec),
            None,
            "no root value supplied -> flag clear"
        );
    }

    /// A v1 record (no root tail) must still decode: v2 only appends, so every earlier field
    /// sits at the same offset and the 250k-game 512-sim archive stays readable.
    #[test]
    fn v1_records_still_decode() {
        let gs = new_game(2);
        let mut v1 = vec![0u8; record_size_v(2, 1)];
        let value = [1.0f32, 0.0, 0.0, 0.0];
        pack_record(&mut v1, &gs, &value, None, 4, 42, None);
        assert_eq!(unpack_state(&v1), gs);
        let (v, _, _, na) = unpack_targets(&v1);
        assert_eq!(v, value);
        assert_eq!(na, 4);
        assert_eq!(record_game_id(&v1), 42);
        assert_eq!(unpack_root_value(&v1), None);
    }

    #[test]
    fn record_size_matches_the_documented_layout() {
        assert_eq!(BOARD_BYTES, 176);
        assert_eq!(record_size_v(2, 1), 434, "v1 layout is frozen");
        assert_eq!(record_size_v(4, 1), 786);
        assert_eq!(record_size(2), 450, "v2 appends 16 B of root value");
        assert_eq!(record_size(4), 802);
    }

    #[test]
    fn header_round_trips_and_rejects_junk() {
        let h = Header {
            version: FORMAT_VERSION,
            player_count: 2,
            record_size: record_size(2) as u32,
            n_records: 803_659,
            policy_offset: 12_345,
        };
        assert_eq!(Header::from_bytes(&h.to_bytes()).unwrap(), h);

        let mut bad = h.to_bytes();
        bad[0] = b'X';
        assert!(Header::from_bytes(&bad).unwrap_err().contains("magic"));

        let mut wrong_version = h.to_bytes();
        wrong_version[4] = 99;
        assert!(Header::from_bytes(&wrong_version)
            .unwrap_err()
            .contains("version"));

        // A v1 header must still parse — the 250k-game archive predates v2.
        let v1 = Header {
            version: 1,
            player_count: 2,
            record_size: record_size_v(2, 1) as u32,
            n_records: 10,
            policy_offset: 99,
        };
        assert_eq!(Header::from_bytes(&v1.to_bytes()).unwrap(), v1);

        let mut mismatched = h.to_bytes();
        mismatched[12] = 1; // record_size no longer matches player_count
        assert!(Header::from_bytes(&mismatched)
            .unwrap_err()
            .contains("disagrees"));
    }
}
