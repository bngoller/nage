//! The schema of migration 0001: what the `book` table's constraints accept and refuse, and
//! that every table is STRICT (decision 56; the Schema half of INV-5). The rows are written
//! with unchecked SQL through `Storage::begin_write`, as decision 57 allows for schema tests.

mod common;

use nage_core::storage::{Storage, WriteTransaction};
use sqlx::{AssertSqlSafe, Row as _};

use common::{open_migrated, primary_code, sqlite_code};

/// `SQLITE_CONSTRAINT`, the primary result code of every constraint failure.
const CONSTRAINT: i32 = 19;
/// `SQLITE_CONSTRAINT_CHECK`.
const CHECK: i32 = 275;
/// `SQLITE_CONSTRAINT_NOTNULL`.
const NOT_NULL: i32 = 1299;
/// `SQLITE_CONSTRAINT_PRIMARYKEY`.
const PRIMARY_KEY: i32 = 1555;
/// `SQLITE_CONSTRAINT_DATATYPE`, a STRICT table refusing a value its column type cannot
/// hold losslessly.
const DATATYPE: i32 = 3091;

/// The SQL literals of one `book` row, one per column.
#[derive(Clone, Copy)]
struct BookRow {
    id: &'static str,
    name: &'static str,
    currency: &'static str,
    minor_digits: &'static str,
    created_at: &'static str,
}

/// A row every constraint accepts.
const VALID: BookRow = BookRow {
    id: "1",
    name: "'Home'",
    currency: "'EUR'",
    minor_digits: "2",
    created_at: "'2026-10-28T12:00:00Z'",
};

impl BookRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO book (id, name, currency, minor_digits, created_at) \
             VALUES ({}, {}, {}, {}, {})",
            self.id, self.name, self.currency, self.minor_digits, self.created_at
        )
    }
}

/// Begins a write transaction, naming `label` on failure.
async fn begin(storage: &Storage, label: &str) -> WriteTransaction {
    storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction for {label}: {error}"))
}

/// Inserts `row` inside `tx`.
async fn insert(tx: &mut WriteTransaction, row: BookRow) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe(row.insert()))
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Rolls `tx` back, naming `label` on failure.
async fn roll_back(tx: WriteTransaction, label: &str) {
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back after {label}: {error}"));
}

