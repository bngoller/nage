//! The SQLite column forms of the core's values on real tables (SPEC 7, decisions 57, 89 and
//! 98): `Money` as INTEGER, `Date`, `Timestamp`, `Kind` and `SystemKey` as TEXT read back
//! strictly, the ids as INTEGER. The rows are written with unchecked SQL and `bind` and stay
//! the tests' own (decision 98), since nothing here may depend on setup.

mod common;

use nage_core::accounts::{AccountId, Kind, SystemKey};
use nage_core::date::Date;
use nage_core::money::Money;
use nage_core::test_support::open_migrated;
use nage_core::timestamp::Timestamp;
use nage_core::users::UserId;
use sqlx::{AssertSqlSafe, Row as _, SqliteConnection};

use common::{NOW_TEXT, begin, commit, now, raw_roots, roll_back, stamp};

/// Inserts a transaction dated `date` (a bound `Date`) created at `created_at` by user 1 and
/// returns its id.
async fn insert_transaction(conn: &mut SqliteConnection, date: Date, created_at: Timestamp) -> i64 {
    sqlx::query(
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES (?1, '', '', 'manual', 1, 1, ?2, 1, ?2, NULL, NULL)",
    )
    .bind(date)
    .bind(created_at)
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|error| panic!("cannot insert a transaction: {error}"))
    .last_insert_rowid()
}

/// Inserts a posting of `amount` (a bound `Money`) on `account` in `transaction` and returns
/// its id.
async fn insert_posting(
    conn: &mut SqliteConnection,
    transaction: i64,
    account: AccountId,
    amount: Money,
) -> i64 {
    sqlx::query("INSERT INTO postings (transaction_id, account_id, amount) VALUES (?1, ?2, ?3)")
        .bind(transaction)
        .bind(account)
        .bind(amount)
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("cannot insert a posting: {error}"))
        .last_insert_rowid()
}

/// Reads one value of type `T` with the unchecked `query_scalar`, whose `try_get` checks the
/// column's compatibility with `T` and then decodes.
async fn scalar<T>(conn: &mut SqliteConnection, sql: &str, id: i64) -> Result<T, sqlx::Error>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    sqlx::query_scalar::<_, T>(AssertSqlSafe(sql.to_owned()))
        .bind(id)
        .fetch_one(&mut *conn)
        .await
}

/// `Money` binds as the INTEGER of its minor units and reads back from `postings.amount`, at
/// the extremes of `i64` included; a NULL read under `Option<Money>` is `None`; a bound
/// `Money` reaches SQLite as an integer (`typeof` of the parameter; a STRICT TEXT column would
/// convert it to text rather than refuse it, so the storage class is what proves the form).
///
/// Proves: decision 98 (Money's column form), INV-3.
#[tokio::test]
async fn money_binds_as_an_integer_and_reads_back() {
    let (_dir, storage) = open_migrated().await;
    raw_roots(&storage).await;
    let mut tx = begin(&storage, "money").await;
    let transaction = insert_transaction(&mut tx, Date::MIN, now()).await;
    for minor in [1, -1, 12_345, i64::MAX, i64::MIN] {
        let id = insert_posting(&mut tx, transaction, AccountId(1), Money::from_minor(minor)).await;
        let read: Money = scalar(&mut tx, "SELECT amount FROM postings WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read posting {id}: {error}"));
        assert_eq!(
            read,
            Money::from_minor(minor),
            "the amount {minor} read back"
        );
        let typeof_amount: String = scalar(
            &mut tx,
            "SELECT typeof(amount) FROM postings WHERE id = ?1",
            id,
        )
        .await
        .unwrap_or_else(|error| panic!("cannot read typeof for {id}: {error}"));
        assert_eq!(
            typeof_amount, "integer",
            "the storage class of amount {minor}"
        );
    }
    let none: Option<Money> = scalar(&mut tx, "SELECT NULL FROM postings WHERE id = ?1", 1)
        .await
        .unwrap_or_else(|error| panic!("cannot read a NULL as Option<Money>: {error}"));
    assert_eq!(none, None, "a NULL under Option<Money>");
    let bound: String = sqlx::query_scalar("SELECT typeof(?1)")
        .bind(Money::from_minor(5))
        .fetch_one(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the storage class of a bound Money: {error}"));
    assert_eq!(bound, "integer", "the storage class of a bound Money");
    roll_back(tx, "money").await;
    storage.close().await;
}

