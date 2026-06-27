//! Batched **net** self-play — the "games as data" architecture (CLAUDE §4; mirrors
//! SpaceBase's `batch_selfplay.rs`). A pool of independent games, each an MCTS state machine,
//! advanced in lockstep so their leaf evaluations batch into one network forward.
//!
//! Division of labour: this Rust pool owns the games, trees, and **encoding** (directly from
//! `GameState`, via `crate::encoder` — no JSON); Python owns only the loop + the net. Each
//! round: `collect` descends one simulation per game to a leaf (rayon-parallel) and returns
//! the batched encoded inputs + per-action descriptors; Python does one forward; `apply`
//! expands+backs-up every game. With no virtual loss this is *exactly* equivalent to running
//! the games sequentially — pool size changes throughput, not results.

use numpy::ndarray::{Array2, Array3, Array4};
use numpy::{IntoPyArray, PyReadonlyArray2};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Gamma};
use rayon::prelude::*;

use kingdomino_engine::core::{
    apply_action, apply_chance, chance_outcomes, current_decision, legal_actions, new_game_with,
    terminal_value, Action, Decision, GameState, Phase, Variants, MAX_PLAYERS,
};
use kingdomino_engine::rules::cell_of;

use crate::encoder;

type Value = [f32; MAX_PLAYERS];

// =================================================================================
// MCTS node + helpers
// =================================================================================

struct Node {
    gs: GameState,
    terminal: bool,
    chance: bool,
    expanded: bool, // priors+value set (chance/terminal: true; player: only after a net eval)
    to_act: usize,
    pc: usize,
    value: Value,
    actions: Vec<Action>,
    priors: Vec<f32>,
    visits: Vec<u32>,
    wsum: Vec<Value>,
    children: Vec<i64>,
    outcomes: Vec<(Action, f32)>,
    chance_children: Vec<i64>,
}

fn new_node(gs: GameState) -> Node {
    let pc = gs.player_count as usize;
    let mut node = Node {
        gs,
        terminal: false,
        chance: false,
        expanded: false,
        to_act: 0,
        pc,
        value: [0.0; MAX_PLAYERS],
        actions: Vec::new(),
        priors: Vec::new(),
        visits: Vec::new(),
        wsum: Vec::new(),
        children: Vec::new(),
        outcomes: Vec::new(),
        chance_children: Vec::new(),
    };
    match current_decision(&node.gs) {
        Decision::Terminal => {
            node.terminal = true;
            node.expanded = true;
            node.value = terminal_value(&node.gs).unwrap_or([0.0; MAX_PLAYERS]);
        }
        Decision::Chance => {
            node.chance = true;
            node.expanded = true; // chance nodes are descended, never net-evaluated
            node.outcomes = chance_outcomes(&node.gs);
            node.chance_children = vec![-1; node.outcomes.len()];
        }
        Decision::Player(_) => {
            node.to_act = node.gs.to_act as usize;
            let mut buf = Vec::new();
            legal_actions(&node.gs, &mut buf);
            let n = buf.len();
            node.actions = buf;
            node.visits = vec![0; n];
            node.wsum = vec![[0.0; MAX_PLAYERS]; n];
            node.children = vec![-1; n];
        }
    }
    node
}

/// Build a node after skipping forced single-action player plies, so a no-choice node never
/// materializes in the tree (deeper search per sim).
fn new_settled_node(mut gs: GameState) -> Node {
    let mut buf = Vec::new();
    for _ in 0..128 {
        if !matches!(current_decision(&gs), Decision::Player(_)) {
            break;
        }
        legal_actions(&gs, &mut buf);
        if buf.len() != 1 {
            break;
        }
        apply_action(&mut gs, buf[0]);
    }
    new_node(gs)
}

fn argmax_f(v: &[f32]) -> usize {
    let mut best = 0;
    let mut bv = f32::NEG_INFINITY;
    for (i, &x) in v.iter().enumerate() {
        if x > bv {
            bv = x;
            best = i;
        }
    }
    best
}

fn sample_outcome(outcomes: &[(Action, f32)], rng: &mut impl Rng) -> usize {
    let r: f32 = rng.gen();
    let mut acc = 0.0;
    for (i, &(_, p)) in outcomes.iter().enumerate() {
        acc += p;
        if r <= acc {
            return i;
        }
    }
    outcomes.len() - 1
}

fn sample_policy(p: &[f32], rng: &mut impl Rng) -> usize {
    let r: f32 = rng.gen();
    let mut acc = 0.0;
    for (i, &x) in p.iter().enumerate() {
        acc += x;
        if r <= acc {
            return i;
        }
    }
    p.len() - 1
}

