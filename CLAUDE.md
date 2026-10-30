# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Nage is household accounting: one Rust binary, server-rendered HTML with htmx, one SQLite file, a double-entry ledger whose UI never says "debit" or "credit". The specification is SPEC.md and the ordered task list is PLAN.md; read them when planning a slice, not by default.

## Commands

| Purpose | Command |
|---|---|
| Test | `cargo test --workspace` |
| One test | `cargo test -p nage-core test_runner_reaches_core` (substring match on the test path) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck | `cargo check --workspace --all-targets` |
| Format | `cargo fmt --all` (check only: `cargo fmt --all --check`) |
| Licenses | `cargo deny check licenses` |
| Query data check | `cargo sqlx prepare --check --workspace -- --all-targets` |
| Lint the workflows | `actionlint` |
| Advisories (network; not a pre-commit check) | `cargo deny check advisories` |
| Build | `cargo build --workspace` |
| Run locally | `cargo run -p nage -- serve` |
| Seed a fixture database | `cargo xtask seed --out <path>` |
| Write or refresh a snapshot | `INSTA_UPDATE=always cargo test -p <crate> --test <target>` (`--lib`, or `--bin <name>` in a binary crate, for a snapshot under `src/`) |
| More property cases | `PROPTEST_CASES=4096 cargo test -p nage-core` |

Run test, lint, typecheck, format check, licenses and the query data check before every commit, and `actionlint` when a workflow changed. CI (`.github/workflows/ci.yml`) runs the same six, one job each, with `--locked` added where cargo accepts it, on pull requests and pushes to `main`; the docker, e2e and fixture-guard jobs arrive with slices 6, 7 and 8 (SPEC decision 64). `advisories.yml` runs the advisory check weekly (once it is on `main`) and opens an issue on failure (decision 65). `cargo run -p nage -- serve` creates `data/nage.sqlite`, applies migrations and serves `/`, `/health` and `/static/*`; `nage health` probes it.

