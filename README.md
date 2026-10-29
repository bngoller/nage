# Nage

Nage is open-source personal accounting for one household. It runs as a single Rust binary on a home server, serves a browser UI that works on a phone, and keeps one set of books in one SQLite file. Under the hood it is a double-entry, cash-basis, single-currency ledger. On the surface it says "paid from Checking to Groceries" and never says "debit" or "credit". The workflow v1 must nail: import bank and card CSV exports, categorize them with rules, and see where the money went.

The specification is [SPEC.md](SPEC.md); the ordered task list is [PLAN.md](PLAN.md).

## Status

Milestone 1 (skeleton and pipeline) is in progress on the branch `m1-skeleton`. Slices 3 (storage) and 4 (test harness) have landed: `nage serve` creates `data/nage.sqlite`, applies the migrations at start and answers `/`, `/health` (which counts the book rows) and `/static/*`; `nage health` probes it. The database holds one table, `book`, and the home page is a placeholder. Slice 5 (CI) is next, then `docker compose up` with slice 6.

## Development setup

1. Install [rustup](https://rustup.rs) any way you like. On Arch: `sudo pacman -S --needed rustup cargo-deny`.
2. In the repository, run `rustup toolchain install`. It reads `rust-toolchain.toml` and installs the pinned Rust version with `rustfmt` and `clippy`.
3. Install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if your package manager did not: `cargo install cargo-deny --locked`. `deny.toml` is written for cargo-deny 0.20.
4. Install sqlx-cli for the query data check and for changing queries or migrations; see [Database and sqlx](#database-and-sqlx).

No Node toolchain is needed to build. Node is a test-time dependency only, for the browser tests that arrive with slice 7.

## Run locally

Start the server with `cargo run -p nage -- serve` and open <http://127.0.0.1:8787>. In a second terminal, `cargo run -p nage -- health` asks the running server whether it is well: it prints nothing and exits 0 when `/health` answers 200, and prints one line on stderr and exits 1 otherwise. Ctrl-C or SIGTERM stops the server gracefully.

The first start creates `data/nage.sqlite` (with `data/backups/`). Before it listens, `nage serve` applies any pending migrations and logs one `database ready` line with the `path`, the number `applied` and the `schema_version`; a later start applies nothing. While the server runs, SQLite keeps `nage.sqlite-wal` and `nage.sqlite-shm` beside the database; a clean stop removes them. A database written by a newer Nage, or one that cannot be opened, is refused at start with exit 1.

Settings come from flags or environment variables, flag over variable over default; an empty variable counts as unset. A bad value is refused at start with a one-line message naming where it came from, and exits 2.

| Variable | Flag | Default | Meaning |
|---|---|---|---|
| `NAGE_DATA_DIR` | `--data-dir` | `./data` | Data directory holding `nage.sqlite`; created with a `backups/` folder if missing |
| `NAGE_BIND` | `--bind` | `127.0.0.1:8787` | IP address and port to listen on; `nage health` reads it too |
| `NAGE_TZ` | `--tz` | the `TZ` variable, then UTC | IANA time zone name |
| `NAGE_LOG` | `--log` | `info` | Log filter, a [tracing filter directive](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) |
| `NO_COLOR` | none | unset | Any non-empty value turns off ANSI colour in the log output (colour is otherwise on when stderr is a terminal); an empty value counts as unset |

`NAGE_LOG` takes a bare level or per-crate directives. For example, `NAGE_LOG=warn,nage_web=debug cargo run -p nage -- serve` logs warnings everywhere and debug detail from the web crate, including the passing `/health` requests that the default level leaves out.

## Database and sqlx

Queries are checked against the schema at compile time by [sqlx](https://github.com/transact-rs/sqlx), and the result is committed in `.sqlx/`, so an ordinary build needs no database.

Install sqlx-cli, which provides `cargo sqlx`. Its version must match the sqlx line in `Cargo.toml` (0.9). On Arch: `sudo pacman -S --needed sqlx-cli`. Elsewhere: `cargo install sqlx-cli@0.9.0 --no-default-features --features sqlite`.

The committed `.env` holds `DATABASE_URL=sqlite:target/dev.sqlite` and `SQLX_OFFLINE=true`, and never a secret. Because of the second line every build is offline and reads `.sqlx/`; `cargo sqlx prepare` goes live by itself. A `DATABASE_URL` exported in your shell is never overridden by `.env`. Run every `cargo sqlx` command from the repository root, because the relative database path resolves against the working directory.

- First time: `cargo build`, then `cargo sqlx database setup`. It creates `target/dev.sqlite` and applies the migrations.
- After a migration change: `cargo sqlx migrate run`. While `0001_init.sql` is unreleased, an edit to it changes its checksum, so use `cargo sqlx database reset -y` and delete `data/` instead.
- After a query change: `cargo sqlx prepare --workspace -- --all-targets`, then commit the changes in `.sqlx/`.
- Before every commit: `cargo sqlx prepare --check --workspace -- --all-targets`. `cargo clean` deletes `target/dev.sqlite`, so re-run `cargo sqlx database setup` before the check.

The Arch `sqlx-cli` links the system SQLite (3.54), while the Nage binary bundles 3.51.3 and the tests run the migrations through the bundled library. A migration that needs a newer SQLite than 3.51 passes `cargo sqlx migrate run` and fails the tests.

`fixtures/books/v0.1.0.sqlite` is a freshly migrated database with no book row. Tests open a copy of it, never the file itself. When `0001_init.sql` changes before `v0.1.0` is tagged, regenerate it: `rm fixtures/books/v0.1.0.sqlite`, then `cargo xtask seed --out fixtures/books/v0.1.0.sqlite` (`seed` refuses to overwrite a file).

## Commands

| Purpose | Command |
|---|---|
| Test | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck | `cargo check --workspace --all-targets` |
| Format check | `cargo fmt --all --check` |
| Licenses | `cargo deny check licenses` |
| Query data check | `cargo sqlx prepare --check --workspace -- --all-targets` |

Run all six before every commit. Clippy findings are errors (`clippy::all` is denied for the whole workspace), and `nage-core` forbids floating-point arithmetic (SPEC INV-3).

## Tests

Each crate keeps its integration tests in `crates/<crate>/tests/`, with helpers local to that crate in `tests/common/` where it needs them. Helpers shared by all crates (temporary directories, opening a fresh or migrated database, copying the committed fixture, the proptest configuration) live in `nage_core::test_support`, behind the `test-support` cargo feature that only dev-dependencies enable, so a production build never carries them.

- Goldens use [insta](https://insta.rs). Snapshots are files under `snapshots/` beside the test, and the `.snap` files are committed. To write or refresh one, run `INSTA_UPDATE=always cargo test -p <crate> --test <target>` (the test file's stem; `--lib`, or `--bin <name>` in a binary crate, for a snapshot under `src/`) and read the result in `git diff` (a new snapshot is untracked: read the file itself, or run `git add -N <file>` first). [cargo-insta](https://crates.io/crates/cargo-insta) is optional and only needed for `cargo insta review` (Arch: `sudo pacman -S --needed cargo-insta`). Under `CI=true` a missing or stale snapshot fails and nothing is written.
- Properties use [proptest](https://proptest-rs.github.io/proptest/). Each runs 256 cases by default; `PROPTEST_CASES=4096 cargo test -p nage-core` runs more, and `PROPTEST_RNG_SEED` reproduces a run. A failing case is saved under `crates/<crate>/proptest-regressions/` and replayed first on every run; commit those files.
- A test that panics while it still holds its temporary directory keeps it, named `nage-test-*` under the system temp directory, for inspection (a directory dropped before the failing assertion is removed). `rm -rf /tmp/nage-test-*` clears them.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
