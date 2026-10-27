# Nage v1 Specification

Status: draft 3, 2026-10-26. Written from the kickoff interview, amended after the planning interview, then revised after two independent reviews (a consistency review and a skeptical-implementer review) the same day. Section 2 records every decision and what was rejected, so later arguments can start from the record instead of from memory.

## 1. Summary

Nage is open-source (AGPL-3.0) personal accounting for one household. It runs as a single Rust binary on a home server, serves a browser UI that works on a phone, and keeps one set of books in one SQLite file. Under the hood it is a double-entry, cash-basis, single-currency ledger. On the surface it says "paid from Checking to Groceries" and never says "debit" or "credit".

The workflow v1 must nail: import bank and card CSV exports, categorize them with rules, and see where the money went. Everything else in this document is either in service of that workflow or small enough not to endanger it.

## 2. Decisions record

| # | Topic | Decision | Rejected alternatives |
|---|-------|----------|-----------------------|
| 1 | Audience | A shared household, several people on one set of books | Single user; built for strangers from day one |
| 2 | Must-nail workflow | Import, categorize, see where money went | Net worth over time; reconcile to the penny; replace a spreadsheet |
| 3 | Accounting model | Double-entry core, plain-language UI | Ledger-style UI with signed postings; single-entry with categories |
| 4 | Basis | Cash basis, credit cards as liability accounts | Accrual |
| 5 | Household shape | Home server with simple logins (chosen over the smaller "no logins" option; auth, sessions and concurrent writes are v1 work) | One install with no logins; device sync |
| 6 | Platform | Local web app, server-rendered | CLI; desktop app; TUI |
| 7 | Storage | Single SQLite file | Plain-text ledger; Postgres |
| 8 | Data sources | CSV import plus manual entry | OFX/QFX/QIF; direct bank connections |
| 9 | Currency | Single currency per book | Per-account currency; full multi-currency |
| 10 | Duplicate imports | Fingerprint per row plus a review queue for near-matches | Import everything and flag later; refuse overlapping date ranges |
| 11 | Categorization | User rules plus "same as last time" suggestions | Manual only; learned classifier |
| 12 | Reconciliation | Light: a statement-balance check with per-posting ticking and locking; no line-by-line statement matching and no statement import | None; full reconciliation sessions against imported statements |
| 13 | Reports | Register, income vs expense by category, balance sheet, month-over-month trends | Budget vs actual |
| 14 | Budgets | None in v1 | Simple monthly targets; envelope budgeting |
| 15 | Investments and assets | Balance-only accounts whose changes post to a valuation-changes equity account | Out entirely; lots, prices and cost basis |
| 16 | Extras in scope | Tags on transactions; phone-friendly layout | Recurring transactions; attachments |
| 17 | Stack | Rust, axum, sqlx, server-rendered HTML with htmx | Go; TypeScript; Python |
| 18 | Auth | Users with passwords, everyone sees everything, first user is admin | Roles; per-user private accounts; one shared password |
| 19 | History | Who/when stamps plus soft delete | Hard delete; append-only audit log |
| 20 | License | AGPL-3.0 | GPL-3.0; Apache-2.0; MIT |
| 21 | Transfers | Auto-match mirror rows across imports; transfers between household members are transfers, not income or expense | Import both legs and merge by hand; import one side only |
| 22 | Test fixtures | Real anonymized exports from the household's banks | Synthetic only |
| 23 | Fixture banks and currency | Major US and Canadian banks, Canadian first; the book currency is CAD | Synthetic fixtures; USD |
| 24 | Multi-account exports | A profile may name an account-selector column; one import run per account, the selector value chosen at import time; single-account exports leave it blank | One run fanning out to every mapped account; splitting files by hand |
| 25 | Reference deployment | The household server (x86_64, Ubuntu, Docker Compose): a Compose project on its service pattern, an amd64 image on GHCR, HTTPS through tailscale serve | arm64 targets; TLS inside Nage |
| 26 | Process | GitHub Actions on `main`; about eight milestones; one PR per milestone, CI green, reviewed and merged by the maintainer | PR per slice; commits straight to main; five or fifteen milestones |
| 27 | Ordering | Auth in milestone 2 before any ledger screen; import before manual entry; deployed to the household server from milestone 2 and redeployed each milestone | Dev auto-login; manual ledger first; deploy only at v1 |
| 28 | UI base | Vendored classless CSS framework (Pico CSS, classless build) plus a small custom sheet; Playwright from milestone 1 | Handwritten CSS only; Tailwind; browser tests later |
| 29 | Undo import | An import can be undone outright (superseded by row 32) | Soft-deleting the import's transactions, which would block a corrected re-import by fingerprint |
| 30 | Fix-release lane (amends 26) | Besides the milestone PRs, a `fix/<slug>` PR may land a regression fix found after a deploy, tagged `v0.N.x`; the PR's "how to try it" is a post-merge check on the server | Waiting for the next milestone; PR images built before merge |
| 31 | Browser matrix (amends 28) | Two Playwright projects: WebKit at phone size (390×844, the iPhone 13 profile) and Chromium at desktop size (1280×800); the household's phones are iPhones | Four projects (both browsers at both sizes) |
| 32 | Undo semantics (amends 29) | Undo removes an import outright whenever none of its postings is reconciled and no later import references it; edits, categorizations, tags and soft deletes made since the import are counted on the confirmation page and lost with it | Refusing undo after any edit, which left a bad import stuck until manual delete existed |
| 33 | Currency immutability | The currency is chosen at setup (a select defaulting to CAD) and never changes afterwards; a wrong choice means starting the data directory over before entering data | Editable until the first posting |
| 34 | Transfers deferred wholesale (amends 27) | Milestone 3 lets rules and inline picks target income and expense accounts only; nothing creates a transfer leg until mirror matching exists in milestone 5 | Creating transfer legs in milestone 3 that nothing could match, which double-counted every transfer |
| 35 | Security headers | A Content Security Policy with no inline scripts or styles, `Referrer-Policy: same-origin`, and htmx configured with eval disabled, from milestone 2 | Adding headers at the end |
| 36 | Dependency checks | `cargo deny` license check blocks merges; the advisory check runs on a schedule and does not block unrelated PRs | Advisories blocking every PR; no dependency checks |
| 37 | Scheduled snapshots | None inside Nage; consistent backups are the on-demand `VACUUM INTO` plus the host's backup taking a consistent copy by path | A nightly in-process snapshot with retention |

## 3. Goals

- G1. First evening: a household member goes from a fresh install to categorized books in one sitting. Setup, create accounts, build one import profile per bank, import a month, categorize, read the report.
- G2. Monthly upkeep under 30 minutes for about six accounts and 400 rows a month: re-import, clear the review queue, categorize the remainder, reconcile each account.
- G3. The books always balance. Every invariant in section 9 has an ID and a test; every invariant that can be checked after the fact also has a line in the output of `nage check`.
- G4. Nothing is lost by accident. Raw import rows are kept verbatim, deletes are soft, a backup is one button, export is complete. The two deliberate exceptions, undoing an import and merging the two sides of a transfer, remove exactly what they say.
- G5. Runs on modest hardware with no external services. One binary, one data directory, no Node toolchain needed to build. The published image is amd64; other targets build from source.
- G6. Daily tasks work on a phone: categorizing, entering a cash expense, checking a balance.
- G7. Household aware. Accounts have owners, transfers between members are transfers, and every change records who made it.

## 4. Non-goals

Each item names the reason and the earliest version it could land. None of these may be partially built in v1 "because it was easy".