/// `book` accepts one valid row, storing a lossless `'2'` or `2.0` for `minor_digits` as the
/// integer 2, and refuses with the named SQLite constraint code: an `id` other than 1, a
/// second row, `minor_digits` outside 0 to 3 or not losslessly an integer, a currency that
/// is not three capital letters, a `created_at` that is not an RFC 3339 UTC whole-second
/// timestamp naming a real instant, and NULL in any column but `id` (a NULL `id` is rowid 1).
///
/// Cites: decision 56, INV-5 (schema half), SPEC 5.4 (timestamp format).
#[tokio::test]
async fn book_accepts_one_valid_row_and_refuses_the_rest() {
    let (_dir, storage) = open_migrated().await;

    let accepted = [
        ("the valid row", VALID),
        (
            "minor_digits as the text '2'",
            BookRow {
                minor_digits: "'2'",
                ..VALID
            },
        ),
        (
            "minor_digits as the real 2.0",
            BookRow {
                minor_digits: "2.0",
                ..VALID
            },
        ),
    ];
    for (label, row) in accepted {
        let mut tx = begin(&storage, label).await;
        insert(&mut tx, row)
            .await
            .unwrap_or_else(|error| panic!("book refused {label}: {error}"));
        let stored =
            sqlx::query("SELECT typeof(minor_digits), minor_digits FROM book WHERE id = 1")
                .fetch_one(&mut *tx)
                .await
                .unwrap_or_else(|error| panic!("cannot read back {label}: {error}"));
        let kind = stored
            .try_get_unchecked::<String, _>(0)
            .unwrap_or_else(|error| panic!("typeof(minor_digits) after {label}: {error}"));
        let value = stored
            .try_get_unchecked::<i64, _>(1)
            .unwrap_or_else(|error| panic!("minor_digits after {label}: {error}"));
        assert_eq!(
            (kind.as_str(), value),
            ("integer", 2),
            "minor_digits stored for {label}"
        );
        roll_back(tx, label).await;
    }

    let refused = [
        ("id = 2", BookRow { id: "2", ..VALID }, CHECK),
        (
            "minor_digits = 4",
            BookRow {
                minor_digits: "4",
                ..VALID
            },
            CHECK,
        ),
        (
            "minor_digits = -1",
            BookRow {
                minor_digits: "-1",
                ..VALID
            },
            CHECK,
        ),
        (
            "minor_digits = 1.5",
            BookRow {
                minor_digits: "1.5",
                ..VALID
            },
            DATATYPE,
        ),
        (
            "minor_digits = 'two'",
            BookRow {
                minor_digits: "'two'",
                ..VALID
            },
            DATATYPE,
        ),
        (
            "currency = 'cad'",
            BookRow {
                currency: "'cad'",
                ..VALID
            },
            CHECK,
        ),
        (
            "currency = 'CA'",
            BookRow {
                currency: "'CA'",
                ..VALID
            },
            CHECK,
        ),
        (
            "currency = 'CAD1'",
            BookRow {
                currency: "'CAD1'",
                ..VALID
            },
            CHECK,
        ),
        (
            "created_at = '2026-02-30T00:00:00Z' (no such day)",
            BookRow {
                created_at: "'2026-02-30T00:00:00Z'",
                ..VALID
            },
            CHECK,
        ),
        (
            "created_at = '2026-10-28 12:00:00' (no T, no Z)",
            BookRow {
                created_at: "'2026-10-28 12:00:00'",
                ..VALID
            },
            CHECK,
        ),
        (
            "created_at = '2026-10-28T12:00:00.5Z' (fractional second)",
            BookRow {
                created_at: "'2026-10-28T12:00:00.5Z'",
                ..VALID
            },
            CHECK,
        ),
        (
            "created_at = '2026-10-28T12:00:00+00:00' (offset instead of Z)",
            BookRow {
                created_at: "'2026-10-28T12:00:00+00:00'",
                ..VALID
            },
            CHECK,
        ),
        (
            "created_at = '2026-10-28T12:00:60Z' (no such second)",
            BookRow {
                created_at: "'2026-10-28T12:00:60Z'",
                ..VALID
            },
            CHECK,
        ),
        (
            "name = NULL",
            BookRow {
                name: "NULL",
                ..VALID
            },
            NOT_NULL,
        ),
        (
            "currency = NULL",
            BookRow {
                currency: "NULL",
                ..VALID
            },
            NOT_NULL,
        ),
        (
            "minor_digits = NULL",
            BookRow {
                minor_digits: "NULL",
                ..VALID
            },
            NOT_NULL,
        ),
        (
            "created_at = NULL",
            BookRow {
                created_at: "NULL",
                ..VALID
            },
            NOT_NULL,
        ),
    ];
    for (label, row, expected) in refused {
        let mut tx = begin(&storage, label).await;
        let error = match insert(&mut tx, row).await {
            Ok(()) => panic!("book accepted {label}"),
            Err(error) => error,
        };
        let code = sqlite_code(&error)
            .unwrap_or_else(|| panic!("book refused {label} without a SQLite code: {error}"));
        assert_eq!(
            primary_code(code),
            CONSTRAINT,
            "primary result code for {label} (extended {code}): {error}"
        );
        assert_eq!(code, expected, "extended result code for {label}: {error}");
        roll_back(tx, label).await;
    }

    let mut tx = begin(&storage, "the committed valid row").await;
    insert(&mut tx, VALID)
        .await
        .unwrap_or_else(|error| panic!("book refused the valid row: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the valid row: {error}"));

    let label = "a second row with id = 1";
    let mut tx = begin(&storage, label).await;
    let error = match insert(&mut tx, VALID).await {
        Ok(()) => panic!("book accepted {label}"),
        Err(error) => error,
    };
    let code = sqlite_code(&error)
        .unwrap_or_else(|| panic!("book refused {label} without a SQLite code: {error}"));
    assert_eq!(
        code, PRIMARY_KEY,
        "extended result code for {label}: {error}"
    );
    roll_back(tx, label).await;

    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books after the inserts: {error}"));
    assert_eq!(count, 1, "book rows after one committed insert");

    storage.close().await;
}

/// Every table the migrations create is STRICT: `pragma_table_list` lists `book` and no
/// non-strict table besides SQLite's own and sqlx's `_sqlx_migrations`, which decision 57
/// exempts.
///
/// Cites: decision 56.
#[tokio::test]
async fn every_table_is_strict() {
    let (_dir, storage) = open_migrated().await;
    let rows = sqlx::query(
        "SELECT name, strict FROM pragma_table_list \
         WHERE schema = 'main' AND type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'",
    )
    .fetch_all(storage.reader())
    .await
    .unwrap_or_else(|error| panic!("cannot list the tables: {error}"));

    let mut tables = Vec::with_capacity(rows.len());
    for row in &rows {
        let name = row
            .try_get_unchecked::<String, _>("name")
            .unwrap_or_else(|error| panic!("pragma_table_list.name: {error}"));
        let strict = row
            .try_get_unchecked::<i64, _>("strict")
            .unwrap_or_else(|error| panic!("pragma_table_list.strict of {name}: {error}"));
        tables.push((name, strict));
    }
    assert!(
        tables.iter().any(|(name, _)| name == "book"),
        "pragma_table_list does not list book: {tables:?}"
    );
    for (name, strict) in &tables {
        assert_eq!(*strict, 1, "table {name} is not STRICT");
    }

    storage.close().await;
}