fn select(node: &Node, c_puct: f32) -> usize {
    let total: u32 = node.visits.iter().sum();
    if total == 0 {
        return argmax_f(&node.priors);
    }
    let sqrt_total = (total as f32).sqrt();
    let mut best = 0;
    let mut best_score = f32::NEG_INFINITY;
    for a in 0..node.actions.len() {
        let na = node.visits[a];
        let q = if na > 0 {
            node.wsum[a][node.to_act] / na as f32
        } else {
            0.0
        };
        let u = c_puct * node.priors[a] * sqrt_total / (1.0 + na as f32);
        let score = q + u;
        if score > best_score {
            best_score = score;
            best = a;
        }
    }
    best
}

fn add_dirichlet(node: &mut Node, alpha: f32, eps: f32, rng: &mut impl Rng) {
    let n = node.priors.len();
    if n < 2 {
        return;
    }
    let gamma = Gamma::new(alpha as f64, 1.0).unwrap();
    let mut noise: Vec<f32> = (0..n).map(|_| gamma.sample(rng) as f32).collect();
    let sum: f32 = noise.iter().sum::<f32>().max(1e-8);
    for x in noise.iter_mut() {
        *x /= sum;
    }
    for (p, &nz) in node.priors.iter_mut().zip(noise.iter()) {
        *p = (1.0 - eps) * *p + eps * nz;
    }
}

/// Policy-head action descriptor (matches `kdagent.encoder` / `dataset`): `(type, place_idx,
/// claim_line_tok)`. type 0=place, 1=claim, 2=discard.
fn action_descriptor(a: Action, phase: Phase) -> (i64, i64, i64) {
    match a {
        Action::Place { anchor, rot } => {
            let (r, c) = cell_of(anchor);
            (0, rot as i64 * 169 + r as i64 * 13 + c as i64, 0)
        }
        Action::Claim { slot } => {
            let lt = if matches!(phase, Phase::StartClaim) {
                slot as i64
            } else {
                4 + slot as i64
            };
            (1, 0, lt)
        }
        _ => (2, 0, 0), // discard (or unreachable at a player node)
    }
}

// =================================================================================
// One game's search (resumable state machine)
// =================================================================================

enum Leaf {
    NeedEval,
    Terminal,
}
enum Step {
    NeedEval,
    Over,
    Idle,
}

struct GameSearch {
    players: usize,
    variants: Variants,
    n_sims: u32,
    c_puct: f32,
    temp_moves: u32,
    alpha: f32,
    eps: f32,

    gs: GameState,
    rng: ChaCha8Rng,
    arena: Vec<Node>,
    root: usize,
    sims_done: u32,
    move_no: u32,
    has_tree: bool,
    root_noised: bool,

    pending_path: Vec<(usize, usize)>,
    pending_leaf: i64,

    rec_obs: Vec<String>,
    rec_legal: Vec<String>,
    rec_policy: Vec<Vec<f32>>,
    rec_toact: Vec<usize>,
    out_lines: Vec<String>,

    idle: bool,
    base_seed: u64,
    stride: usize,
    total: usize,
    game_index: usize,
    completed: usize,
}