/// `Date` binds as its `YYYY-MM-DD` text and reads back from `transactions.date`; a stored text
/// the schema accepts but `Date` refuses (a year before 1900) is a decode error naming the
/// range, never a wrong date; a text of another shape (a date followed by a time, read from a
/// literal, since the schema's CHECK refuses it in the column) is a decode error naming the
/// form.
///
/// Proves: decision 98 (Date's column form), INV-6 (Core half on read-back).
#[tokio::test]
async fn dates_bind_as_text_and_read_back_strictly() {
    let (_dir, storage) = open_migrated().await;
    raw_roots(&storage).await;
    let mut tx = begin(&storage, "dates").await;
    for text in ["1900-01-01", "2026-11-09", "2199-12-31", "2024-02-29"] {
        let date: Date = text
            .parse()
            .unwrap_or_else(|error| panic!("{text}: {error}"));
        let id = insert_transaction(&mut tx, date, now()).await;
        let stored: String = scalar(&mut tx, "SELECT date FROM transactions WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read the text of {id}: {error}"));
        assert_eq!(stored, text, "the stored text of {text}");
        let read: Date = scalar(&mut tx, "SELECT date FROM transactions WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read {id} as a Date: {error}"));
        assert_eq!(read, date, "the date {text} read back");
    }
    let early = sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES ('0001-01-01', '', '', 'manual', 1, 1, '{NOW_TEXT}', 1, '{NOW_TEXT}', NULL, NULL)"
    )))
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("the schema refused 0001-01-01: {error}"))
    .last_insert_rowid();
    let error = match scalar::<Date>(
        &mut tx,
        "SELECT date FROM transactions WHERE id = ?1",
        early,
    )
    .await
    {
        Ok(date) => panic!("0001-01-01 read back as the date {date}"),
        Err(error) => error,
    };
    assert!(
        matches!(error, sqlx::Error::ColumnDecode { .. }),
        "0001-01-01 failed otherwise than as a column decode: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("outside 1900-01-01 to 2199-12-31"),
        "the decode error does not name the range: {error}"
    );
    let error = match scalar::<Date>(
        &mut tx,
        "SELECT '2026-11-09T00:00:00' FROM transactions WHERE id = ?1",
        early,
    )
    .await
    {
        Ok(date) => panic!("the text 2026-11-09T00:00:00 read back as the date {date}"),
        Err(error) => error,
    };
    assert!(
        matches!(error, sqlx::Error::ColumnDecode { .. }),
        "a date with a time failed otherwise than as a column decode: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("not a date of the form YYYY-MM-DD"),
        "the decode error does not name the form: {error}"
    );
    roll_back(tx, "dates").await;
    storage.close().await;
}

