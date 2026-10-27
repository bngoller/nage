# Nage

Nage is open-source personal accounting for one household. It runs as a single Rust binary on a home server, serves a browser UI that works on a phone, and keeps one set of books in one SQLite file. Under the hood it is a double-entry, cash-basis, single-currency ledger. On the surface it says "paid from Checking to Groceries" and never says "debit" or "credit". The workflow v1 must nail: import bank and card CSV exports, categorize them with rules, and see where the money went.

The specification is [SPEC.md](SPEC.md); the ordered task list is [PLAN.md](PLAN.md).

## Status

Milestone 1 (skeleton and pipeline) is in progress on the branch `m1-skeleton`. Nothing runs yet:

- `cargo run -p nage -- serve` arrives with milestone 1 slice 2;
- the sqlx workflow (`.env`, `cargo sqlx migrate run`, `cargo sqlx prepare`) with slice 3;
- `docker compose up` with slice 6.

## Development setup

1. Install [rustup](https://rustup.rs) any way you like. On Arch: `sudo pacman -S --needed rustup cargo-deny`.
2. In the repository, run `rustup toolchain install`. It reads `rust-toolchain.toml` and installs the pinned Rust version with `rustfmt` and `clippy`.
3. Install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if your package manager did not: `cargo install cargo-deny --locked`. `deny.toml` is written for cargo-deny 0.20.

No Node toolchain is needed to build. Node is a test-time dependency only, for the browser tests that arrive with slice 7.

## Commands

| Purpose | Command |
|---|---|
| Test | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck | `cargo check --workspace --all-targets` |
| Format check | `cargo fmt --all --check` |
| Licenses | `cargo deny check licenses` |

Clippy findings are errors (`clippy::all` is denied for the whole workspace), and `nage-core` forbids floating-point arithmetic (SPEC INV-3).

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
