# Getting the net to pick up what search is putting down

Status: **plan**, written 2026-07-29 after the gen11 plateau. Measured numbers live in
`Docs/Training History.txt`; this doc is the forward plan only.

## The problem in one line

512-sim search beats gen11's own raw policy **97.2% ± 0.7**, and training on that search's
targets produces a policy only **~2 points** better than gen11's (52.5% ± 1.5, LCB 51.0) — an
improvement search then erases entirely (netmcts-vs-netmcts ≈ 50%).

So the teacher is overwhelmingly strong, the student learns almost nothing useful from it, and
the flywheel's engine — *better net → better search → better targets* — has stalled at the first
arrow.

## Doors already closed (do not reopen without a new reason)

| lever | result |
|---|---|
| capacity ch=128 → 256 | 4x params, **zero** gain (raw curves identical, ±1.5) |
| epochs 5 → 12 | peaks at 5, collapses after (39.5% by epoch 9) |
| 512-sim generation | 2x cost, **nothing** (48.9% on a fully-512-sim window) |
| target-entropy floor | **refuted** — H(target)=1.157 vs achieved 1.313, 0.157 nats remain |

Two that *did* work and are already in use: a 4-generation window, and ch=128 (which only pays
off *with* the wide window — they interact).

## Phase 0 results (2026-07-29) — hypothesis A confirmed

**D2 — the policy prior is worth 4.7 points.** Running the arena with agent A's policy logits
zeroed (uniform prior, same value head) scores **45.3% ± 2.2** against the real prior at 512
sims. So discarding gen11's entire policy costs under 5 points, while training recovers ~2
points of policy quality per generation — a fraction of a fraction. **Search strength is
overwhelmingly the value head, and the policy is nearly a passenger.** Every lever tested on
2026-07-29 (capacity, epochs, sim count) aimed at the policy head; that was the wrong head.

**D1 — the value head HAS been learning.** On 410,642 positions from gen11's own play:

| net | params | winner-acc | corr |
|-----|-------:|-----------:|-----:|
| gen0  | 133k | 0.568 | 0.163 |
| gen11 | 496k | **0.682** | **0.463** |

A prediction that it had stalled was wrong. *Caveat on an earlier number:* gen0 was once
measured at "90% winner accuracy" — but that was on gen0's **own** corpus, where weak play makes
games lopsided and easy to call. On gen11-level positions the same net scores 0.568. Value
accuracy is a function of how close the games are; those figures were never comparable and the
90% should not be used as a baseline.

So neither head is broken — both have extracted what the current targets can teach. For the
value head the target is one win/loss label shared across all ~81 positions of a game, and
Kingdomino's hidden draw means the winner genuinely depends on unseen cards. 68.2% on close
games between strong players may be near the ceiling *for that target*. Hence: change the
target, don't grow the net.

**Still open (D3).** Whether the 0.157-nat policy residual is irreducible target noise. Now
low priority — with the prior worth only 4.7 points, the answer barely changes what we do.

## Phase 0 — three diagnostics, all local, no pod

We have 250k games of gen11-generated 512-sim data locally (`agent/data/pod/`), so all of this
runs on the 4070S.

**D1. Value-head quality (trivial, ~2 min).** Measure gen11's value head on held-out corpus
positions: winner accuracy and correlation with the actual outcome. gen0 scored 90% / 0.87 back
when we checked it. If gen11 is no better, the search has been guided by a value function that
stopped improving many generations ago — which would explain every result above.

**D2. Prior sensitivity (easy, ~30 min).** Run the arena with the policy head's logits
**replaced by zeros**, which makes the prior uniform while leaving the value head untouched. No
Rust change needed — the Python driver already hands logits to `pool.apply()`, so it is a
two-line edit in a copy of the arena loop. Then:

    netmcts:512 (real prior)  vs  netmcts:512 (uniform prior)      [same value head]

If uniform is nearly as strong, hypothesis **A** is confirmed and policy work is pointless.
This is the highest-information test per unit of effort in the plan.

**D3. Target noise (medium effort).** Run search twice on the *same* position with different RNG
seeds and measure the KL between the two visit distributions. If that KL is around 0.157 nats,
the net is already at the floor and hypothesis **B** is confirmed. Needs a way to seed search
from a packed record's state — `pack::unpack_state` plus a small pyfunction, or reuse
`core/rebuild.rs`. Only worth building if D1/D2 do not already settle the question.

## Phase 1 — everything aims at the evaluator now

D2 reordered this list. Anything that improves the *policy* has a ceiling of roughly 4.7 points
and is therefore deprioritised; anything that improves the *evaluator* is where the leverage is.

**F3a. Blend the score head into the search's leaf value (free — no retrain, no regeneration).**
The value head predicts win/loss. The **score head** predicts final score margin: dense,
already trained, loss 0.033 and flat — and search ignores it entirely. Calibrating
margin → win-prob on held-out positions gives `a = 6.139`, at which the score head alone scores
**0.675 winner accuracy vs the value head's 0.682**. Two comparably-accurate evaluators derived
differently, which is the classic setup for an ensemble to beat both. The blend happens purely
in the Python driver (it already computes the value it hands to `pool.apply()`), so `alpha` is
the only knob and `alpha=0` is the control. **Cheapest shot at the head that matters.**

**F3b. Blend the MCTS root value into the value *target* (moderate; needs regeneration).**
The value target is *only* the terminal outcome — one win/loss per game, shared across ~81
positions, in a game whose result depends on unseen cards. The search's backed-up root value is
a far lower-variance estimate of position quality and is computed during generation, then thrown
away. Requires a new field in the packed record (bump `FORMAT_VERSION` to 2, +16 B, and keep the
v1 reader so the 250k-game 512-sim archive stays readable), emitting it from
`batch_selfplay::commit_move`, and a mixing weight in `train.py`.

**F4. Average over determinizations (larger; search change).** Kingdomino's hidden draw means
each search samples one deck realisation, which adds noise to *both* targets. Averaging over
several determinizations per move cuts it at the source. CLAUDE.md §4 already anticipates this
(information-set / PIMC at the root).

**F2. LR schedule (free, offline).** Constant Adam 1e-3, no decay (`train.py:163`). I
over-claimed this as *the* diagnosis earlier and walked it back — the epoch-9 collapse is
ordinary overfitting, not optimiser wandering. Cheap, speculative, not a plateau fix.

**F1. Sharpen the policy target — DEMOTED.** Mean max target probability is only 0.580, so a
soft target teaches a soft prior. But D2 caps all policy-side work at ~4.7 points, so this is no
longer worth doing first despite being free.

## Suggested order

1. ~~D1, D2~~ — **done**, see above
2. **F3a** — free, in flight
3. **F3b** — the real fix if F3a is not enough; needs a pod for regeneration
4. **F4** if target noise still dominates; **F2/F1** only as cheap extras

Only F3b and F4 need a pod.