/// `Timestamp` binds as its `YYYY-MM-DDTHH:MM:SSZ` text and reads back from `users.created_at`;
/// a stored text the schema accepts but the type refuses (a year before 2020) is a decode error
/// naming the range; a text of another shape (a fraction of a second, read from a literal, since
/// the schema's CHECK refuses it in the column) is a decode error naming the form.
///
/// Proves: decision 98 (the timestamp's column form), SPEC 5.4.
#[tokio::test]
async fn timestamps_bind_as_text_and_read_back_strictly() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "timestamps").await;
    for (name, second) in [
        ("a", 1_577_836_800),
        ("b", 1_600_000_000),
        ("c", 1_794_225_600),
        ("d", 7_258_118_399),
    ] {
        let instant = stamp(second);
        let id = sqlx::query(
            "INSERT INTO users (username, username_folded, display_name, password_hash, is_admin, created_by, created_at, updated_by, updated_at, disabled_by, disabled_at) VALUES (?1, ?1, ?1, 'hash', 1, NULL, ?2, NULL, NULL, NULL, NULL)",
        )
        .bind(name)
        .bind(instant)
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot insert user {name}: {error}"))
        .last_insert_rowid();
        let stored: String = scalar(&mut tx, "SELECT created_at FROM users WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read the text of user {id}: {error}"));
        assert_eq!(
            stored,
            instant.to_string(),
            "the stored text of second {second}"
        );
        let read: Timestamp = scalar(&mut tx, "SELECT created_at FROM users WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read user {id}'s stamp: {error}"));
        assert_eq!(read, instant, "the stamp of second {second} read back");
    }
    let early = sqlx::query(
        "INSERT INTO users (username, username_folded, display_name, password_hash, is_admin, created_by, created_at, updated_by, updated_at, disabled_by, disabled_at) VALUES ('e', 'e', 'E', 'hash', 1, NULL, '2019-12-31T23:59:59Z', NULL, NULL, NULL, NULL)",
    )
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("the schema refused 2019-12-31T23:59:59Z: {error}"))
    .last_insert_rowid();
    let error =
        match scalar::<Timestamp>(&mut tx, "SELECT created_at FROM users WHERE id = ?1", early)
            .await
        {
            Ok(instant) => panic!("2019-12-31T23:59:59Z read back as {instant}"),
            Err(error) => error,
        };
    assert!(
        matches!(error, sqlx::Error::ColumnDecode { .. }),
        "the early stamp failed otherwise than as a column decode: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("outside 2020-01-01T00:00:00Z to 2199-12-31T23:59:59Z"),
        "the decode error does not name the range: {error}"
    );
    let error = match scalar::<Timestamp>(
        &mut tx,
        "SELECT '2026-11-09T12:00:00.5Z' FROM users WHERE id = ?1",
        early,
    )
    .await
    {
        Ok(instant) => panic!("the text 2026-11-09T12:00:00.5Z read back as {instant}"),
        Err(error) => error,
    };
    assert!(
        matches!(error, sqlx::Error::ColumnDecode { .. }),
        "a fractional stamp failed otherwise than as a column decode: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("not a timestamp of the form YYYY-MM-DDTHH:MM:SSZ"),
        "the decode error does not name the form: {error}"
    );
    roll_back(tx, "timestamps").await;
    storage.close().await;
}

