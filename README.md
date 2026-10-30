# Nage

Nage is open-source personal accounting for one household. It runs as a single Rust binary on a home server, serves a browser UI that works on a phone, and keeps one set of books in one SQLite file. Under the hood it is a double-entry, cash-basis, single-currency ledger. On the surface it says "paid from Checking to Groceries" and never says "debit" or "credit". The workflow v1 must nail: import bank and card CSV exports, categorize them with rules, and see where the money went.

The specification is [SPEC.md](SPEC.md); the ordered task list is [PLAN.md](PLAN.md).

## Status

[![CI](https://github.com/bngoller/nage/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/bngoller/nage/actions/workflows/ci.yml)

Milestone 1 (skeleton and pipeline) is in progress on the branch `m1-skeleton`. Slices 3 (storage), 4 (test harness) and 5 (CI) have landed: `nage serve` creates `data/nage.sqlite`, applies the migrations at start and answers `/`, `/health` (which counts the book rows) and `/static/*`; `nage health` probes it. The database holds one table, `book`, and the home page is a placeholder. CI runs the six checks on every pull request and every push to `main`, and, once `advisories.yml` is on `main`, the advisory check weekly (see [Continuous integration](#continuous-integration)). Slice 6 (Docker) is next, and with it `docker compose up`.

## Development setup

1. Install [rustup](https://rustup.rs) any way you like. On Arch: `sudo pacman -S --needed rustup cargo-deny`.
2. In the repository, run `rustup toolchain install`. It reads `rust-toolchain.toml` and installs the pinned Rust version with `rustfmt` and `clippy`.
3. Install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if your package manager did not: `cargo install cargo-deny --locked`. `deny.toml` is written for cargo-deny 0.20.
4. Install sqlx-cli for the query data check and for changing queries or migrations; see [Database and sqlx](#database-and-sqlx).
5. Install `actionlint` and `shellcheck` (Arch: `sudo pacman -S --needed actionlint shellcheck`) if you edit a workflow; actionlint lints the workflow files and uses shellcheck on their `run:` scripts.

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

Install sqlx-cli, which provides `cargo sqlx`. Its version must match the sqlx line in `Cargo.toml` (0.9). On Arch: `sudo pacman -S --needed sqlx-cli`. Elsewhere: `cargo install sqlx-cli@0.9.0 --no-default-features --features sqlite --locked`.

The committed `.env` holds `DATABASE_URL=sqlite:target/dev.sqlite` and `SQLX_OFFLINE=true`, and never a secret. Because of the second line every build is offline and reads `.sqlx/`; `cargo sqlx prepare` goes live by itself. A `DATABASE_URL` exported in your shell is never overridden by `.env`. Run every `cargo sqlx` command from the repository root, because the relative database path resolves against the working directory.

- First time: `cargo build`, then `cargo sqlx database setup`. It creates `target/dev.sqlite` and applies the migrations.
- After a migration change: `cargo sqlx migrate run`. While `0001_init.sql` is unreleased, an edit to it changes its checksum, so use `cargo sqlx database reset -y` and delete `data/` instead.
- After a query change: `cargo sqlx prepare --workspace -- --all-targets`, then commit the changes in `.sqlx/`.
- Before every commit: `cargo sqlx prepare --check --workspace -- --all-targets`. `cargo clean` deletes `target/dev.sqlite`, so re-run `cargo sqlx database setup` before the check.

The Arch `sqlx-cli` links the system SQLite (3.54), while the Nage binary bundles 3.51.3 and the tests run the migrations through the bundled library. A migration that needs a newer SQLite than 3.51 passes `cargo sqlx migrate run` and fails the tests.

CI sets no `SQLX_OFFLINE`: the committed `.env` makes every job offline, and the prepare-check job goes live by itself after installing sqlx-cli with that command (bundled SQLite 3.51.3, the same as the binary) and migrating a fresh `target/dev.sqlite`.

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

Run all six before every commit. CI runs the same six, one job each, with `--locked` added where cargo accepts it (a lock file that does not match the manifests fails there). Clippy findings are errors (`clippy::all` is denied for the whole workspace), and `nage-core` forbids floating-point arithmetic (SPEC INV-3).

## Tests

Each crate keeps its integration tests in `crates/<crate>/tests/`, with helpers local to that crate in `tests/common/` where it needs them. Helpers shared by all crates (temporary directories, opening a fresh or migrated database, copying the committed fixture, the proptest configuration) live in `nage_core::test_support`, behind the `test-support` cargo feature that only dev-dependencies enable, so a production build never carries them.

- Goldens use [insta](https://insta.rs). Snapshots are files under `snapshots/` beside the test, and the `.snap` files are committed. To write or refresh one, run `INSTA_UPDATE=always cargo test -p <crate> --test <target>` (the test file's stem; `--lib`, or `--bin <name>` in a binary crate, for a snapshot under `src/`) and read the result in `git diff` (a new snapshot is untracked: read the file itself, or run `git add -N <file>` first). [cargo-insta](https://crates.io/crates/cargo-insta) is optional and only needed for `cargo insta review` (Arch: `sudo pacman -S --needed cargo-insta`). Under `CI=true` a missing or stale snapshot fails and nothing is written.
- Properties use [proptest](https://proptest-rs.github.io/proptest/). Each runs 256 cases by default; `PROPTEST_CASES=4096 cargo test -p nage-core` runs more, and `PROPTEST_RNG_SEED` reproduces a run. A failing case is saved under `crates/<crate>/proptest-regressions/` and replayed first on every run; commit those files. CI runs 1024 cases; a failure there prints a `cc <seed>` line to add to the crate's regression file.
- A test that panics while it still holds its temporary directory keeps it, named `nage-test-*` under the system temp directory, for inspection (a directory dropped before the failing assertion is removed). `rm -rf /tmp/nage-test-*` clears them.

## Continuous integration

`.github/workflows/ci.yml` runs on every pull request, on every push to `main` and, once the file is on `main`, by hand from the Actions tab. It has one job per check in the Commands table above: `fmt`, `clippy`, `check (all targets)`, `sqlx prepare check`, `deny licenses` and `test`. Each job runs the table's command with `--locked` added where cargo accepts it. The jobs run on an explicitly named runner image (`ubuntu-24.04`), install the Rust version that `rust-toolchain.toml` pins, and cache builds per job. Every action is pinned to a full commit SHA with a `# vX.Y.Z` comment naming the release, and the pins are re-resolved at each milestone PR. The docker, e2e and fixture-guard jobs join the same file with slices 6, 7 and 8.

To read a failed job, open the run from the pull request's Checks tab and expand the red step. To reproduce the test job locally, run `CI=true INSTA_UPDATE=no PROPTEST_CASES=1024 cargo test --workspace --locked`; for the other jobs, run the table's command with `--locked` added where cargo accepts it, as `ci.yml` does (without it, a lock file that does not match the manifests passes locally and is rewritten). The sqlx prepare check job first migrates a fresh dev database, so reproduce it from one: `mkdir -p target`, `rm -f target/dev.sqlite target/dev.sqlite-wal target/dev.sqlite-shm` and `cargo sqlx database setup`, then the check; against an older dev database the check can pass where the job fails. The job's sqlx-cli is the `cargo install` build from Database and sqlx (bundled SQLite 3.51.3), not the Arch package.

`.github/workflows/advisories.yml` runs `cargo deny --locked check advisories` every Monday at 06:23 UTC and by hand, and on pull requests that change `advisories.yml`, `deny.toml` or `Cargo.lock`; the schedule and the manual button work once the file is on `main`. It is never a required check. When a scheduled or manual run fails it opens one issue titled "cargo deny check advisories failed", or comments on that issue while it is open. The `ignore` list in the `[advisories]` table of `deny.toml` holds every advisory the project has accepted, each with its id, a reason and when to revisit it; adding an entry is the maintainer's decision. GitHub pauses scheduled workflows in a public repository after 60 days without activity; re-enable it with `gh workflow enable advisories.yml`.

Run `actionlint` before committing a workflow change.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
