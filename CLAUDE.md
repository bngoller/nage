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
| Build | `cargo build --workspace` |

Run test, lint, typecheck, format check and licenses before every commit. CI (milestone 1 slice 5, not yet written) runs these plus the sqlx prepare check, fixture guard, docker and e2e jobs (PLAN.md slice 5). Nothing serves yet: `cargo run -p nage -- serve` arrives with milestone 1 slice 2.

Toolchain: `rust-toolchain.toml` pins Rust 1.99.1 with rustfmt and clippy; `rustup toolchain install` in the repo installs it. cargo-deny comes from the package manager (Arch: `sudo pacman -S --needed rustup cargo-deny`) or `cargo install cargo-deny --locked`; `deny.toml` is written for cargo-deny 0.20.

## Conventions that differ from defaults

- **Every clippy finding is an error.** `[workspace.lints.clippy] all = { level = "deny", priority = -1 }` in the root Cargo.toml (the priority lets a specific lint override the group), inherited by each crate through `[lints] workspace = true`; the lint command adds `-D warnings` for the compiler's own warnings.
- **No floats in nage-core.** `#![forbid(clippy::float_arithmetic)]` in `crates/nage-core/src/lib.rs` (INV-3, SPEC decision 39); `forbid` makes any `#[allow]` a compile error. Money is `i64` minor units with checked arithmetic; decimal strings are parsed and formatted exactly (SPEC 6.10).
- **Dependencies are license-gated.** `deny.toml` holds only `[licenses]`; its allowlist starts empty and grows one line at a time in the slice that adds the dependency (`cargo deny check licenses` fails until it does). The workspace crates are `publish = false` and skipped by the check.
- **Edition 2024, resolver 3, all package metadata inherited** from `[workspace.package]`. The version is `0.N.0` per milestone, bumped in the milestone PR (SPEC 5.2, PLAN 1.2).
- **Timestamps are written only from Rust**, as RFC 3339 UTC with whole seconds, never from SQLite defaults. Dates are ISO calendar dates with no time zone; periods are half-open. (SPEC 5.4, 6.4)
- **One code path.** Every ledger operation is a function in nage-core taking a database transaction and the acting user; the web layer calls it and tests call it directly. Queries are compile-time checked by sqlx with the offline data committed in `.sqlx` (from slice 3); the query builder is allowed only for filters, search and pagination. Report queries return typed models consumed by both templates and golden tests. (SPEC 11, PLAN 1.2)
- **Tests reference invariant IDs** (INV-n below). Scenario databases are built through core operations only, never raw SQL, so every scenario is also a `nage check` subject. Golden snapshots (insta, from slice 4) cover CSV parsing per fixture, rule application, import summaries and report models; report goldens snapshot the data model, not the HTML. (SPEC 10.1)
- **Node is test-time only**, for Playwright in `e2e/` (slice 7). No bundler, no Node at build time.
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
