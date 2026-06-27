//! The MCTS pool for fully-Rust net self-play: a set of concurrent games, each a resumable
//! MCTS state machine. Same logic as the bridge's `batch_selfplay.rs` (max-n backups, sampled
//! chance nodes, forced-ply skip, Dirichlet root noise, slot refill), but the evaluator is
//! internal (libtorch via tch in `main.rs`) — no Python in the hot loop. `apply_eval` takes a
//! leaf's per-action `logits` + seat-relative `value`, exactly what the net produces.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Gamma};

use kingdomino_engine::core::{
    apply_action, apply_chance, chance_outcomes, current_decision, legal_actions, new_game_with,
    terminal_value, Action, Decision, GameState, Variants, MAX_PLAYERS,
};
use kingdomino_features::serialize::{legal_json, obs_json};

type Value = [f32; MAX_PLAYERS];

pub struct Node {
    pub gs: GameState,
    terminal: bool,
    chance: bool,
    expanded: bool,
    to_act: usize,
    pc: usize,
    value: Value,
    pub actions: Vec<Action>,
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
            node.expanded = true;
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

enum Leaf {
    NeedEval,
    Terminal,
}
enum Step {
    NeedEval,
    Over,
    Idle,
}

pub struct GameSearch {
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
    pub out_lines: Vec<String>,

    pub idle: bool,
    base_seed: u64,
    stride: usize,
    total: usize,
    game_index: usize,
    pub completed: usize,
}

fn game_seed(base: u64, k: usize) -> u64 {
    base.wrapping_add((k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

impl GameSearch {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
                        apply_action(&mut self.gs, buf[0]);
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

    /// Expand the pending leaf with per-action `logits` + seat-relative `value`, and back up.
    pub fn apply_eval(&mut self, logits: &[f32], value_rel: &[f32]) {
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

        self.rec_obs.push(obs_json(&self.arena[root].gs));
        self.rec_legal.push(legal_json(&self.arena[root].actions));
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
        let val = serde_json::to_string(&outcome[..pc].to_vec()).unwrap();
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
    pub fn pump(&mut self) {
        loop {
            match self.step() {
                Step::NeedEval => return,
                Step::Over => self.advance_to_next_game(),
                Step::Idle => return,
            }
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending_leaf >= 0
    }
    pub fn leaf(&self) -> &Node {
        &self.arena[self.pending_leaf as usize]
    }
}
