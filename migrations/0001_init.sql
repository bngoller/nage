-- Milestone 1 slice 3: the first migration, creating the `book` table (SPEC 7, decision 56).
--
-- sqlx checksums this file's exact bytes, comments included, and refuses to start against a
-- database whose record of migration 1 differs, so the file is frozen once v0.1.0 is tagged
-- (SPEC 5.5). Until then an edit needs the dev database and the fixture regenerated.
--
-- Conventions (decision 56): every table is STRICT; timestamps are RFC 3339 UTC with whole
-- seconds, written only from Rust, never from SQLite defaults (SPEC 5.4); every date and
-- timestamp column carries a format CHECK, a GLOB for the shape plus a round trip through
-- date() or strftime(), which rejects what the GLOB alone accepts (a 30th of February, a
-- 60th second). INV-5's "set once" is enforced by core; the schema enforces the singleton
-- row, the shapes and NOT NULL.

CREATE TABLE book (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL,
    currency TEXT NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    minor_digits INTEGER NOT NULL CHECK (minor_digits BETWEEN 0 AND 3),
    created_at TEXT NOT NULL CHECK (
        created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z'
        AND strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+0 seconds') IS created_at
    )
) STRICT;
