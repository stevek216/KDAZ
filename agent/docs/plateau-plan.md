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

## The key unknown

The net matches the targets to within 0.157 nats and no amount of capacity or training closes
that. Two very different explanations remain, and they imply opposite work:

- **A. The prior barely matters.** Search strength may come almost entirely from the *value*
  head, in which case improving the policy can never improve search, and every hour spent on
  policy is wasted.
- **B. The residual is irreducible target noise.** Each visit distribution is one noisy sample
  of the search's policy (Kingdomino's hidden draw means each search samples different
  dominoes), so the net is already at the achievable floor and the fix is *less noisy targets*,
  not better fitting.

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

## Phase 1 — fixes, chosen by what Phase 0 says

Ordered by cost. The first two need **no regeneration** and can run on existing local data.

**F1. Sharpen the policy target (free, offline).** Mean max target probability is only 0.580 — a
soft target teaches a soft prior, which wastes search. Apply a temperature to the stored visit
distribution at train time (`p^(1/T)`, renormalised, T < 1). Costs one argument in
`dataset.collate_packed`, needs no new corpora, and can be swept T ∈ {0.7, 0.85, 1.0} tonight.

**F2. LR schedule (free, offline).** `train.py` uses constant Adam 1e-3 with no decay
(`train.py:163`). Cosine or step decay over `--epochs`. I over-claimed this as *the* diagnosis
earlier and had to walk it back — the epoch-9 collapse is ordinary overfitting, not optimiser
wandering — so treat this as a cheap possible improvement, not a fix for the plateau.

**F3. Blend the MCTS root value into the value target (moderate; needs regeneration).**
Currently the value target is *only* the terminal game outcome — a single win/loss/draw per
game, shared across all ~81 positions of that game, which is a very high-variance label. Modern
AlphaZero variants blend it with the search's backed-up root value, which is far lower variance
and available for free during generation. Requires: a new field in the packed record (bump
`FORMAT_VERSION`, +16 B), emit it from `batch_selfplay::commit_move`, and a mixing weight in
`train.py`. **If D1 shows the value head has stalled, this becomes the top priority** — it
attacks the value head directly, and the value head is what search actually runs on.

**F4. Average over determinizations (larger; search change).** Kingdomino's hidden draw means
each search samples one deck realisation. Averaging visit counts over several determinizations
per move would cut target noise at its source. This is the Kingdomino-specific lever nobody has
pulled, and CLAUDE.md §4 already anticipates it (information-set / PIMC at the root). Only worth
the effort if D3 shows target noise dominates.

## Suggested order

1. **D1** (2 min) — is the value head stalled?
2. **D2** (30 min) — does the prior matter at all?
3. **F1 + F2** (a few hours, offline, existing data) — the two free shots
4. Then **F3** or **F4**, whichever D1/D2/D3 pointed at

Nothing in Phase 0 or F1/F2 needs a pod. Spin one up again only for F3/F4, which require
regenerating corpora.