| Non-goal | Why not now | Earliest |
|----------|-------------|----------|
| Multi-currency | Touches every report and invariant; the single biggest complexity multiplier. A USD column in a CAD export is never imported | v2 |
| Budgets | Only useful once categories have settled after months of real use | v2 |
| Investments with lots, prices, cost basis | A product of its own; balance-only accounts cover net worth | v3 |
| Direct bank connections (Plaid, SimpleFIN, GoCardless) | Credentials, API keys, rate limits, and a murky open-source story | v3 |
| OFX/QFX/QIF import | The pipeline after parsing is format-agnostic, so this is a parser away | v2 |
| Accrual accounting | Personal books are cash basis | never |
| Recurring and scheduled transactions | Imports bring in the real ones; templates need a posting engine and reminders | v2 |
| Receipts and attachments | File storage, thumbnails, bigger backups, no gain for the must-nail workflow. Uploaded CSVs are never stored | v2 |
| Per-user private accounts, roles beyond admin | Authorization filter on every query doubles the test surface | v2 |
| Expense splitting between members, who-owes-whom | The household is one economic unit; settlement tracking is a different product | never in core |
| Sync, offline, multi-device merge | Conflict resolution for financial data is a v3 problem | v3 |
| Multiple books per install | One household, one book; run two instances if needed | v2 |
| Stable public JSON API | htmx endpoints are internal and may change freely; `/health` promises only its HTTP status | v2 |
| Localization beyond currency minor digits | English UI only; one number format; the currency table holds codes and minor digits, nothing else | v2 |
| Native mobile apps | The responsive web UI is the phone app | never |
| Forecasting, bill reminders, invoicing, tax reports | Out of the household bookkeeping scope | v3 |
| Payee entities, merchant enrichment | Payee is text; rules and suggestions do the work | v2 |
| Plugin system | No stable internal API to plug into | v3 |
| TLS termination | Run behind tailscale serve, Caddy or nginx; documented, not built. Nage never inspects the connection for TLS | never |

## 5. Users, deployment, security

### 5.1 Users and the household

- One book, one household. Every signed-in user sees and may edit everything.
- The first user, created at setup, is the admin. Admins create users, reset passwords and deactivate users. The last remaining admin cannot be deactivated or demoted.
- Every user has a username (unique, case-insensitive), a display name and a password. No email, no self-service recovery; an admin resets passwords. Every user can change their own password and display name.
- Every account may have an owner (a user) or be joint. Ownership is a label for display and filtering. It is never an authorization boundary.
- Every row a user creates or edits records who and when, except sessions and postings; postings belong to their transaction, whose stamps cover them. The first admin's own `created_by` is empty.

### 5.2 Deployment

