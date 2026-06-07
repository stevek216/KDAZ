# Contributing

Thanks for contributing to the Kingdomino game engine!

## Development Setup

Install Rust via [rustup](https://rustup.rs/), then:

```bash
cargo build
cargo test
```

## Workflow

1. Create a branch off `main`.
2. Keep the engine **deterministic** — all randomness flows through an injected,
   seedable RNG (`rand_chacha`) so games are reproducible from a seed.
3. Add or update tests for the code you change (see Conventions below).
4. Run the checks below before opening a PR.

## Checks

```bash
cargo test                       # unit + integration tests
cargo fmt --all -- --check       # formatting
cargo clippy --all-targets -- -D warnings   # lints
```

## Conventions

- **Unit tests beside the code.** Put `#[cfg(test)] mod tests { ... }` in the
  same file as the code it covers. Reserve `tests/` for cross-module,
  end-to-end behavior (e.g. playing a full game from a fixed seed).
- **No hidden state.** Game logic takes a state in and returns the next state;
  avoid global mutable state so the engine is safe for parallel self-play.
- **Allocation-aware.** Hot paths (legal-move generation, action application)
  run millions of times during self-play — prefer stack data and reuse buffers.
- **Document rules decisions.** When you encode a Kingdomino rule, note the
  rulebook reference (`../Docs/rules.pdf`) in `docs/` so the engine stays auditable.
- **Never fabricate game data.** The 48-domino table must be sourced and verified,
  not guessed (see `docs/engine-design.md` §2).

## Commit Messages

Use clear, imperative subject lines (e.g. "Add domino-placement legal-action gen").
