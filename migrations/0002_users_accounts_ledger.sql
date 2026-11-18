-- Milestone 2 slice 2: the users, sessions, accounts, transactions and postings tables (SPEC 7,
-- decisions 56 and 92), without the columns of later milestones: postings gains its import
-- columns and the bank date in schema 0003, the tag tables are milestone 4's and the
-- reconciliation link is milestone 6's.
--
-- sqlx checksums this file's exact bytes, comments included, and refuses to start against a
-- database whose record of migration 2 differs, so the file is frozen once v0.2.0 is tagged
-- (SPEC 5.5).
--
-- Conventions (decisions 56 and 92): every table is STRICT; ids are INTEGER PRIMARY KEY, with
-- AUTOINCREMENT on the ledger tables whose rows are hard-deleted (transactions and postings), so a
-- deleted row's id is never given to a new one; booleans are INTEGER 0 or 1 with a CHECK; the
-- kind and origin columns carry their full value sets; a date is YYYY-MM-DD and a timestamp
-- RFC 3339 UTC with whole seconds, each with a GLOB for the shape plus a round trip through
-- date() or strftime(), both written only from Rust, never from SQLite defaults (SPEC 5.4,
-- decision 92); every foreign key is ON DELETE RESTRICT; every unique rule is a named index; the
-- stamps that name a user are NOT NULL except users.created_by (the first admin has none) and the
-- pairs that are set together (a user's updated and disabled stamps, a transaction's deleted
-- stamps). The folded name columns hold the fold core computes (decision 93) and no CHECK
-- reads them; the content rules (a name's length, trimming and colon, owners and the
-- balance-only flag on Assets and Liabilities only) are core's, and the eight system keys, depth,
-- cycles and the count of roots are core's and nage check's.

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL,
    username_folded TEXT NOT NULL,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL CHECK (is_admin IN (0, 1)),
    created_by INTEGER REFERENCES users (id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (
        created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+0 seconds') IS created_at
    ),
    updated_by INTEGER REFERENCES users (id) ON DELETE RESTRICT,
    updated_at TEXT CHECK (
        updated_at IS NULL
        OR (
            updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
            AND strftime('%Y-%m-%dT%H:%M:%SZ', updated_at, '+0 seconds') IS updated_at
        )
    ),
    disabled_by INTEGER REFERENCES users (id) ON DELETE RESTRICT,
    disabled_at TEXT CHECK (
        disabled_at IS NULL
        OR (
            disabled_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
            AND strftime('%Y-%m-%dT%H:%M:%SZ', disabled_at, '+0 seconds') IS disabled_at
        )
    ),
    CHECK ((updated_by IS NULL) = (updated_at IS NULL)),
    CHECK ((disabled_by IS NULL) = (disabled_at IS NULL))
) STRICT;

CREATE UNIQUE INDEX users_username_folded ON users (username_folded);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    token_hash TEXT NOT NULL CHECK (length(token_hash) = 64 AND token_hash NOT GLOB '*[^0-9a-f]*'),
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (
        created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+0 seconds') IS created_at
    ),
    last_seen_at TEXT NOT NULL CHECK (
        last_seen_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', last_seen_at, '+0 seconds') IS last_seen_at
    ),
    expires_at TEXT NOT NULL CHECK (
        expires_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', expires_at, '+0 seconds') IS expires_at
    )
) STRICT;

CREATE UNIQUE INDEX sessions_token_hash ON sessions (token_hash);

CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_folded TEXT NOT NULL,
    parent_id INTEGER REFERENCES accounts (id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('asset', 'liability', 'equity', 'income', 'expense')),
    system_key TEXT,
    owner_user_id INTEGER REFERENCES users (id) ON DELETE RESTRICT,
    is_balance_only INTEGER NOT NULL CHECK (is_balance_only IN (0, 1)),
    archived_at TEXT CHECK (
        archived_at IS NULL
        OR (
            archived_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
            AND strftime('%Y-%m-%dT%H:%M:%SZ', archived_at, '+0 seconds') IS archived_at
        )
    ),
    created_by INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (
        created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+0 seconds') IS created_at
    ),
    updated_by INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    updated_at TEXT NOT NULL CHECK (
        updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', updated_at, '+0 seconds') IS updated_at
    ),
    CHECK (parent_id IS NOT id)
) STRICT;

CREATE UNIQUE INDEX accounts_parent_name_folded ON accounts (parent_id, name_folded);
CREATE UNIQUE INDEX accounts_root_name_folded ON accounts (name_folded) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX accounts_system_key ON accounts (system_key) WHERE system_key IS NOT NULL;

-- INV-9's Schema half: a child's kind equals its parent's when it is created and when it is
-- moved, and a kind never changes. The WHEN compares with <>, so a parent that does not exist
-- (the subquery is NULL) is left to the foreign key's refusal.
CREATE TRIGGER accounts_kind_matches_parent_on_insert
BEFORE INSERT ON accounts
WHEN NEW.parent_id IS NOT NULL
    AND NEW.kind <> (SELECT kind FROM accounts WHERE id = NEW.parent_id)
BEGIN
    SELECT RAISE(ABORT, 'accounts.kind must equal the parent''s kind');
END;

CREATE TRIGGER accounts_kind_matches_parent_on_move
BEFORE UPDATE OF parent_id ON accounts
WHEN NEW.parent_id IS NOT NULL
    AND NEW.kind <> (SELECT kind FROM accounts WHERE id = NEW.parent_id)
BEGIN
    SELECT RAISE(ABORT, 'accounts.kind must equal the parent''s kind');
END;

CREATE TRIGGER accounts_kind_never_changes
BEFORE UPDATE OF kind ON accounts
WHEN NEW.kind <> OLD.kind
BEGIN
    SELECT RAISE(ABORT, 'accounts.kind never changes');
END;

CREATE TABLE transactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL CHECK (
        date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND date(date, '+0 days') IS date
    ),
    payee TEXT NOT NULL,
    memo TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('manual', 'import', 'opening_balance', 'balance_update')),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_by INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (
        created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+0 seconds') IS created_at
    ),
    updated_by INTEGER NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    updated_at TEXT NOT NULL CHECK (
        updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', updated_at, '+0 seconds') IS updated_at
    ),
    deleted_by INTEGER REFERENCES users (id) ON DELETE RESTRICT,
    deleted_at TEXT CHECK (
        deleted_at IS NULL
        OR (
            deleted_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
            AND strftime('%Y-%m-%dT%H:%M:%SZ', deleted_at, '+0 seconds') IS deleted_at
        )
    ),
    CHECK ((deleted_by IS NULL) = (deleted_at IS NULL))
) STRICT;

CREATE INDEX transactions_date_id ON transactions (date, id);
CREATE INDEX transactions_live_date_id ON transactions (date, id) WHERE deleted_at IS NULL;

CREATE TABLE postings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL REFERENCES transactions (id) ON DELETE RESTRICT,
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT,
    amount INTEGER NOT NULL CHECK (amount <> 0)
) STRICT;

CREATE INDEX postings_account_transaction ON postings (account_id, transaction_id);