Toolchain: `rust-toolchain.toml` pins Rust 1.99.1 with rustfmt and clippy; `rustup toolchain install` in the repo installs it. cargo-deny comes from the package manager (Arch: `sudo pacman -S --needed rustup cargo-deny actionlint shellcheck`; actionlint, which uses shellcheck for the `run:` scripts, lints the workflows) or `cargo install cargo-deny --locked`; `deny.toml` is written for cargo-deny 0.20. `cargo sqlx` comes from sqlx-cli 0.9.0, the same minor as the sqlx line (Arch: `sudo pacman -S --needed sqlx-cli`; the README's Database and sqlx section has the rest).

## Conventions that differ from defaults

- **Every clippy finding is an error.** `[workspace.lints.clippy] all = { level = "deny", priority = -1 }` in the root Cargo.toml (the priority lets a specific lint override the group), inherited by each crate through `[lints] workspace = true`; the lint command adds `-D warnings` for the compiler's own warnings.
- **No floats in nage-core.** `#![forbid(clippy::float_arithmetic)]` in `crates/nage-core/src/lib.rs` (INV-3, SPEC decision 39); `forbid` makes any `#[allow]` a compile error. Money is `i64` minor units with checked arithmetic; decimal strings are parsed and formatted exactly (SPEC 6.10).
- **Dependencies are license-gated.** `deny.toml` holds `[licenses]` and `[advisories]`; the license allowlist starts empty and grows one line at a time in the slice that adds the dependency (`cargo deny check licenses` fails until it does); an advisory `ignore` entry carries the id, a reason and when to revisit it, and is the maintainer's decision (SPEC decision 65). The workspace crates are `publish = false` and skipped by the check. `include-dev = true`, so dev-dependencies are checked too (decision 63).
- **Dependencies are declared once in `[workspace.dependencies]`; members add features; `Cargo.lock` is the pin** (SPEC decision 51).
- **Edition 2024, resolver 3, all package metadata inherited** from `[workspace.package]`. The version is `0.N.0` per milestone, bumped in the milestone PR (SPEC 5.2, PLAN 1.2).
- **Timestamps are written only from Rust**, as RFC 3339 UTC with whole seconds, never from SQLite defaults; the one exemption is `_sqlx_migrations`, sqlx's own table, which Nage only reads (SPEC decision 57). Dates are ISO calendar dates with no time zone; periods are half-open. (SPEC 5.4, 6.4)
- **One code path.** Every ledger operation is a function in nage-core taking a database transaction and the acting user; the web layer calls it and tests call it directly. Queries are compile-time checked by sqlx with the offline data committed in `.sqlx`. Report queries return typed models consumed by both templates and golden tests. (SPEC 11, PLAN 1.2)
- **Every table is `STRICT`**, booleans are INTEGER 0 or 1 with a CHECK, and every date and timestamp column carries a format CHECK (SPEC 7, decision 56).
- **Builds are offline.** The committed `.env` holds `DATABASE_URL=sqlite:target/dev.sqlite` and `SQLX_OFFLINE=true`, never a secret, so every build reads the committed `.sqlx`. After a query change run `cargo sqlx prepare --workspace -- --all-targets` and commit `.sqlx`; it needs the dev database (`cargo build` then `cargo sqlx database setup`; `cargo sqlx migrate run` after a migration change; `cargo clean` deletes it). While `0001_init.sql` is unreleased an edit to it changes its checksum: use `cargo sqlx database reset -y` and delete `data/`. Run `cargo sqlx` from the repository root. (SPEC decision 57)
- **Query rules.** Every query carries explicit type overrides for computed columns, booleans, dates and money (`"count!: i64"`); the query builder is only for filters, search and pagination. Unchecked SQL (a literal, or a statement assembled from test constants and marked `AssertSqlSafe`) is only for PRAGMAs, `VACUUM INTO`, migration bookkeeping, transaction control such as `BEGIN IMMEDIATE`, connection-mode tests, and schema or corruption tests, never for building a scenario database. (SPEC decision 57)
- **The committed fixture `fixtures/books/v0.1.0.sqlite` is never opened in place**; copy it first (the shared helper `fixture_copy` does), because opening it creates WAL files beside it. When `0001_init.sql` changes before `v0.1.0` is tagged, regenerate it with `rm fixtures/books/v0.1.0.sqlite` then `cargo xtask seed --out fixtures/books/v0.1.0.sqlite` (the command refuses to overwrite). (SPEC decision 58)
- **Shared test helpers live in `nage_core::test_support`, behind the `test-support` feature**, enabled only by dev-dependencies (nage-core lists itself), never by a production build; nage-core's own unit tests under `src/` use `crate::test_support` (the self dev-dependency links a second copy of the library), its `tests/` use `nage_core::test_support` like every other crate. Each crate's `tests/common` keeps only crate-local helpers. (SPEC decision 59)
- **Snapshots (insta):** file snapshots beside the test under `snapshots/`, the `.snap` committed, `*.snap.new` and `*.pending-snap` ignored. Redact varying values (versions, paths, times) before asserting. Never set `INSTA_FORCE_PASS`. Review a new or changed snapshot with `cargo insta review` or write it with `INSTA_UPDATE=always` and read it in `git diff` (a new snapshot is untracked: read the file itself, or run `git add -N <file>` first). (SPEC decision 60)
- **Properties (proptest):** every property declares `#![proptest_config(test_support::proptest_config(file!()))]`: 256 cases by default, `PROPTEST_CASES` overrides it, regression files under `crates/<crate>/proptest-regressions/` are committed. Pure properties sit in the module's `#[cfg(test)]` block under `src/`; database-backed ones are integration tests. A crate other than nage-core adds `proptest.workspace = true` to its `[dev-dependencies]` with its first property. A property without the `proptest_config` line falls back to proptest's default layout: for an integration test proptest prints a warning and writes `<file>.proptest-regressions` beside it; for a `src/` property there is no warning and it writes `proptest-regressions/<file>.txt` without the `src/` level; `git status` shows the file and review rejects it. (SPEC decision 61)
- **Only the `nage` binary reads the environment; `nage-web` is handed values** (SPEC decision 42); the test tooling's own variables (`PROPTEST_*`, `INSTA_*`, `CI`, `TMPDIR` and the test runner's own) are read by the harness at test time and are outside this rule (decision 59).
- **Tests reference the invariant IDs** (INV-n below) they prove or, where none applies, the narrowest SPEC clause, decision or PLAN slice (SPEC decision 52); the last line of the doc comment reads `Proves:` followed by what the test proves (decision 62). Scenario databases are built through core operations only, never raw SQL, so every scenario is also a `nage check` subject. Golden snapshots (insta) cover CSV parsing per fixture, rule application, import summaries and report models; report goldens snapshot the data model, not the HTML. (SPEC 10.1)
- **Node is test-time only**, for Playwright in `e2e/` (slice 7). No bundler, no Node at build time.
- **Workflows.** Every action in `.github/workflows/` is pinned to a full commit SHA with a `# vX.Y.Z` comment verified against the tag, and every pin is re-resolved at each milestone PR. The runner image is named explicitly (`ubuntu-24.04`), checkouts do not persist credentials, and no job sets `SQLX_OFFLINE` (the committed `.env` already does). The test job sets `INSTA_UPDATE=no` and `PROPTEST_CASES=1024`. CI adds `--locked` where cargo accepts it and the local commands do not. Later slices add their jobs to `ci.yml`. Run `actionlint` after any workflow change. (SPEC decisions 64 and 65)
- **Commits.** Commits use the maintainer's GitHub noreply address, carry no `Co-Authored-By` or session trailers, and are made with `TZ=UTC git commit`; pull-request descriptions carry no tooling or session line either. A CI failure found after a push on a milestone branch is fixed by an ordinary commit named for the failure, independently reviewed before its push; a fix that loosens or skips a test, adds an advisory ignore or a licence line, or changes a file outside the slice being built waits for the maintainer. A milestone branch's draft pull request into `main` is opened at its first push, since `ci.yml` starts by itself only on pull requests and on pushes to `main` (decision 67). Claude never pushes `main`, never force-pushes and never rewrites pushed history. Before every push, before a pull request is opened and before its description is changed, run the leak gate of decision 67 over every commit to be pushed and over the PR body (the new text, when the description changes): `git log --format='%ae %ce %ad %cd' --date=iso origin/main..HEAD` shows only the noreply address and `+0000`; neither `git log --format=%B origin/main..HEAD` nor the PR body has a `Co-Authored-By:` or `Claude-Session:` line, and the PR body has no tooling line (such as "Generated with Claude Code") and no session link; no file in those commits, no commit message and no PR body names a bank or a personal mailbox address. Any hit stops the push, the pull request or the description change. The documents name no bank, host or household member. (SPEC decisions 66 and 67)
- **Formatting is automatic for Edit and Write.** A PostToolUse hook in `.claude/settings.json` runs rustfmt on every `.rs` file those tools touch. A `.rs` file written by a Bash command (heredoc, `sed -i`) is not formatted; run `cargo fmt --all` after such writes, or the format check fails.