fn game_seed(base: u64, k: usize) -> u64 {
    base.wrapping_add((k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

impl GameSearch {
    #[allow(clippy::too_many_arguments)]
    fn new(
        base_seed: u64,
        slot: usize,
        stride: usize,
        total: usize,
        players: u8,
        variants: Variants,
        n_sims: u32,
        c_puct: f32,
        temp_moves: u32,
        alpha: f32,
        eps: f32,
    ) -> Self {
        let first = game_seed(base_seed, slot);
        GameSearch {
            players: players as usize,
            variants,
            n_sims,
            c_puct,
            temp_moves,
            alpha,
            eps,
            gs: new_game_with(players, variants),
            rng: ChaCha8Rng::seed_from_u64(first ^ 0x5BA5_E5ED),
            arena: Vec::new(),
            root: 0,
            sims_done: 0,
            move_no: 0,
            has_tree: false,
            root_noised: false,
            pending_path: Vec::new(),
            pending_leaf: -1,
            rec_obs: Vec::new(),
            rec_legal: Vec::new(),
            rec_policy: Vec::new(),
            rec_toact: Vec::new(),
            out_lines: Vec::new(),
            idle: false,
            base_seed,
            stride,
            total,
            game_index: slot,
            completed: 0,
        }
    }

    fn reset(&mut self, seed: u64) {
        self.gs = new_game_with(self.players as u8, self.variants);
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5BA5_E5ED);
        self.arena.clear();
        self.root = 0;
        self.sims_done = 0;
        self.move_no = 0;
        self.has_tree = false;
        self.root_noised = false;
        self.pending_leaf = -1;
        self.pending_path.clear();
    }

    fn push_node(&mut self, gs: GameState) -> usize {
        self.arena.push(new_settled_node(gs));
        self.arena.len() - 1
    }

    /// Advance the live game past chance + forced plies to a real decision, then build the root.
    fn start_search(&mut self) -> bool {
        let mut buf = Vec::new();
        loop {
            match current_decision(&self.gs) {
                Decision::Terminal => return false,
                Decision::Chance => {
                    apply_chance(&mut self.gs, &mut self.rng);
                }
                Decision::Player(_) => {
                    legal_actions(&self.gs, &mut buf);
                    if buf.len() == 1 {
                        apply_action(&mut self.gs, buf[0]); // forced — no search, no record
                    } else {
                        self.arena.clear();
                        self.arena.push(new_node(self.gs));
                        self.root = 0;
                        self.sims_done = 0;
                        self.has_tree = true;
                        self.root_noised = false;
                        return true;
                    }
                }
            }
        }
    }

    fn collect_leaf(&mut self) -> Leaf {
        let mut path: Vec<(usize, usize)> = Vec::new();
        let mut cur = self.root;
        loop {
            if self.arena[cur].terminal {
                let v = self.arena[cur].value;
                let pc = self.players;
                for (n, a) in &path {
                    let node = &mut self.arena[*n];
                    node.visits[*a] += 1;
                    for (w, x) in node.wsum[*a][..pc].iter_mut().zip(&v[..pc]) {
                        *w += *x;
                    }
                }
                return Leaf::Terminal;
            }
            if self.arena[cur].chance {
                let oi = sample_outcome(&self.arena[cur].outcomes, &mut self.rng);
                let action = self.arena[cur].outcomes[oi].0;
                let existing = self.arena[cur].chance_children[oi];
                cur = if existing < 0 {
                    let mut gs = self.arena[cur].gs;
                    apply_action(&mut gs, action);
                    let ni = self.push_node(gs);
                    self.arena[cur].chance_children[oi] = ni as i64;
                    ni
                } else {
                    existing as usize
                };
                continue;
            }
            if !self.arena[cur].expanded {
                self.pending_path = path;
                self.pending_leaf = cur as i64;
                return Leaf::NeedEval;
            }
            let a = select(&self.arena[cur], self.c_puct);
            let existing = self.arena[cur].children[a];
            let child = if existing < 0 {
                let action = self.arena[cur].actions[a];
                let mut gs = self.arena[cur].gs;
                apply_action(&mut gs, action);
                let ni = self.push_node(gs);
                self.arena[cur].children[a] = ni as i64;
                ni
            } else {
                existing as usize
            };
            path.push((cur, a));
            cur = child;
        }
    }

    fn apply_eval(&mut self, logits: &[f32], value_rel: &[f32]) {
        let leaf = self.pending_leaf as usize;
        let n = self.arena[leaf].actions.len();
        let mx = logits[..n]
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let mut exps: Vec<f32> = logits[..n].iter().map(|&x| (x - mx).exp()).collect();
        let s: f32 = exps.iter().sum::<f32>().max(1e-12);
        for e in exps.iter_mut() {
            *e /= s;
        }
        self.arena[leaf].priors = exps;
        let to_act = self.arena[leaf].to_act;
        let pc = self.arena[leaf].pc;
        let mut absval = [0f32; MAX_PLAYERS];
        for k in 0..pc {
            absval[(to_act + k) % pc] = value_rel[k];
        }
        self.arena[leaf].value = absval;
        self.arena[leaf].expanded = true;
        if leaf == self.root && !self.root_noised {
            add_dirichlet(&mut self.arena[leaf], self.alpha, self.eps, &mut self.rng);
            self.root_noised = true;
        }
        let path = std::mem::take(&mut self.pending_path);
        for (n, a) in &path {
            let node = &mut self.arena[*n];
            node.visits[*a] += 1;
            for (w, x) in node.wsum[*a][..pc].iter_mut().zip(&absval[..pc]) {
                *w += *x;
            }
        }
        self.sims_done += 1;
        self.pending_leaf = -1;
    }

    fn commit_move(&mut self) {
        let root = self.root;
        let n = self.arena[root].actions.len();
        let total: u32 = self.arena[root].visits.iter().sum();
        let policy: Vec<f32> = (0..n)
            .map(|a| {
                if total > 0 {
                    self.arena[root].visits[a] as f32 / total as f32
                } else {
                    1.0 / n as f32
                }
            })
            .collect();

        self.rec_obs.push(crate::obs_json(&self.arena[root].gs));
        self.rec_legal
            .push(crate::legal_json(&self.arena[root].actions));
        self.rec_toact.push(self.arena[root].to_act);
        self.rec_policy.push(policy.clone());

        let a = if self.move_no < self.temp_moves {
            sample_policy(&policy, &mut self.rng)
        } else {
            argmax_f(&policy)
        };
        let action = self.arena[root].actions[a];
        apply_action(&mut self.gs, action);
        self.move_no += 1;
        self.has_tree = false;
    }

    fn finalize(&mut self) {
        let outcome = terminal_value(&self.gs).unwrap_or([0.0; MAX_PLAYERS]);
        let pc = self.players;
        let value: Vec<f32> = outcome[..pc].to_vec();
        let val = serde_json::to_string(&value).unwrap();
        for i in 0..self.rec_obs.len() {
            let pol = serde_json::to_string(&self.rec_policy[i]).unwrap();
            self.out_lines.push(format!(
                "{{\"obs\":{},\"legal\":{},\"policy\":{},\"to_act\":{},\"value\":{}}}",
                self.rec_obs[i], self.rec_legal[i], pol, self.rec_toact[i], val
            ));
        }
        self.rec_obs.clear();
        self.rec_legal.clear();
        self.rec_policy.clear();
        self.rec_toact.clear();
        self.completed += 1;
    }

    fn advance_to_next_game(&mut self) {
        self.game_index += self.stride;
        if self.game_index < self.total {
            self.reset(game_seed(self.base_seed, self.game_index));
        } else {
            self.idle = true;
        }
    }

    fn step(&mut self) -> Step {
        loop {
            if self.idle {
                return Step::Idle;
            }
            if !self.has_tree && !self.start_search() {
                self.finalize();
                return Step::Over;
            }
            if self.sims_done >= self.n_sims {
                self.commit_move();
                continue;
            }
            match self.collect_leaf() {
                Leaf::NeedEval => return Step::NeedEval,
                Leaf::Terminal => self.sims_done += 1,
            }
        }
    }

    /// Pump to the next leaf needing eval, finishing games and refilling the slot internally.
    fn pump(&mut self) {
        loop {
            match self.step() {
                Step::NeedEval => return,
                Step::Over => self.advance_to_next_game(),
                Step::Idle => return,
            }
        }
    }

    fn has_pending(&self) -> bool {
        self.pending_leaf >= 0
    }
    fn pending_na(&self) -> usize {
        self.arena[self.pending_leaf as usize].actions.len()
    }
}

// =================================================================================
// The pool (PyO3)
// =================================================================================

#[pyclass]
pub struct BatchedNetSelfPlay {
    games: Vec<GameSearch>,
    pending: Vec<usize>,
    finished: Vec<String>,
    total_games: usize,
    players: usize,
}

#[pymethods]
impl BatchedNetSelfPlay {
    #[new]
    #[pyo3(signature = (n_games, total_games, players, n_sims, c_puct = 1.5, temp_moves = 12,
                        dirichlet_alpha = 0.3, noise_eps = 0.25, seed = 0,
                        harmony = true, middle_kingdom = true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        n_games: usize,
        total_games: usize,
        players: u8,
        n_sims: u32,
        c_puct: f32,
        temp_moves: u32,
        dirichlet_alpha: f32,
        noise_eps: f32,
        seed: u64,
        harmony: bool,
        middle_kingdom: bool,
    ) -> Self {
        let variants = Variants {
            harmony,
            middle_kingdom,
        };
        let slots = n_games.min(total_games).max(1);
        let games = (0..slots)
            .map(|s| {
                GameSearch::new(
                    seed,
                    s,
                    slots,
                    total_games,
                    players,
                    variants,
                    n_sims,
                    c_puct,
                    temp_moves,
                    dirichlet_alpha,
                    noise_eps,
                )
            })
            .collect();
        BatchedNetSelfPlay {
            games,
            pending: Vec::new(),
            finished: Vec::new(),
            total_games,
            players: players as usize,
        }
    }

    /// Advance every game one simulation; return the batched net inputs + action descriptors
    /// for all leaves needing eval (a dict of numpy arrays, `b` = number of leaves; 0 = done).
    fn collect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let games = &mut self.games;
        py.allow_threads(|| games.par_iter_mut().for_each(|g| g.pump()));

        self.pending.clear();
        for (slot, g) in self.games.iter_mut().enumerate() {
            if !g.out_lines.is_empty() {
                self.finished.append(&mut g.out_lines);
            }
            if g.has_pending() {
                self.pending.push(slot);
            }
        }

        let b = self.pending.len();
        let pc = self.players;
        let amax = self
            .pending
            .iter()
            .map(|&s| self.games[s].pending_na())
            .max()
            .unwrap_or(1)
            .max(1);

        // Snapshot leaf states + build per-action descriptors (cheap, sequential).
        let mut states: Vec<GameState> = Vec::with_capacity(b);
        let mut a_type = vec![-1i64; b * amax];
        let mut a_pidx = vec![0i64; b * amax];
        let mut a_ltok = vec![0i64; b * amax];
        let mut a_mask = vec![0u8; b * amax];
        for (i, &slot) in self.pending.iter().enumerate() {
            let node = &self.games[slot].arena[self.games[slot].pending_leaf as usize];
            states.push(node.gs);
            for (j, &act) in node.actions.iter().enumerate() {
                let (t, p, l) = action_descriptor(act, node.gs.phase);
                a_type[i * amax + j] = t;
                a_pidx[i * amax + j] = p;
                a_ltok[i * amax + j] = l;
                a_mask[i * amax + j] = 1;
            }
        }

        // Encode the leaf states into the batch buffers (rayon-parallel, GIL released).
        let per_board = encoder::board_per_state(pc);
        let glen = encoder::glob_len(pc);
        let mut board = vec![0f32; b * per_board];
        let mut lines = vec![0f32; b * encoder::LINES_LEN];
        let mut glob = vec![0f32; b * glen];
        py.allow_threads(|| {
            board
                .par_chunks_mut(per_board.max(1))
                .zip(lines.par_chunks_mut(encoder::LINES_LEN))
                .zip(glob.par_chunks_mut(glen.max(1)))
                .zip(states.par_iter())
                .for_each(|(((bc, lc), gc), gs)| encoder::encode_into(gs, bc, lc, gc));
        });

        let d = PyDict::new_bound(py);
        d.set_item("b", b)?;
        d.set_item(
            "board",
            Array4::from_shape_vec(
                (b, pc * encoder::N_PLANES, encoder::STORE, encoder::STORE),
                board,
            )
            .unwrap()
            .into_pyarray_bound(py),
        )?;
        d.set_item(
            "lines",
            Array3::from_shape_vec((b, 8, encoder::LINE_FEATS), lines)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        d.set_item(
            "glob",
            Array2::from_shape_vec((b, glen), glob)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        d.set_item(
            "a_type",
            Array2::from_shape_vec((b, amax), a_type)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        d.set_item(
            "a_pidx",
            Array2::from_shape_vec((b, amax), a_pidx)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        d.set_item(
            "a_ltok",
            Array2::from_shape_vec((b, amax), a_ltok)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        d.set_item(
            "a_mask",
            Array2::from_shape_vec((b, amax), a_mask)
                .unwrap()
                .into_pyarray_bound(py),
        )?;
        Ok(d)
    }

    /// Apply the net outputs for this round's batch: `logits [B, Amax]` (per-action policy
    /// logits, illegal = anything; masked by Amax) and `value [B, pc]` (seat-relative).
    fn apply(&mut self, logits: PyReadonlyArray2<f32>, value: PyReadonlyArray2<f32>) {
        let la = logits.as_array();
        let va = value.as_array();
        let pc = self.players;
        for (k, &slot) in self.pending.iter().enumerate() {
            let na = self.games[slot].pending_na();
            let lrow: Vec<f32> = (0..na).map(|j| la[[k, j]]).collect();
            let vrow: Vec<f32> = (0..pc).map(|j| va[[k, j]]).collect();
            self.games[slot].apply_eval(&lrow, &vrow);
        }
        self.pending.clear();
    }

    /// Drain finished games' corpus lines (one JSON line per recorded decision).
    fn drain(&mut self) -> Vec<String> {
        std::mem::take(&mut self.finished)
    }

    fn done(&self) -> bool {
        self.games.iter().all(|g| g.idle)
    }

    /// `(completed, total)` for progress reporting.
    fn stats(&self) -> (usize, usize) {
        (
            self.games.iter().map(|g| g.completed).sum(),
            self.total_games,
        )
    }
}