- One binary `nage` with subcommands: `serve`, `health`, `init`, `user add`, `user reset-password`, `check`, `backup`, `export`, `migrate`. Development tooling (seeding scenario databases, generating fixtures) lives in an `xtask` crate, not in the shipped binary.
- Configuration via flags or `NAGE_*` environment variables: data directory (`NAGE_DATA_DIR`, default `./data`, created with its `backups/` subfolder if missing), bind address (`NAGE_BIND`, default `127.0.0.1:8787`; the image sets `0.0.0.0:8787`), trust-proxy flag (`NAGE_TRUST_PROXY`, default off), time zone (`NAGE_TZ`, an IANA name; defaults to `TZ`, then UTC; an invalid name refuses to start), log level (`NAGE_LOG`, default `info`, text format).
- Logs carry method, path without query string, status, latency and user id. Never cookies, form bodies, passwords or CSV contents.
- Data directory holds `nage.sqlite` with its `-wal` and `-shm` companions, and `backups/`. Copying `nage.sqlite` alone while the server runs is not a backup; a backup is a `VACUUM INTO` snapshot (`nage backup`, the Settings button, or the host's consistent copy by path), or a copy of all three files with the server stopped.
- `GET /health` is unauthenticated and returns JSON with `version` and `ok`, with status 200 when the database answers and 503 otherwise. Only the status code is a contract. `nage health` performs that request against the local server (derived from `NAGE_BIND`, with `0.0.0.0` mapped to `127.0.0.1`), with a two-second timeout, exiting 0 only on 200, so a container health check needs no curl.
- `serve` applies pending migrations at startup, writing a pre-migration backup first (section 5.5), then runs `check` and logs the result. `check` on a database with no book row reports "not initialized" and exits 0. The server shuts down gracefully on SIGTERM and SIGINT.
- Published as an amd64 image on GHCR with exact version tags only (no `latest`), plus the same Linux binary attached to each GitHub release. The binary is a static `x86_64-unknown-linux-musl` build with SQLite bundled and the time-zone database embedded, so it runs on any x86_64 Linux and in a distroless runtime image. The image runs as uid 1000 with `/data` pre-created and owned by that uid, declares an exec-form `HEALTHCHECK` on `nage health`, and sets `NAGE_DATA_DIR=/data` and `NAGE_BIND=0.0.0.0:8787`.
- Versioning: each milestone PR bumps the workspace version to `0.N.0`; after the merge the maintainer pushes the tag `v0.N.0`, which runs the release workflow; the workflow fails if the tag and the Cargo version differ. Fix releases are `v0.N.x` from `fix/<slug>` PRs (decision 30).
- Reference deployment: the household server, an x86_64 Ubuntu box where every service is a Docker Compose project with a `data/` folder, a container user of 1000:1000, `restart: unless-stopped`, `TZ` set, and one published port reachable from the LAN and the tailnet. Nage follows that pattern. HTTPS for phones comes from `tailscale serve` in front of the published port; the LAN reaches it over plain HTTP. The host's nightly backup copies the data directory as files and takes a consistent copy of the SQLite file by path. Host-specific values (addresses, names, script paths) stay in that server's runbooks, never in this repo. `/setup` is open to anyone who can reach the port until it has been used, so the deployment guide recommends `nage init` before publishing the port.

### 5.3 Security

- Passwords hashed with argon2id at m=19456 KiB, t=2, p=1 (the OWASP recommendation and the argon2 crate default), in a blocking thread outside any database transaction; cheap parameters under `cfg(test)`. Minimum length 10, maximum 1 KiB, no composition rules; the setup and change-password forms ask for the password twice. Login runs a dummy verification for unknown usernames so timing does not reveal them.
- Sessions: 32 random bytes; only the SHA-256 of the token is stored. Cookie `nage_session`, `Path=/`, HttpOnly, SameSite=Lax, with `Max-Age` refreshed as the session slides. It is marked Secure if and only if the trust-proxy flag is on and the request carries `X-Forwarded-Proto: https` (set by `tailscale serve` or a reverse proxy); Nage never inspects the connection itself. Expiry is the earlier of 30 days after last use and 90 days after creation; `last_seen_at` is written at most once an hour per session. Logout is a POST. A password reset revokes the user's sessions. Expired sessions are deleted at each login.
- Every route except `/setup`, `/login`, `/health` and `/static/*` requires a valid session for an active user. With no user in the database every other route redirects to `/setup`, `/login` included; `/setup` returns 404 once any user exists. An unauthenticated plain request gets a 303 to `/login?next=<relative path>` (never an absolute URL); an unauthenticated htmx request gets `HX-Redirect: /login`.
- Mutating requests (POST, PUT, PATCH, DELETE) must carry an `Origin` header, or `Referer` when `Origin` is absent, whose host and port equal the request's `Host` header or, with trust-proxy on, its `X-Forwarded-Host`; the scheme is ignored. A missing or mismatched header gets 403. `Referrer-Policy: same-origin` keeps browsers from sending `Origin: null`.
- Response headers on every page: `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'; base-uri 'self'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`. No inline scripts or styles anywhere; htmx is configured through its `<meta>` config with `allowEval: false` and `includeIndicatorStyles: false`, and `hx-on` attributes are not used.
- Login rate limit, kept in memory and keyed by the case-folded username whether or not it exists: after five failures within 15 minutes the sixth and later attempts are refused with 429 and a `Retry-After`, the lock lasting 2^(k−5) seconds for the k-th failure, capped at 15 minutes, reset on success. A refused attempt does not check the password.
- Request bodies are limited to 64 KiB, except upload routes, which accept 10 MiB.
- No secrets in the database beyond password and session hashes. No telemetry, no outbound network calls of any kind.

### 5.4 Concurrency and HTTP conventions

- SQLite in WAL mode with `synchronous=NORMAL`, `foreign_keys=ON` and a busy timeout of five seconds. Writes go through a pool of exactly one connection and every write transaction starts with `BEGIN IMMEDIATE`; reads use a separate pool. Password hashing and CSV parsing happen before the write transaction opens. When the busy timeout is exhausted the request gets 503 with `Retry-After` and a "busy, try again" page.
- Every mutating request runs inside one SQLite transaction. An import, its sweep included, is one transaction; a partially written import cannot exist.
- Transactions (the ledger kind) carry a version number. Every write to a transaction or its postings, by a form, an import link, a merge, a reconciliation tick or a rule application, increments it by one. Only edit forms name the version they expect; a mismatch returns 409 and shows the current data. Two household members editing the same transaction cannot silently overwrite each other.
- Full-page forms are plain POSTs: 303 on success, 422 with the re-rendered form on validation errors, 409 with the current data on a stale version, 403 on a failed Origin check. htmx partial requests get the same codes, and htmx's response handling is configured to swap 409 and 422 bodies. Timestamps are written only from Rust as RFC 3339 UTC with whole seconds (`2026-10-26T12:00:00Z`), never from SQLite defaults, and displayed in the configured time zone.

### 5.5 Migrations and the pre-migration backup

- sqlx migrations, forward only, applied by `serve` at startup and by `nage migrate` (with the server stopped). Migration files are frozen once released: enum-valued columns carry their full v1 value set in the migration that creates them, and a table's CHECK constraints are final from creation.
- When migrations are pending and the database already has a migrations table, the server first writes `backups/pre-migrate-<last applied migration>-to-<app version>-<UTC stamp>.sqlite` via `VACUUM INTO`; a failed backup aborts startup without migrating. The very first migration gets no backup. Retention is unbounded; the deployment guide says when to prune.
- An older binary started against a newer database exits with a message naming the newest backup; the deployment guide describes the restore.

## 6. Accounting model

### 6.1 Double entry, plain language

A transaction is a dated set of two or more postings. Each posting names an account and a signed integer amount in minor units. The amounts of a transaction's postings sum to zero. That is the whole model; everything else is presentation.

The UI never shows debits or credits. A simple transaction is shown as date, payee, amount, from-account, to-account. A split transaction is shown as one from-account and several to-lines whose amounts sum to the total. Internally these are just postings.

### 6.2 Chart of accounts

Five root accounts exist from setup, one per kind: Assets, Liabilities, Equity, Income, Expenses. Every other account has a parent and inherits its kind from its root. Categories are simply accounts under Income or Expenses. Full names join the path with a colon, for example `Expenses:Food:Groceries`.

Rules:
- Roots never receive postings. Any non-root account may.
- A name is trimmed, non-empty, at most 64 characters, and contains no colon. Sibling names are unique ignoring case (a Unicode-folded copy of the name is stored for the index); root names likewise. Depth is at most six path segments, the root included.
- An account can be moved under a different parent of the same kind, never under its own descendant, never to a depth beyond six, never to a different kind.
- Owners are set only on Assets and Liabilities accounts.
- Reports roll children up into parents.
- Archiving hides an account from pickers and lists. Archiving requires every child to be archived first and the account's own live postings to sum to zero over all dates; an archived account accepts no new postings and no restore of a deleted transaction. Unarchive is always allowed. There is no delete: a mistyped account is renamed or archived, and the UI says so.

System accounts, created at setup, carry a `system_key` so code and checks find them after a rename: the five roots (`assets`, `liabilities`, `equity`, `income`, `expenses`), `Equity:Opening Balances` (`opening_balances`), `Equity:Valuation Changes` (`valuation_changes`) and `Expenses:Uncategorized` (`uncategorized`). Roots cannot be renamed; the other three can. None can be archived, re-parented or re-kinded.

A starter chart is offered at setup (a checkbox, on by default) and can be edited freely afterwards: Assets (Checking, Savings, Cash), Liabilities (Credit Card), Income (Salary, Other), Expenses (Groceries, Dining, Housing, Utilities, Transport, Health, Entertainment, Shopping, Subscriptions, Travel, Gifts, Fees, Other). Declining it leaves the roots and the three system accounts; the home page then shows an empty state with a link to create the first account. The starter chart cannot be added later.

### 6.3 Sign convention and display

Stored amounts follow one convention: a posting's amount is the change in that account's stored balance, where asset and expense balances are normally positive and liability, income and equity balances are normally negative. For an asset or liability account, a positive posting is what that account's own statement shows as money in: a deposit, or a payment received by a card.

Display uses the natural sign for each kind: `natural = stored × kind_sign`, with `kind_sign` of +1 for Assets and Expenses and −1 for Liabilities, Income and Equity. A credit card with a stored balance of −1,200.00 displays as 1,200.00 owed. Salary with a stored total of −3,000.00 displays as 3,000.00 income.

Worked examples (stored amounts; account names are illustrative, not the starter chart):

| Event | Posting 1 | Posting 2 | Effect |
|-------|-----------|-----------|--------|
| Groceries 45.00 on the card | Liabilities:Card −45.00 | Expenses:Groceries +45.00 | Owed up by 45, Groceries up by 45 |
| Paycheck 3,000.00 | Assets:Checking +3,000.00 | Income:Salary −3,000.00 | Checking up, Salary income 3,000 |
| Pay card 500.00 from checking | Assets:Checking −500.00 | Liabilities:Card +500.00 | Transfer; no income or expense |
| Alice sends Bob 200.00 by e-transfer | Assets:Alice Checking −200.00 | Assets:Bob Checking +200.00 | Transfer; net worth unchanged |
| Refund 20.00 to the card | Liabilities:Card +20.00 | Expenses:Groceries −20.00 | Groceries total reduced |
| Opening balance 1,250.00 | Assets:Checking +1,250.00 | Equity:Opening Balances −1,250.00 | Starting point, not income |
| Retirement statement shows +800.00 | Assets:Retirement +800.00 | Equity:Valuation Changes −800.00 | Net worth up, income unchanged |
| Costco 120.00 split | Assets:Checking −120.00 | Expenses:Groceries +90.00, Expenses:Household +30.00 | Three postings, sum zero |

### 6.4 Transactions, postings, dates

Transaction fields: date (calendar date, the accounting date), payee (free text), memo, origin (`manual`, `import`, `opening_balance`, `balance_update`), tags, who/when stamps, version, soft-delete stamps.

Posting fields: account, amount, optional import-row link and fingerprint, optional bank date, `expects_mirror` flag, reconciliation link.

There is one accounting date per transaction. Every balance, every report, every matching window and every duplicate check uses it. A posting may additionally carry a bank date, the date the bank's export gave for that leg. Bank dates exist so that a transfer that left Checking on January 31 and arrived in Savings on February 1 reconciles correctly on both statements. Only reconciliation and the register's secondary date column use bank dates; nothing else does.

Dates have no time zone. "Today" is the current date in the configured time zone (5.2). Periods are half-open: a month is `[first day, first day of next month)`.

### 6.5 Transfers and the household

A transfer is any transaction whose postings all land in Assets or Liabilities accounts. Transfers never appear in income or expense reports. Paying a credit card, moving money to savings, and one household member sending money to another are all transfers, because the household is one economic unit and no money left the house.

For member-to-member payments the receiving account must exist in the book. If a member's account is not imported in detail, create it as a manual or balance-only account so the transfer has somewhere to land. Transfers to accounts outside the household (rent to a landlord, a gift to a relative) are expenses.

### 6.6 Opening balances

An Assets or Liabilities account may have exactly one opening-balance transaction: the account against `Equity:Opening Balances`, dated the day before the first real transaction. The balance is entered in natural sign (a card shows what is owed), must be nonzero, and has no default date. It is created and edited only through the account's opening-balance form, never through the general transaction editor; removing it needs the delete of W8. The form says: "Enter the balance at the end of the day before the first transaction you will import. Imported rows dated on or before this date would be counted twice." The import preview warns when rows that would be created fall on or before that date; it does not block.

### 6.7 Balance-only accounts

An account flagged balance-only (brokerage, retirement, crypto, home, car, mortgage) accepts normal transfers in and out, plus a balance-update action: the user enters a date and the balance from the statement in natural sign, and Nage posts the difference against `Equity:Valuation Changes`. The resulting transaction is ordinary and can be edited; for example, a mortgage update's counter posting can be changed to `Expenses:Interest` by hand, after which its origin becomes `manual`. No lots, no prices, no cost basis.

### 6.8 Uncategorized

Every import row without a matching rule lands against `Expenses:Uncategorized`. It is a real expense account so that totals never silently exclude money that left the house. The home page, the income vs expense report and the trends report show the count of uncategorized postings in the period with a link to the categorize queue (until the queue exists, to the register of that account filtered to the period). Uncategorized inflows (a refund or a paycheck without a rule) show as negative expense until categorized; the banner is the fix.

### 6.9 Cash basis

An expense is recorded on the date it hits a bank or card account. Nothing is owed or accrued. The only equity movements are opening balances, valuation changes and retained earnings (the running total of income and expense), plus any equity accounts the user creates by hand.

### 6.10 Money and dates

- Amounts are 64-bit integers in minor units of the book currency. The number of minor-unit digits comes from the ISO 4217 code chosen at setup; the built-in table holds codes and minor digits for common currencies and nothing else (0, 2 and 3 digits are tested).
- The core crate forbids floating point arithmetic by lint. Parsing and formatting of decimal strings are the only places that touch the decimal point, and they are exact.
- All arithmetic is checked; overflow is a hard error, never a wraparound.
- Display: `1,234.56`, an ASCII hyphen for negatives, no currency symbol in tables, ISO dates. Form input accepts a leading `$`, thousands commas and a leading minus, and refuses more decimals than the currency has; nothing is rounded.
- Averages in reports are computed from integer sums and rounded only for display.

## 7. Data model

Tables below list columns and the constraints that matter. Types are SQLite affinities; all money is INTEGER, all dates are ISO-8601 TEXT with a CHECK on the format, all timestamps are RFC 3339 UTC TEXT, all ids are INTEGER primary keys. Every `created_by`, `updated_by` and `imported_by` is NOT NULL (except `users.created_by`) and references `users`. Every foreign key is `ON DELETE RESTRICT` unless stated. Enum columns carry a CHECK with their full value set from the migration that creates them.

**book** (singleton row, `CHECK (id = 1)`)
name, currency (ISO 4217, set once), minor_digits (0 to 3), created_at

**users**
username (UNIQUE on a case-folded copy), display_name, password_hash, is_admin, disabled_at, created_by (nullable, the first admin has none), created_at

**sessions**
token_hash (UNIQUE), user_id → users, created_at, last_seen_at, expires_at

**accounts**
name, name_folded (for the unique indexes), parent_id → accounts (NULL only for roots), kind (`asset`, `liability`, `equity`, `income`, `expense`), system_key (nullable, UNIQUE), owner_user_id → users (NULL = joint), is_balance_only, archived_at, created_by, created_at, updated_by, updated_at
- UNIQUE (parent_id, name_folded); partial UNIQUE (name_folded) WHERE parent_id IS NULL
- trigger: kind must equal the parent's kind on insert and update; depth, cycles and the count of roots are enforced by core and verified by `check`

**transactions**
date, payee, memo, origin (CHECK in the four values), version (starts at 1), created_by, created_at, updated_by, updated_at, deleted_by, deleted_at
- index on (date, id); partial index on deleted_at IS NULL

**postings**
transaction_id → transactions, account_id → accounts, amount (CHECK amount <> 0), bank_date, import_row_id → import_rows, fingerprint, expects_mirror, reconciliation_id → reconciliations
- partial UNIQUE (account_id, fingerprint) WHERE fingerprint IS NOT NULL
- partial UNIQUE (import_row_id) WHERE import_row_id IS NOT NULL
- index on (account_id, transaction_id)

**tags** name (UNIQUE on a case-folded copy), created_by, created_at
**transaction_tags** transaction_id, tag_id, PRIMARY KEY (transaction_id, tag_id); `ON DELETE CASCADE` from both parents

**import_profiles**
name (UNIQUE), default_account_id → accounts (nullable), config (JSON with a `version` field; the mapping described in W3, including the optional account-selector column), config_fingerprint_hash (a hash of the settings that influence fingerprints), created_by, created_at, updated_by, updated_at

**imports**
profile_id → import_profiles, account_id → accounts, selector_value (nullable), file_name, file_sha256, header (JSON array of column names, or null for a headerless file), config_fingerprint_hash (copied from the profile at import time), min_date, max_date (over rows that survived the filters), row_count (all rows in the file), imported_by, imported_at
- UNIQUE (account_id, file_sha256)
- the per-outcome counts are derived from import_rows, never stored

**import_rows**
import_id → imports, row_index, raw (JSON array of the row's cell values, in file order), date, amount, description, payee, memo, bank_id, fingerprint (all nullable for filtered rows), outcome (CHECK in: `created`, `matched_transfer`, `linked_manual`, `skipped_duplicate`, `skipped_deleted`, `skipped_filtered`, `review`), duplicate_of_posting_id → postings (nullable)
- UNIQUE (import_id, row_index)

**rules**
priority (UNIQUE, ascending, 1 evaluated first), name, enabled, conditions (JSON array, all must match), set_account_id → accounts, set_payee (nullable), add_tag_ids (JSON array), created_by, created_at, updated_by, updated_at

**reconciliations**
account_id → accounts, statement_date, statement_balance (stored sign), state (`in_progress`, `completed`), started_by, started_at, completed_by, completed_at, completed_hash (a hash of the locked postings, set on completion; see INV-22)
- at most one in_progress per account (partial UNIQUE on account_id WHERE state = 'in_progress')
- UNIQUE (account_id, statement_date) among completed

**review_items**
kind (`near_duplicate`, `manual_match`, `unmatched_transfer`), import_id → imports (the import whose run created the item), import_row_id → import_rows (nullable), posting_id → postings (nullable; the candidate, or the posting that expects a mirror), resolution (nullable: `keep_both`, `skip`, `link`, `no_mirror`, `matched`, `recategorized`), created_by, created_at, resolved_by, resolved_at

Derived, never stored: account balances, report totals, payee suggestions, import counts. They are computed by query from live rows. The login rate-limit state lives in memory.

## 8. Core workflows

Each workflow lists its steps and its acceptance criteria. "Live" means not soft-deleted.

### W1. Setup

1. On first run with no users, every route except `/health` and `/static/*` redirects to `/setup`.
2. The form asks for book name, currency (a select defaulting to CAD), admin username, display name, password twice, and whether to create the starter chart (on by default).
3. Submitting creates the book row, the five roots, the three other system accounts, the optional starter chart, the admin user, and signs them in. Two concurrent submissions are serialized by the writer connection; the second finds a user and gets 404.
4. `/setup` returns 404 from then on. `nage init` does the same from the command line, taking the names as flags and the password from a terminal prompt or `--password-stdin`. `nage user add` creates non-admin users the same way.

Accepts when a fresh data directory reaches the home page in one form submission, with the starter chart when it was requested and the empty state when it was not.

### W2. Accounts

- Create: name, parent, owner (Assets and Liabilities only), optional opening balance and date. Kind is inherited and shown, never chosen. The balance-only flag is on the same form once W11 exists.
- Edit: name, parent (same kind only, not under a descendant, depth at most six), owner, balance-only flag.
- Archive: refused unless every child is archived and the account's live postings sum to zero. Unarchive restores it.
- Opening balance: creates or edits the single opening-balance transaction under the rules of 6.6; an edit carries the transaction's version.

Accepts when the chart can be built to match a household's real accounts without touching the database, and every refusal explains itself.

### W3. Import profile wizard

1. Upload a sample file. Detection proposes, and the user may override: encoding (a UTF-8 BOM means UTF-8; otherwise valid UTF-8 is UTF-8; otherwise Windows-1252; a UTF-16 BOM is refused with a message naming the Excel export to avoid), delimiter (comma, semicolon or tab: the candidate with the most consistent nonzero count outside quotes over the first twenty non-blank lines, ties to comma), and whether the first row is a header (a row with no numeric cells whose shape matches the next row; a file whose first row parses as a date and an amount is headerless). Leading lines to skip and trailing lines to skip are two counts, default zero. Columns are referenced by header name when there is a header and by index otherwise; the header is recorded in the profile and an import whose header differs is refused. Encoding is detected again for every imported file.
2. The user maps columns: date plus format (one of a fixed list of patterns: `YYYY-MM-DD`, `M/D/YYYY`, `D/M/YYYY`, `YYYYMMDD`, `DD-Mon-YYYY`; a datetime keeps its date part; two-digit years are refused; detection offers the patterns that parse every row), amount (one signed column with "positive means money in" or "positive means money out", or separate money-in and money-out columns taken as absolute values; a row with both columns filled is a parse error unless one is zero), description (one or more columns, joined with a space, empty cells skipped), optional payee, optional memo, optional bank transaction id.
3. Number format: decimal separator, thousands separator (comma, period, space or non-breaking space), parentheses as negative, trailing minus, leading plus, an optional CR/DR suffix convention, symbols to strip.
4. Row filters: skip rows where a column equals a value or matches a Rust regex (for pending rows); and an optional account-selector column for exports that hold several accounts in one file (some major banks and credit unions). At import time the user picks the selector value for the account being imported, and rows with any other value are skipped as filtered. Single-account exports leave the selector blank. Filters and the selector are evaluated on raw cell values before any parsing, so filtered rows never need valid dates or amounts.
5. Every row of the sample is validated on save; the preview shows the first ten parsed rows (date, amount, description) and lists the numbers of any rows that fail. The profile cannot be saved while any row fails.
6. Save with a name and an optional default account. Profiles describe a file format, not an account; two accounts at the same bank share one profile. Once a profile has been used by an import, the settings that influence fingerprints (columns, date and number formats, sign conventions, filters, selector) cannot be changed; the user creates a new profile instead. Name and default account stay editable.

Accepts when each fixture in `fixtures/csv/` has a profile that parses every row of its sample exactly as its `expected.json` says.

### W4. Import run

1. Choose a profile, an account (active, non-root, Assets or Liabilities), a file, and, when the profile has an account-selector column, the selector value for this account: the form lists the values found in the file, pre-fills the value this account used last time, and warns when a different one is chosen. The file's SHA-256 is computed; if this account already imported the same hash, the import is refused with 409 and a link to the earlier import.
2. Read the file's structure with the profile (encoding, delimiter, skipped lines, header check). Apply the skip filters and the selector on raw cells; filtered rows are kept raw with outcome `skipped_filtered` and are never parsed or fingerprinted. Parse the surviving rows into date, amount in minor units with positive meaning money into the account, description, payee, memo, bank id. A ragged row, a bad date, a bad amount, or a bank id that appears twice in one file is a parse error; any parse error aborts the whole import with 422 and a list of row numbers and errors, and nothing is written. Rows whose amount is zero get outcome `skipped_filtered`. A file with no surviving rows is refused.
3. Fingerprint each surviving row (fingerprint v1): lowercase hex SHA-256 over the UTF-8 bytes of the fields `nage-fp-v1`, the account id, then either `id` and the trimmed bank id, or `row`, the ISO date, the amount in minor units as a signed decimal, the normalized description and the occurrence index n, all joined by U+001F. n counts earlier surviving rows in the same file with the same date, amount and normalized description. Normalization is NFC, trim, runs of Unicode white space collapsed to one ASCII space, then Unicode uppercase; nothing more. A blank bank id falls back to the row formula. The test suite holds fixed vectors for both formulas.
4. Dry run: run steps 5 and 6 inside a transaction that is rolled back, and show the preview: date range, the number of rows that would be created, matched, skipped as duplicate, filtered and held for review, and a warning listing rows that would be created and are dated on or before the account's opening-balance date. Since the uploaded file is never stored, the confirm form re-posts it together with the preview's SHA-256 and the import refuses a mismatch. The user confirms.
5. Inside one write transaction: insert the import record first (dates and counts are filled at the end), then for each surviving row in file order:
   - a. **Exact duplicate.** A posting on this account, live or deleted, already carries this fingerprint; or an import row on this account from another import carries it with outcome `review` (still pending) or `skipped_duplicate`: outcome `skipped_duplicate`, or `skipped_deleted` when the posting's transaction is soft-deleted at this moment (the outcome records that moment and is not revised on restore), linked to the posting when there is one. Next row.
   - b. **Mirror match.** A live posting on this account has `expects_mirror` set, no import row, the same amount, and a transaction date within four days: link the row to it (row link and fingerprint; the bank date too unless the posting is reconciled, in which case it stays empty), clear `expects_mirror`, resolve any open `unmatched_transfer` item on it as `matched`, outcome `matched_transfer`. If several match, take the closest date, then the oldest. Next row.
   - c. **Manual match candidate.** A live posting on this account from a transaction of origin `manual` has no import row, the same amount and a transaction date within four days: outcome `review`, kind `manual_match`, no posting created. Next row.
   - d. **Near duplicate.** A live import-created posting on this account from a different import whose date range overlaps this file's range has the same amount and a transaction date within three days: outcome `review`, kind `near_duplicate`, no posting created. Next row.
   - e. **Create.** Evaluate rules in priority order; the first whose conditions all match sets the counter account, optionally the payee and tags. No match: counter is `Expenses:Uncategorized`, payee is the row's payee or its description. Create the transaction with origin `import`, the row's date, memo from the row, the account posting (amount, fingerprint, row link, bank date) and the counter posting (negated amount). When the counter account is an asset or liability account, the transfer attach rule of W9 runs first and may attach the row to an existing transaction instead. Outcome `created`.
6. Still inside the transaction: fill the import's date range (over surviving rows) and run the unmatched-transfer sweep: for each live posting on this account with `expects_mirror` set, no open review item, and a transaction date whose four-day window lies entirely within the file's date range, open an `unmatched_transfer` item. Commit.
7. Show the import summary (counts derived from the rows) with links to the review queue, to the created transactions and to the earlier import of the same account. The imports list and an import's row-by-row detail exist for checking a first import; the detail page is the least important screen of the workflow.
8. **Undo.** An import can be undone while none of the postings its rows link to is reconciled, no posting in the transactions it created carries another import's row link, and no row or review item of another import points at those postings. Undo removes outright the transactions it created (edited, categorized, tagged or soft-deleted ones included; the confirmation page shows how many of each, since what is lost is the import's own data), their tags and postings, its rows, the review items it created, and its record; rows with outcome `matched_transfer` or `linked_manual` are unlinked from the postings they attached to (row link, fingerprint and bank date cleared; `expects_mirror` restored for `matched_transfer`). Afterwards the same file can be imported again. When the conditions fail, the page says which import or reconciliation stands in the way; the remedy is to undo later imports first or to delete transactions one by one (W8).

Accepts when importing the same file twice creates nothing the second time, importing an overlapping export creates exactly the new rows, undo followed by re-import reproduces the original result, and every fixture's import summary matches its expected counts.

### W5. Review queue

A single page lists open items, oldest first, with the row's date, amount and description beside the candidate it was compared to.

- `near_duplicate`: **Keep both** re-runs W4 step 5a for the row and, if it still passes, step 5e now; **Skip** sets outcome `skipped_duplicate` linked to the candidate.
- `manual_match`: **Link** re-validates the candidate (live, still without an import row, same amount) and attaches the row to it (row link, fingerprint, and bank date unless reconciled), outcome `linked_manual`; the leg becomes an imported leg and locks. When the candidate is no longer valid the item offers only Keep both and Skip. **Keep both** behaves as above.
- `unmatched_transfer`: **No mirror expected** clears the flag; **Recategorize** opens the transaction, and any edit that clears the flag resolves the item as `recategorized`; **Leave** keeps it open. A later mirror match resolves the item as `matched`.

Resolving an item records who and when; resolving it twice returns 409. A row's outcome changes exactly once, from `review` to its final value.

Accepts when the queue can reach zero for every fixture scenario and each resolution leaves the invariants intact.

### W6. Categorize

- The categorize queue lists live postings against `Expenses:Uncategorized` across all accounts, newest first, filterable by account and owner.
- Each row offers a suggestion: the counter account of the most recent live transaction with the same normalized payee and a categorized counter. Normalized payee is uppercased, split on whitespace and punctuation, tokens containing digits dropped, joined by single spaces; if nothing remains, the uppercased original.
- Actions per row: accept suggestion, pick an account, split, **accept and make a rule** (creates a rule whose condition is `normalized payee equals` the row's normalized payee, with the chosen account), skip.
- Bulk: select several rows with the same suggestion and accept all.
- Rules page: list in priority order, move up and down, enable or disable, delete, edit conditions and actions. Condition fields: description (the import row's description, or the payee for a manual transaction), payee, normalized payee, amount (the bank leg's signed amount, positive meaning money into the account; the form says "in" or "out"), account (the bank leg's account). Operators: contains, equals, starts with, regex (Rust syntax, `(?i)` for case-insensitive), normalized payee equals, amount equals, amount between, account is. Text comparisons are case-insensitive. A rule needs at least one condition. A rule whose target account is archived is disabled automatically and says so. Deleting a tag removes it from every rule's tag list. A preview shows which existing postings a rule would match. **Apply rules to uncategorized** re-runs rules against uncategorized postings only, bumping each changed transaction's version; rules never change a categorized posting. Setting an asset or liability counter by any of these paths follows the transfer attach rule of W9.

Accepts when a month of fixture rows with a fixture rule set ends with the expected categories, and the queue page is usable one-handed on a phone viewport.

### W7. Manual entry and splits

- Quick form with a type switch: Expense (paid from an asset or liability account, for an expense category), Income (received into an asset or liability account, from an income category), Transfer (from an asset or liability account, to another). Fields: date (defaults to today), payee with autocomplete from existing payees, amount, the two accounts, tags, memo.
- Split: add to-lines, each with an account and amount; the form shows the remaining amount and refuses to submit until it is zero.
- The server validates independently of the form and refuses any unbalanced submission with 422.

Accepts when a cash expense can be entered on a phone in under 20 seconds and every submission that reaches the database is balanced.

### W8. Edit, delete, restore

- Any user may edit any live transaction: date, payee, memo, tags, and the counter postings. Opening-balance transactions are edited only through their account's form (6.6). Editing the counter postings of a `balance_update` transaction turns its origin into `manual`; editing its payee, memo or tags does not.
- An imported leg (a posting with an import row) never changes account or amount. A bad parse is fixed by undoing the import (W4 step 8) and re-importing with a corrected profile, or, when undo is refused, by deleting the affected transactions and entering them by hand; a touched import's file cannot be re-imported into the same account.
- Changing a counter posting from an asset or liability account to an income or expense account clears its `expects_mirror` flag and resolves any open `unmatched_transfer` item on it as `recategorized`. Changing it to an asset or liability account follows the transfer attach rule of W9.
- A reconciled posting's account, amount, transaction date and bank date are locked while its reconciliation is completed. The transaction cannot be deleted. Payee, memo and tags remain editable. A posting ticked in an in-progress reconciliation is unticked by any edit of those fields or by a delete.
- Delete is soft and records who and when. Deleted transactions leave every balance and report and appear in a Deleted view with a restore action. Restore is refused while any of the transaction's accounts is archived, and otherwise returns date, payee, memo, tags and postings to their pre-delete values (stamps and version move forward).
- Re-importing a row whose fingerprint belongs to a deleted posting skips it as `skipped_deleted`. Deleting an import's mistake does not invite it back.
- Every edit form checks the version; a stale version returns 409 with the current state.

Accepts when the locks hold through the UI and the HTTP layer, and restore returns a transaction to its pre-delete values.

### W9. Transfers

- A rule whose target is an asset or liability account makes a transfer at import time (W4 step 5e) through the transfer attach rule below. Until milestone 5 exists, rule targets and category picks are limited to income and expense accounts (decision 34).
- Entering a transfer by hand creates both postings with no expectation flags; the next import on either account treats the legs as manual-match candidates (W4 step 5c).
- **Transfer attach rule.** Whenever a posting's counter account is set to an asset or liability account B, by a rule at import, by applying rules, by a suggestion, an inline pick, a bulk accept or an edit: look on B for a live import-created two-posting transaction whose bank leg has the same amount as the counter posting, a transaction date within four days, a counter on `Expenses:Uncategorized`, no reconciled posting, no tags and no open review item. Exactly one candidate: merge automatically. No candidate: set `expects_mirror` on the counter posting. Several candidates, or candidates whose counter is a categorized income or expense account: interactive paths ask the user to pick one or to keep the flag; non-interactive paths (import, apply rules, bulk accept) set the flag.
- **Merge**, in one transaction: remove the candidate's postings and transaction outright (the hard delete of INV-25), re-pointing any other import's `duplicate_of_posting_id` from its bank leg to the surviving counter posting; give the surviving counter posting the candidate row's link, fingerprint and bank date and clear its flag; set that row's outcome to `matched_transfer`; resolve any open `unmatched_transfer` item on the surviving posting as `matched`; bump the surviving transaction's version. The order matters for the unique indexes: the candidate's postings go first.
- Member-to-member: the account pickers for transfers list every household account including other members' accounts, grouped by owner.

Accepts when every ordering of two overlapping imports (A then B, B then A, with and without a rule on each side, with date offsets inside and outside the window) yields exactly one transaction per real transfer, and the merge flow leaves no orphaned rows.

### W10. Reconcile

1. From an account, start a reconciliation with the statement date and the statement's ending balance (entered in natural sign, stored in stored sign). Only one may be in progress per account.
2. The page lists live, unreconciled postings on the account with an effective date (bank date, else transaction date) on or before the statement date, each with a checkbox. It shows the previously reconciled balance, the ticked total, the statement balance, and the difference.
3. Ticking and unticking updates the difference without a page reload.
4. **Complete** is enabled only when the difference is zero. Completing locks the ticked postings.
5. **Abandon** discards an in-progress session and unticks everything; on a reopened reconciliation it returns the reconciliation to completed with its original ticks.
6. Only the most recent completed reconciliation of an account can be reopened, which returns it to in progress.
7. The register shows each posting's status: `open`, `ticked` (in an in-progress reconciliation) or `reconciled`.

Accepts when a seeded account with known statements reconciles to zero at each statement date, and a deliberately missing or duplicated posting shows up as exactly its amount in the difference.

### W11. Balance-only update

Enter a date and the statement balance in natural sign. Nage computes the difference against the account's balance on that date and posts it against `Equity:Valuation Changes` with origin `balance_update`. A zero difference creates nothing.

Accepts when a sequence of updates leaves the account balance equal to the last update on and after its date, and the balance sheet moves by exactly the sum of the differences.

### W12. Reports

All reports take a period or a date, a tag filter where noted, and link every number to the postings behind it.

- **R1 Register** (per account): date, bank date when it differs, payee, counter account (or "split"), amount in natural sign, running balance in natural sign (the account's true balance after each posting over all its postings, computed before any filter is applied), status (from W10). Newest first by default, keyset-paginated on date and id, 100 rows a page. Filters: date range, text (payee, memo and the import row's description), uncategorized only, tag, status. A parent account's register shows its own postings only. Phone layout shows one card per row.
- **R2 Income vs expense** (period, tag filter): two trees, Income and Expenses, children rolled up into parents with a parent's own postings shown as an "Other" line when nonzero, `Expenses:Uncategorized` listed first with the banner from 6.8, net line equal to income minus expenses. The default period is the current month; an empty period shows empty trees. Transfers never appear. Clicking a category opens its postings for the period.
- **R3 Balance sheet** (as-of date): Assets tree, Liabilities tree, net worth equal to assets minus liabilities, then an Equity section showing opening balances, valuation changes, other equity accounts, and retained earnings. A footer line states that assets minus liabilities equals total equity, and the page fails loudly if it does not.
- **R4 Trends** (last N months, default 12, tag filter): one row per top-level category, expandable to children, one column per month, totals row and column. A server-rendered SVG chart of the same numbers; no charting library and no client-side rendering. Each cell links to R2 for that month and category.

Accepts when every report over the seeded fixture book matches its golden snapshot and the cross-report identities in 10.2 hold.

### W13. Tags

Create tags inline from the transaction form; manage names on a tags page; filter R1, R2 and R4 by one tag. Deleting a tag removes it from every transaction and every rule after a confirmation that states the counts.

### W14. Users and settings

- Admin: list users, add, reset password (revokes sessions), deactivate, reactivate, grant or revoke admin. The last admin cannot be deactivated or demoted.
- Self: change password, change display name.
- Settings: book name; the currency, shown read-only; run `check` and show the result; download a backup; export CSV; export JSON.

### W15. Export, backup, check

- **Backup**: `VACUUM INTO` a timestamped file under `backups/`, then stream it for download. Works while the server is running. `nage backup` does the same from the command line. There is no scheduled snapshot inside Nage (decision 37).
- **CSV export**: one line per live posting with transaction id, date, bank date, payee, account full name, account kind, stored amount, natural amount, counter accounts, tags, memo, origin, import file name. Optional date range.
- **JSON export**: every table except sessions and password hashes, wrapped with a schema version. Meant for migrating to other tools; restoring Nage means putting a snapshot back as `nage.sqlite`.
- **Check**: runs every invariant query in section 9, prints violations by ID, exits non-zero on any; "not initialized" with exit 0 on a database with no book row. Available from Settings, from the command line, and at server start.

## 9. Invariants

Each invariant has an ID, a statement, and where it is enforced. "Core" means the Rust core crate refuses the operation. "Schema" means a database constraint or trigger. "Check" means a `nage check` query finds violations after the fact. "Lint" means a compile-time lint. "HTTP" means middleware proven by integration tests. Only invariants marked Check appear in `nage check` output; the others are proven by the tests named in 10.3. Tests reference these IDs.

### Ledger

- **INV-1 Balance.** For every transaction, live or deleted, the sum of its postings' amounts is zero. Core, Check.
- **INV-2 Shape.** Every transaction has at least two postings, and no posting has amount zero. Core, Schema (CHECK), Check.
- **INV-3 Integer money.** Every amount is a 64-bit integer in minor units of the book currency; the core crate contains no floating-point arithmetic. Lint (clippy `float_arithmetic` denied in core), Core.
- **INV-4 Accounting identity.** The sum of all live postings is zero, so at every date stored Assets + Liabilities + Equity + Income + Expenses = 0. Follows from INV-1; verified independently by Check.
- **INV-5 Single currency.** The book currency and its minor digits are set at setup and never change, and no other table carries a currency. Core, Schema.
- **INV-6 Dates.** Every transaction date and bank date is a valid ISO calendar date; periods are half-open. Core, Schema (CHECK on format).
- **INV-7 Version.** Every write to a transaction or its postings increases its version by exactly one, and every edit form names the version it expects. Core.

### Accounts

- **INV-8 Roots.** Exactly five root accounts exist, one per kind, each with its system key, and no posting references a root. Core, Check.
- **INV-9 Kind inheritance.** Every non-root account's kind equals its parent's kind; the tree has no cycles and depth at most six segments including the root. Schema (trigger), Core, Check.
- **INV-10 Names.** Sibling names are unique ignoring case; root names are unique; no name contains a colon. Schema, Core.
- **INV-11 System accounts.** The eight system-keyed accounts exist and are never deleted, archived or re-parented, and the roots are never renamed. Core, Check.
- **INV-12 Archive.** An archived account's live postings sum to zero, every child of an archived account is archived, and no transaction touching it was created after its archive time. Core, Check.
- **INV-13 Opening balance.** Each account has at most one live transaction of origin `opening_balance`, with exactly two nonzero postings: the account (an Assets or Liabilities account) and `Equity:Opening Balances`. Core, Check.
- **INV-14 Balance update.** Every transaction of origin `balance_update` has exactly two postings, one of them on `Equity:Valuation Changes`. Core, Check.

### Imports

- **INV-15 Atomic import.** An import record exists only with all of its rows and their outcomes; (account, file hash) is unique. Core (one SQL transaction), Schema.
- **INV-16 Row outcome.** Every import row has exactly one outcome. Rows with outcome `created`, `matched_transfer` or `linked_manual` are referenced through `postings.import_row_id` by exactly one posting, on the import's account. Rows with any other outcome are referenced by no posting. Schema (UNIQUE import_row_id), Check.
- **INV-17 Fingerprint uniqueness.** Among all postings on an account, live or deleted, fingerprints are unique, and every posting with an import row has a fingerprint equal to its row's. Schema (partial UNIQUE), Check.
- **INV-18 Fingerprint determinism.** A row's fingerprint depends only on account, bank id or (date, amount, normalized description, occurrence index among surviving rows). Permuting a file's rows leaves the multiset of fingerprints unchanged. Core, property test.
- **INV-19 Imported leg immutability.** A posting with an import row never changes account or amount. Core.
- **INV-20 Expectation flags.** `expects_mirror` is set only on postings in asset or liability accounts that have no import row. Core, Check.
- **INV-21 Review items.** An open review item references either an import row with outcome `review` or a posting with `expects_mirror` set. A resolved item has a resolution, a resolver and a time, and its row's outcome is final. Core, Check.

### Reconciliation

- **INV-22 Locked postings.** A posting linked to a completed reconciliation keeps its account, amount, transaction date and bank date, and its transaction is not deleted. Core, Check (compares against the hash captured at completion).
- **INV-23 Statement equality.** For every completed reconciliation of an account, the sum of amounts of postings linked to it or to any earlier completed reconciliation of that account equals its statement balance. Core at completion, Check.
- **INV-24 Ordering.** Completed reconciliations of an account have strictly increasing statement dates, at most one is in progress, and only the latest completed one can be reopened. Schema, Core.

### Lifecycle and access

- **INV-25 Soft delete.** Deleted transactions and their postings are excluded from every balance, report, reconciliation candidate list, suggestion and queue, and are restorable to their pre-delete values. Hard deletion occurs only through the undo of an import (W4 step 8) and the transfer merge (W9), and never removes a transaction with a reconciled posting. Core, Check.
- **INV-26 Stamps.** Every row in accounts, transactions, tags, rules, import_profiles, imports, reconciliations and review_items names the user who created it (as `created_by`, `imported_by` or `started_by`) and, when changed, who changed it. Schema (NOT NULL, FK), Check.
- **INV-27 Sessions.** Every request outside `/setup`, `/login`, `/health` and `/static/*` carries a valid, unexpired session for an active user; `/setup` is unavailable once any user exists. HTTP.
- **INV-28 No partial writes.** Every mutating HTTP request runs in one database transaction and either fully succeeds or leaves no trace. Core.

## 10. Verification

### 10.1 Test layers

| Layer | Tooling | What it proves |
|-------|---------|----------------|
| Unit | `cargo test` in the core crate | Money parsing and formatting, date arithmetic, normalization functions, fingerprint vectors, rule operators, sign conventions |
| Property | `proptest` | Invariants under random inputs: balanced transactions round-trip, unbalanced ones are refused, fingerprints are permutation-stable, reconciliation sums; arbitrary bytes and mutated fixtures never panic the parser |
| Golden | `insta` snapshots over `fixtures/` | CSV parsing per bank profile, rule application, import summaries, report models over the seeded book |
| Integrity | `nage check` | Every Check-marked invariant against test databases after each scenario, and against deliberately corrupted databases to prove each query fires |
| HTTP | axum test client with a temporary SQLite file per test | Routes, auth, locks, 409 on stale versions, form validation, Origin checks |
| End-to-end | Playwright against the built binary: `webkit-phone` (iPhone 13 profile, 390×844) and `chromium-desktop` (1280×800), each project with its own server and fresh data directory, a setup project that runs `/setup` and saves the signed-in state | The must-nail workflow and the phone layout; Node is a test dependency only |
| Migration | Fixture databases from each released version | Migrations apply cleanly and the result passes `check` |
| Performance | Seeded 100k-posting book, measured in CI | Register under 200 ms, each report under 1 s, 5k-row import under 5 s on the CI runner |

Report snapshots are taken of the report's data model, not its HTML, so template changes do not churn goldens. Scenario databases are built through core operations only, never through raw SQL, so every scenario is also a `check` subject.

### 10.2 Cross-report identities

Tested on the seeded book and on every fixture scenario:
- Balance sheet: assets minus liabilities equals opening balances plus valuation changes plus other equity plus retained earnings (natural signs). INV-4.
- Change in net worth between two dates equals net income for the period plus valuation changes plus opening balances dated in the period.
- Trends: the sum of a category's monthly cells equals its R2 total for the same span.
- Register: the final running balance equals the account's balance sheet line on the same date.
- CSV export: line count equals the live posting count, and per-account sums equal the balances.

### 10.3 Per-feature verification

| Feature | Verified by |
|---------|-------------|
| Money and dates (6.10) | Unit: parse and format every supported number format; property: `parse(format(x)) == x` (INV-5, INV-6); lint denies floats in core (INV-3) |
| Chart of accounts (W2) | Property: random trees never violate INV-8 to INV-10; HTTP: move across kinds returns 422; archive with balance or unarchived children returns 422; integrity: INV-11, INV-12 |
| Transactions and splits (W7) | Property: random balanced transactions persist and reload equal; unbalanced refused (INV-1, INV-2); HTTP: split form with nonzero remainder returns 422; e2e: phone entry |
| Opening balances (W2) | Scenario: second opening balance refused, zero refused, income account refused (INV-13); edit carries the version; import preview warns on rows before the date |
| Balance-only updates (W11) | Scenario: a sequence of updates leaves the balance equal to the last update; shape per INV-14; identity tests in 10.2 |
| CSV profiles and parsing (W3) | Golden per fixture directory; unit per date, sign and number convention; property: arbitrary bytes never panic and either parse or report row errors; HTTP: a profile with a failing row cannot be saved |
| Fingerprints and duplicates (W4) | Unit: fixed vectors for both formulas; property: import twice creates zero (INV-15, INV-17); import a superset creates exactly the extras; permutation stability (INV-18); same hash refused with 409 |
| Undo (W4 step 8) | Scenario: undo of an untouched import frees fingerprints and hash and a re-import reproduces it; undo after categorization lists the count and succeeds; undo refused by a reconciled posting, by a later import's link, and by a later import's duplicate row; matched and linked rows are unlinked with the flag restored (INV-25) |
| Rules and suggestions (W6) | Unit per operator including signed amounts and normalized payee; golden: fixture rows plus fixture rules yield expected categories; property: first match wins and is deterministic; archived target disables the rule; tag deletion edits the rule |
| Transfer matching and merge (W9) | Scenario matrix: both import orders, a rule on neither, one or both sides, offsets of 0 to 5 days, equal and unequal amounts, member-to-member, merge with one, several and no candidates; integrity after each step (INV-16, INV-17, INV-20) |
| Review queue (W5) | State-machine tests over every kind and resolution, including a stale manual-match candidate and auto-resolution by a later match (INV-21); HTTP: resolving twice returns 409 |
| Edit, delete, restore (W8) | HTTP: imported-leg edit returns 422 (INV-19); reconciled edit returns 422 (INV-22); stale version returns 409 (INV-7); restore round-trip equality and refusal on an archived account (INV-25); flag clearing on recategorization (INV-20) |
| Reconciliation (W10) | Scenario: statements over a seeded account complete at zero; missing and duplicated postings show as their amounts; abandon after reopen returns to completed; an edit unticks; property: INV-23 after every completion; reopen rules (INV-24) |
| Reports (W12) | Golden snapshots of each report model; identities in 10.2 (INV-4); HTTP: tag filter changes totals as expected; SVG chart snapshot |
| Tags (W13) | HTTP: add, remove, filter; delete confirmation states the counts and edits rules |
| Auth and users (W14) | HTTP: unauthenticated redirect on every route and HX-Redirect on htmx requests (INV-27); wrong password; the lockout engages on the sixth failure with 429; expired session; Origin mismatch returns 403; admin-only routes return 403; last admin cannot be demoted; cookie flags present; stamps on every created row (INV-26) |
| Concurrency (5.4) | Two concurrent edits: one succeeds, one gets 409 (INV-7); two parallel imports into different accounts both commit; the busy timeout returns 503; a failed request leaves no rows (INV-28) |
| Export and backup (W15) | Backup file opens and passes `check`; CSV identities in 10.2; JSON export round-trips its row counts |
| `nage check` | Corrupt a copy of a test database by raw SQL once per Check-marked invariant and assert the matching ID is reported; a fresh database reports "not initialized" |
| Migrations (5.5) | Fresh database migrates; each released fixture database migrates and passes `check`; the pre-migration backup is written only when migrations are pending; an older binary refuses a newer database with the message |
| Phone layout (G6) | e2e in `webkit-phone`: home, login, setup, categorize queue, register, quick entry and reconcile have no page-level horizontal scroll and every interactive element outside running text is at least 44×44 CSS px; every other screen only has no page-level horizontal scroll; the quick-entry flow completes |
| Deployment (5.2) | CI builds the binary and the image, starts the container with an empty volume, runs `nage health` inside it, and from milestone 2 runs the e2e setup project against the container |

### 10.4 CI gates

A change merges only when all of these pass: formatting, clippy with the core float lint, `cargo check` on all targets, the sqlx offline query check, the `cargo deny` license check, unit and property tests, golden tests, HTTP tests, integrity checks over every scenario database, migration tests, the fixture guard, the image build with its container smoke, and the e2e suite. The advisory check runs on a schedule and opens an issue rather than failing PRs. Performance is recorded on every run; the absolute budgets in 10.1 are a v1 release gate measured in milestone 8, and after v1 a run fails when it regresses by more than 50 percent against the previous release.

### 10.5 v1 release checklist (manual)

1. Fresh install on the household server from the published image and from the binary.
2. One import profile per household bank, built from real exports, each saved as an anonymized fixture.
3. One full month imported for every account, categorized to zero uncategorized, review queue at zero.
4. Every account reconciled to its statement.
5. R2 and R3 checked by hand against the statements for that month.
6. A second household member completes the categorize queue from a phone without instructions.
7. Backup downloaded, restored into a scratch data directory, `check` passes.

## 11. Architecture and repository layout

```
nage/
  Cargo.toml              workspace
  crates/
    nage-core/            money, dates, chart, ledger operations, storage (sqlx), import engine
                          (profiles, parsing, fingerprints, rules, matching), reconciliation,
                          reports, integrity checks. No HTTP, no HTML, no floats.
    nage-web/             axum app: routes, auth middleware, askama templates, htmx partials,
                          embedded static assets (htmx, Pico CSS, app.css) served at
                          content-hashed URLs, server-rendered SVG charts.
    nage/                 the binary: serve, health, init, user, check, backup, export, migrate.
    xtask/                development tooling: seed scenario databases, generate fixtures.
  migrations/             sqlx migrations, forward only, frozen once released.
  fixtures/
    csv/<bank>-<kind>/    sample.csv, profile.json, expected.json, rules.json (optional)
    books/                seeded scenario databases, one per released version
  e2e/                    Playwright tests (Node, test-time only)
  SPEC.md  PLAN.md  README.md  LICENSE (AGPL-3.0)
```

Design rules:
- Every ledger operation is a function in `nage-core` that takes a database transaction and the acting user. The web layer calls it; tests call it directly. There is no second code path.
- The web layer renders HTML on the server. htmx handles inline updates (categorize, tick a reconciliation row, split editor). No bundler, no Node at build time, no client-side state beyond the page.
- Queries are checked at compile time by sqlx with the offline data committed; filters, search and pagination may use the query builder.
- Report queries return typed models that both the templates and the golden tests consume.
- Suggested crates: axum, sqlx (sqlite, runtime-tokio), askama, argon2, jiff (dates and the configured time zone, with the bundled time-zone database), csv, encoding_rs, unicode-normalization, regex, sha2, serde, clap, rust-embed, insta, proptest.

## 12. Open questions

Answered on 2026-10-26 and recorded above: the household's banks and the fixture plan, the currency (CAD), the hosting (Docker Compose on the household server from milestone 2), the register sort (newest first), and the opening-balance dating (the day before the earliest imported row; importing older history later means re-dating and re-amounting it, which the import preview's warning points at).

Still open:

1. Repository visibility. The plan assumes a public repository, as befits an open-source project: fixtures are anonymized by rule and scanned by the guard, and the image on GHCR is public. A private repository would need a registry credential on the household server and changes nothing else.

## 13. Glossary

- **Account**: a node in the chart of accounts. Bank accounts, cards, loans and categories are all accounts.
- **Posting**: one signed amount against one account, inside a transaction.
- **Transaction**: a dated set of postings that sum to zero.
- **Transfer**: a transaction whose postings all land in asset or liability accounts.
- **Counter posting**: the posting opposite the bank leg in an imported transaction; the "category".
- **Bank leg**: the posting on the account an import row belongs to.
- **Fingerprint**: the deterministic identity of an import row within an account, used to skip duplicates.
- **Mirror**: the other side of a transfer as it appears in the other account's export.
- **Natural sign**: the sign a person expects for an account kind; owed balances positive, income positive.
- **Stored sign**: the internal convention where asset and expense increases are positive.
- **Effective date**: bank date when present, otherwise the transaction date; used only by reconciliation.
- **Surviving row**: an import row that passed the skip filters and the selector and is therefore parsed and fingerprinted.