/// `Kind` and `SystemKey` bind as their texts and read back from the five raw roots (the kinds
/// singular, the keys plural); a key text that is not one of the eight (`expense`, the likeliest
/// typo for `expenses`) is a decode error naming it, and so is a kind text that is not one of
/// the five.
///
/// Proves: decision 98 (the kind's and key's column forms), decision 92 (the spellings).
#[tokio::test]
async fn kinds_and_system_keys_bind_and_read_back_and_unknown_text_is_refused() {
    let (_dir, storage) = open_migrated().await;
    raw_roots(&storage).await;
    let mut tx = begin(&storage, "kinds").await;
    for (id, kind) in Kind::ALL.iter().enumerate() {
        let id = i64::try_from(id).unwrap_or_else(|error| panic!("{error}")) + 1;
        let read: Kind = scalar(&mut tx, "SELECT kind FROM accounts WHERE id = ?1", id)
            .await
            .unwrap_or_else(|error| panic!("cannot read the kind of {id}: {error}"));
        assert_eq!(read, *kind, "the kind of root {id}");
        let key: Option<SystemKey> =
            scalar(&mut tx, "SELECT system_key FROM accounts WHERE id = ?1", id)
                .await
                .unwrap_or_else(|error| panic!("cannot read the key of {id}: {error}"));
        assert_eq!(key, Some(kind.root_key()), "the key of root {id}");
    }
    let bound = sqlx::query(
        "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('Opening Balances', 'opening balances', 3, ?1, ?2, NULL, 0, NULL, 1, ?3, 1, ?3)",
    )
    .bind(Kind::Equity)
    .bind(SystemKey::OpeningBalances)
    .bind(now())
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("cannot insert a bound kind and key: {error}"))
    .last_insert_rowid();
    let texts = sqlx::query("SELECT kind, system_key FROM accounts WHERE id = ?1")
        .bind(bound)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read {bound}: {error}"));
    let kind_text: String = texts
        .try_get_unchecked(0)
        .unwrap_or_else(|error| panic!("the kind text: {error}"));
    let key_text: String = texts
        .try_get_unchecked(1)
        .unwrap_or_else(|error| panic!("the key text: {error}"));
    assert_eq!(
        (kind_text.as_str(), key_text.as_str()),
        ("equity", "opening_balances")
    );
    let typo = sqlx::query(
        "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('Probe', 'probe', 5, 'expense', 'expense', NULL, 0, NULL, 1, ?1, 1, ?1)",
    )
    .bind(now())
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("cannot insert the probe: {error}"))
    .last_insert_rowid();
    let error = match scalar::<SystemKey>(
        &mut tx,
        "SELECT system_key FROM accounts WHERE id = ?1",
        typo,
    )
    .await
    {
        Ok(key) => panic!("the key text expense read back as {key}"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("unknown system key \"expense\""),
        "the decode error does not name the text: {error}"
    );
    let error =
        match scalar::<Kind>(&mut tx, "SELECT 'expenses' FROM accounts WHERE id = ?1", 5).await {
            Ok(kind) => panic!("the kind text expenses read back as {kind}"),
            Err(error) => error,
        };
    assert!(
        error
            .to_string()
            .contains("unknown account kind \"expenses\""),
        "the decode error does not name the text: {error}"
    );
    roll_back(tx, "kinds").await;
    storage.close().await;
}

/// `UserId` and `AccountId` bind and read back as the INTEGER ids they wrap.
///
/// Proves: decision 98 (the ids' column forms).
#[tokio::test]
async fn ids_bind_and_read_back_as_integers() {
    let (_dir, storage) = open_migrated().await;
    raw_roots(&storage).await;
    let mut tx = begin(&storage, "ids").await;
    let owner: UserId = scalar(&mut tx, "SELECT id FROM users WHERE id = ?1", 1)
        .await
        .unwrap_or_else(|error| panic!("cannot read user 1 as a UserId: {error}"));
    assert_eq!(owner, UserId(1));
    let root: AccountId = scalar(&mut tx, "SELECT id FROM accounts WHERE id = ?1", 5)
        .await
        .unwrap_or_else(|error| panic!("cannot read account 5 as an AccountId: {error}"));
    assert_eq!(root, AccountId(5));
    let child = sqlx::query(
        "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('Checking', 'checking', ?1, 'asset', NULL, ?2, 0, NULL, ?2, ?3, ?2, ?3)",
    )
    .bind(AccountId(1))
    .bind(UserId(1))
    .bind(now())
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("cannot insert the child: {error}"))
    .last_insert_rowid();
    let parent: Option<AccountId> = scalar(
        &mut tx,
        "SELECT parent_id FROM accounts WHERE id = ?1",
        child,
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the parent of {child}: {error}"));
    assert_eq!(parent, Some(AccountId(1)));
    let owner: Option<UserId> = scalar(
        &mut tx,
        "SELECT owner_user_id FROM accounts WHERE id = ?1",
        child,
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the owner of {child}: {error}"));
    assert_eq!(owner, Some(UserId(1)));
    let no_owner: Option<UserId> = scalar(
        &mut tx,
        "SELECT owner_user_id FROM accounts WHERE id = ?1",
        1,
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the owner of root 1: {error}"));
    assert_eq!(no_owner, None);
    commit(tx, "ids").await;
    storage.close().await;
}
