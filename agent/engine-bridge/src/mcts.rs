//! Pure-Rust rollout MCTS + **batched** self-play. The search, game stepping, and random
//! playouts run entirely in Rust, and a pool of independent games runs in parallel across
//! cores (rayon) — so corpus generation isn't bottlenecked by Python or the FFI boundary.
//! Mirrors `kdagent.mcts` (CLAUDE §4): explicit chance nodes (true distribution, **sampled**
//! — the draw has up to 48 outcomes) + max-n **vector** backups with a random-playout leaf
//! value. No network here; this is the rollout tier used for fast baselines / perf testing.
//!
//! `selfplay_batch` returns one JSON line per real decision, matching `kdagent.selfplay`:
//! `{"obs":...,"legal":...,"policy":[...],"to_act":N,"value":[...]}` (value = absolute
//! per-seat final outcome).

use pyo3::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Gamma};
use rayon::prelude::*;

use kingdomino_engine::core::{
    apply_action, apply_chance, chance_outcomes, current_decision, legal_actions, new_game_with,
    terminal_value, Action, Decision, GameState, Variants, MAX_PLAYERS,
};

type Value = [f32; MAX_PLAYERS];

struct Node {
    gs: GameState,
    terminal: bool,
    chance: bool,
    to_act: usize,
    pc: usize,
    expanded: bool,
    value: Value, // terminal value, or a decision leaf's rollout value
    actions: Vec<Action>,
    priors: Vec<f32>,
    visits: Vec<u32>,
    wsum: Vec<Value>,
    children: Vec<i64>, // action -> arena index (-1 unexpanded)
    outcomes: Vec<(Action, f32)>,
    chance_children: Vec<i64>, // outcome -> arena index (-1)
}

fn new_node(gs: GameState) -> Node {
    let pc = gs.player_count as usize;
    let mut node = Node {
        gs,
        terminal: false,
        chance: false,
        to_act: 0,
        pc,
        expanded: false,
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
            node.outcomes = chance_outcomes(&node.gs);
            node.chance_children = vec![-1; node.outcomes.len()];
        }
        Decision::Player(_) => {
            node.to_act = node.gs.to_act as usize;
            let mut buf = Vec::new();
            legal_actions(&node.gs, &mut buf);
            let n = buf.len();
            node.actions = buf;
            node.priors = vec![1.0 / n as f32; n];
            node.visits = vec![0; n];
            node.wsum = vec![[0.0; MAX_PLAYERS]; n];
            node.children = vec![-1; n];
        }
    }
    node
}

/// Build a node after first applying forced single-action player plies, so a no-choice node
/// never materializes in the tree (deeper search per sim). Common in Kingdomino: a forced
/// discard, or a placement/claim with exactly one option.
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

fn push(arena: &mut Vec<Node>, gs: GameState) -> usize {
    arena.push(new_settled_node(gs));
    arena.len() - 1
}

