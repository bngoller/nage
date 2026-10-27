# Nage

Nage is open-source personal accounting for one household. It runs as a single Rust binary on a home server, serves a browser UI that works on a phone, and keeps one set of books in one SQLite file. Under the hood it is a double-entry, cash-basis, single-currency ledger. On the surface it says "paid from Checking to Groceries" and never says "debit" or "credit". The workflow v1 must nail: import bank and card CSV exports, categorize them with rules, and see where the money went.

The specification is [SPEC.md](SPEC.md); the ordered task list is [PLAN.md](PLAN.md).

## Status

Milestone 1 (skeleton and pipeline) is in progress on the branch `m1-skeleton`. Slice 2 (binary and server) has landed: `nage serve` answers `/`, `/health` and `/static/*`, and `nage health` probes it. There is no storage yet and the home page is a placeholder. Slice 3 (storage) is next:

- the sqlx workflow (`.env`, `cargo sqlx migrate run`, `cargo sqlx prepare`) arrives with slice 3;
- `docker compose up` with slice 6.

## Development setup

1. Install [rustup](https://rustup.rs) any way you like. On Arch: `sudo pacman -S --needed rustup cargo-deny`.
2. In the repository, run `rustup toolchain install`. It reads `rust-toolchain.toml` and installs the pinned Rust version with `rustfmt` and `clippy`.
3. Install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if your package manager did not: `cargo install cargo-deny --locked`. `deny.toml` is written for cargo-deny 0.20.

No Node toolchain is needed to build. Node is a test-time dependency only, for the browser tests that arrive with slice 7.

## Run locally

Start the server with `cargo run -p nage -- serve` and open <http://127.0.0.1:8787>. In a second terminal, `cargo run -p nage -- health` asks the running server whether it is well: it prints nothing and exits 0 when `/health` answers 200, and prints one line on stderr and exits 1 otherwise. Ctrl-C or SIGTERM stops the server gracefully.

Settings come from flags or environment variables, flag over variable over default; an empty variable counts as unset. A bad value is refused at start with a one-line message naming where it came from, and exits 2.

| Variable | Flag | Default | Meaning |
|---|---|---|---|
| `NAGE_DATA_DIR` | `--data-dir` | `./data` | Data directory; created with a `backups/` folder if missing |
| `NAGE_BIND` | `--bind` | `127.0.0.1:8787` | IP address and port to listen on; `nage health` reads it too |
| `NAGE_TZ` | `--tz` | the `TZ` variable, then UTC | IANA time zone name |
| `NAGE_LOG` | `--log` | `info` | Log filter, a [tracing filter directive](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) |

`NAGE_LOG` takes a bare level or per-crate directives. For example, `NAGE_LOG=warn,nage_web=debug cargo run -p nage -- serve` logs warnings everywhere and debug detail from the web crate, including the passing `/health` requests that the default level leaves out.

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