## Invariants (SPEC section 9)

Enforcement: Core = nage-core refuses the operation; Schema = constraint or trigger; Check = a `nage check` query; Lint = compile-time; HTTP = middleware proven by integration tests. Only Check-marked invariants appear in `nage check` output; the others are proven by the tests named in SPEC 10.3.

| ID | Statement | Enforced by |
|---|---|---|
| INV-1 | Every transaction's postings sum to zero, live or deleted | Core, Check |
| INV-2 | At least two postings per transaction; no zero amount | Core, Schema, Check |
| INV-3 | Amounts are i64 minor units; no float arithmetic in core | Lint (forbid), Core |
| INV-4 | All live postings sum to zero: Assets + Liabilities + Equity + Income + Expenses = 0 | Check |
| INV-5 | One currency and minor-digit count, set at setup, never changed; no other table carries a currency | Core, Schema |
| INV-6 | Transaction and bank dates are valid ISO calendar dates; periods are half-open | Core, Schema |
| INV-7 | Each write to a transaction or its postings bumps its version by one; edit forms name the expected version | Core |
| INV-8 | Exactly five roots, one per kind, each system-keyed; no posting on a root | Core, Check |
| INV-9 | A child's kind equals its parent's; no cycles; depth at most six segments including the root | Schema, Core, Check |
| INV-10 | Sibling names unique ignoring case; root names unique; no colons | Schema, Core |
| INV-11 | The eight system-keyed accounts exist and are never deleted, archived or re-parented; roots are never renamed | Core, Check |
| INV-12 | An archived account's live postings sum to zero, its children are archived, nothing touching it was created after archiving | Core, Check |
| INV-13 | At most one live opening-balance transaction per account, only on Assets or Liabilities accounts: two postings, the account against Equity:Opening Balances | Core, Check |
| INV-14 | A balance_update transaction has two postings, one on Equity:Valuation Changes | Core, Check |
| INV-15 | An import record exists only with all its rows and outcomes; (account, file hash) is unique | Core, Schema |
| INV-16 | One outcome per import row; created, matched_transfer and linked_manual rows are referenced by exactly one posting on the import's account, other rows by none | Schema, Check |
| INV-17 | Fingerprints are unique per account over live and deleted postings; a posting with an import row carries its row's fingerprint | Schema, Check |
| INV-18 | A fingerprint depends only on account, bank id or (date, amount, normalized description, occurrence index among surviving rows); permuting rows keeps the multiset | Core, property test |
| INV-19 | A posting with an import row never changes account or amount | Core |
| INV-20 | expects_mirror is set only on asset or liability postings without an import row | Core, Check |
| INV-21 | An open review item references a row with outcome review or a posting with expects_mirror set; a resolved item has resolution, resolver and time, and its row's outcome is final | Core, Check |
| INV-22 | A posting in a completed reconciliation keeps account, amount, transaction date and bank date; its transaction is not deleted | Core, Check |
| INV-23 | Postings linked to a completed reconciliation or any earlier completed reconciliation of that account sum to its statement balance | Core, Check |
| INV-24 | Per account: completed reconciliations have strictly increasing statement dates; at most one in progress; only the latest completed can be reopened | Schema, Core |
| INV-25 | Deleted transactions are excluded from every balance, report, candidate list, suggestion and queue, and are restorable; hard delete only through import undo or transfer merge, never with a reconciled posting | Core, Check |
| INV-26 | Every row in accounts, transactions, tags, rules, import_profiles, imports, reconciliations and review_items names who created it and, when changed, who changed it | Schema, Check |
| INV-27 | Every route outside /setup, /login, /health and /static/* needs a valid, unexpired session for an active user; /setup is unavailable once any user exists | HTTP |
| INV-28 | Every mutating HTTP request runs in one database transaction: full success or no trace | Core |