fn argmax(v: &[f32]) -> usize {
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

/// A uniform-random playout to terminal (the rollout leaf value).
fn rollout(mut gs: GameState, rng: &mut impl Rng, buf: &mut Vec<Action>) -> Value {
    loop {
        match current_decision(&gs) {
            Decision::Terminal => return terminal_value(&gs).unwrap_or([0.0; MAX_PLAYERS]),
            Decision::Chance => {
                apply_chance(&mut gs, rng);
            }
            Decision::Player(_) => {
                legal_actions(&gs, buf);
                let a = buf[rng.gen_range(0..buf.len())];
                apply_action(&mut gs, a);
            }
        }
    }
}

fn select(node: &Node, c_puct: f32) -> usize {
    let total: u32 = node.visits.iter().sum();
    if total == 0 {
        return argmax(&node.priors);
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

fn simulate(
    arena: &mut Vec<Node>,
    root: usize,
    rng: &mut impl Rng,
    c_puct: f32,
    buf: &mut Vec<Action>,
) {
    let mut path: Vec<(usize, usize)> = Vec::new();
    let mut cur = root;
    let value: Value = loop {
        if arena[cur].terminal {
            break arena[cur].value;
        }
        if arena[cur].chance {
            let oi = sample_outcome(&arena[cur].outcomes, rng);
            let action = arena[cur].outcomes[oi].0;
            let existing = arena[cur].chance_children[oi];
            cur = if existing < 0 {
                let mut gs = arena[cur].gs;
                apply_action(&mut gs, action);
                let ni = push(arena, gs);
                arena[cur].chance_children[oi] = ni as i64;
                ni
            } else {
                existing as usize
            };
            continue;
        }
        if !arena[cur].expanded {
            let v = rollout(arena[cur].gs, rng, buf);
            arena[cur].value = v;
            arena[cur].expanded = true;
            break v;
        }
        let a = select(&arena[cur], c_puct);
        let existing = arena[cur].children[a];
        let child = if existing < 0 {
            let action = arena[cur].actions[a];
            let mut gs = arena[cur].gs;
            apply_action(&mut gs, action);
            let ni = push(arena, gs);
            arena[cur].children[a] = ni as i64;
            ni
        } else {
            existing as usize
        };
        path.push((cur, a));
        cur = child;
    };
    for (ni, a) in path {
        let node = &mut arena[ni];
        node.visits[a] += 1;
        let pc = node.pc;
        for (w, v) in node.wsum[a][..pc].iter_mut().zip(&value[..pc]) {
            *w += *v;
        }
    }
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

/// Visit-count policy over the legal actions of `gs` after `n_sims` rollout-MCTS sims.
fn run_mcts(
    gs: &GameState,
    n_sims: u32,
    c_puct: f32,
    alpha: f32,
    eps: f32,
    rng: &mut impl Rng,
) -> Vec<f32> {
    let mut arena: Vec<Node> = Vec::with_capacity(n_sims as usize + 4);
    let root = push(&mut arena, *gs);
    arena[root].expanded = true; // descend from the (already-built) root every sim
    if alpha > 0.0 && eps > 0.0 {
        add_dirichlet(&mut arena[root], alpha, eps, rng);
    }
    let mut buf = Vec::new();
    for _ in 0..n_sims {
        simulate(&mut arena, root, rng, c_puct, &mut buf);
    }
    let visits = &arena[root].visits;
    let total: u32 = visits.iter().sum();
    if total == 0 {
        let n = visits.len().max(1);
        return vec![1.0 / n as f32; visits.len().max(1)];
    }
    visits.iter().map(|&v| v as f32 / total as f32).collect()
}

fn select_action(policy: &[f32], temperature: f32, rng: &mut impl Rng) -> usize {
    if temperature <= 1e-3 {
        return argmax(policy);
    }
    let p: Vec<f32> = policy.iter().map(|&x| x.powf(1.0 / temperature)).collect();
    let s: f32 = p.iter().sum();
    if s <= 0.0 {
        return argmax(policy);
    }
    let r: f32 = rng.gen::<f32>() * s;
    let mut acc = 0.0;
    for (i, &x) in p.iter().enumerate() {
        acc += x;
        if r <= acc {
            return i;
        }
    }
    p.len() - 1
}

#[allow(clippy::too_many_arguments)]
fn selfplay_game(
    seed: u64,
    players: u8,
    variants: Variants,
    n_sims: u32,
    c_puct: f32,
    temp_moves: u32,
    alpha: f32,
    eps: f32,
) -> Vec<String> {
    let mut gs = new_game_with(players, variants);
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5BA5_E5ED);
    let mut buf: Vec<Action> = Vec::new();

    struct Rec {
        obs: String,
        legal: String,
        policy: Vec<f32>,
        to_act: usize,
        pc: usize,
    }
    let mut recs: Vec<Rec> = Vec::new();
    let mut mv = 0u32;

    loop {
        match current_decision(&gs) {
            Decision::Terminal => break,
            Decision::Chance => {
                apply_chance(&mut gs, &mut rng);
            }
            Decision::Player(_) => {
                legal_actions(&gs, &mut buf);
                if buf.len() <= 1 {
                    if buf.len() == 1 {
                        apply_action(&mut gs, buf[0]); // forced — no example
                    }
                    continue;
                }
                let policy = run_mcts(&gs, n_sims, c_puct, alpha, eps, &mut rng);
                recs.push(Rec {
                    obs: crate::obs_json(&gs),
                    legal: crate::legal_json(&buf),
                    policy: policy.clone(),
                    to_act: gs.to_act as usize,
                    pc: gs.player_count as usize,
                });
                let temperature = if mv < temp_moves { 1.0 } else { 0.0 };
                let a = select_action(&policy, temperature, &mut rng);
                apply_action(&mut gs, buf[a]);
                mv += 1;
            }
        }
    }

    let outcome = terminal_value(&gs).unwrap_or([0.0; MAX_PLAYERS]);
    recs.into_iter()
        .map(|r| {
            let value: Vec<f32> = outcome[..r.pc].to_vec(); // absolute per-seat outcome
            format!(
                "{{\"obs\":{},\"legal\":{},\"policy\":{},\"to_act\":{},\"value\":{}}}",
                r.obs,
                r.legal,
                serde_json::to_string(&r.policy).unwrap(),
                r.to_act,
                serde_json::to_string(&value).unwrap(),
            )
        })
        .collect()
}

fn game_seed(base: u64, k: usize) -> u64 {
    base.wrapping_add((k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Generate `n_games` self-play games with pure rollout-MCTS, in parallel across cores.
/// Returns all corpus JSON lines (one per real decision). Releases the GIL while running.
#[pyfunction]
#[pyo3(signature = (n_games, players, n_sims, c_puct=1.5, temp_moves=12, dirichlet_alpha=0.3,
                    noise_eps=0.25, seed=0, harmony=true, middle_kingdom=true))]
#[allow(clippy::too_many_arguments)]
pub fn selfplay_batch(
    py: Python<'_>,
    n_games: usize,
    players: u8,
    n_sims: u32,
    c_puct: f32,
    temp_moves: u32,
    dirichlet_alpha: f32,
    noise_eps: f32,
    seed: u64,
    harmony: bool,
    middle_kingdom: bool,
) -> Vec<String> {
    let variants = Variants {
        harmony,
        middle_kingdom,
    };
    py.allow_threads(|| {
        (0..n_games)
            .into_par_iter()
            .flat_map_iter(|k| {
                selfplay_game(
                    game_seed(seed, k),
                    players,
                    variants,
                    n_sims,
                    c_puct,
                    temp_moves,
                    dirichlet_alpha,
                    noise_eps,
                )
                .into_iter()
            })
            .collect()
    })
}
