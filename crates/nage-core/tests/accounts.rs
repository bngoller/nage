//! The chart of accounts (SPEC 6.2, W2, the Core halves of INV-9 to INV-11 and the tree halves of
//! INV-8 and INV-12, decisions 95 and 96): create, rename, move, owner, archive, unarchive, their
//! refusals and their stamps, the reads by id, key and full name. Every database test works inside
//! one write transaction on the standard setup and rolls it back; the live postings a test needs
//! are written by `create_transaction` (decision 104), and a deleted transaction is still
//! planted with unchecked SQL, until the soft delete of milestone 4 slice 2 (decision 98).

mod common;

use nage_core::accounts::{
    self, Account, AccountBoxes, AccountError, AccountId, Action, BoxName, ChangeError, Changed,
    Clash, FormRefusals, Kind, NewAccount, Refused, Sent, SentBoxes, SystemKey, Word,
    archive_account, change_account, create_account, create_from_form, move_account,
    rename_account, set_owner, unarchive_account,
};
use nage_core::fold::NameError;
use nage_core::money::{MinorDigits, Money};
use nage_core::storage::WriteTransaction;
use nage_core::test_support;
use nage_core::users::UserId;
use sqlx::AssertSqlSafe;

use common::{
    LATER_SECOND, LATER_TEXT, NOW_TEXT, account_named, assert_sound, begin, insert_transaction,
    later, member, now, post, refusal, roll_back, seeded, stamp,
};

/// Asserts that `result` is a refusal that `expected` accepts: a match on the variant and its
/// data, never on the `Debug` text.
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(
    result: Result<T, AccountError>,
    expected: impl Fn(&AccountError) -> bool,
    label: &str,
) {
    match result {
        Ok(value) => panic!("{label} was accepted: {value:?}"),
        Err(error) => assert!(expected(&error), "{label}: refused as {error:?}"),
    }
}

/// Whether `error` is a refusal of the name rules for `name_error`.
fn name_refused(error: &AccountError, name_error: NameError) -> bool {
    matches!(error, AccountError::Name { error, .. } if *error == name_error)
}

/// Whether `error` is the refusal of a name a sibling already has.
fn name_taken(error: &AccountError) -> bool {
    matches!(error, AccountError::NameTaken { .. })
}

/// A new account named `name` under `parent`, joint.
fn new(name: &str, parent: AccountId) -> NewAccount {
    NewAccount {
        name: name.to_owned(),
        parent,
        owner: None,
    }
}

/// Creates `name` under `parent` by `by` at `later`, panicking on a refusal.
async fn create(tx: &mut WriteTransaction, by: UserId, name: &str, parent: AccountId) -> Account {
    create_account(tx, by, later(), &new(name, parent))
        .await
        .unwrap_or_else(|error| panic!("cannot create {name}: {error}"))
}

/// `create_account` stores the normalized name under the parent with the parent's kind, no
/// owner, the balance-only flag off, no archive stamp and no key, created and updated by the
/// actor (a member, not the admin who set the book up) at the instant; full names join the
/// path; a chain reaches six segments and no more; the same name may live under two different
/// parents; the reads by id answer the row or `None`.
///
/// Proves: SPEC 6.2 (kinds inherited, depth at most six), W2, INV-9 (Core half: kind and
/// depth), INV-26 (Core half: the created and updated pairs name the acting user), decisions 92
/// and 96.
#[tokio::test]
async fn create_account_inherits_the_kind_checks_the_depth_and_stamps_the_row() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "create").await;
    let editor = member(&mut tx, admin, "editor").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let food = create_account(&mut tx, editor, later(), &new("  Food ", expenses))
        .await
        .unwrap_or_else(|error| panic!("cannot create Food: {error}"));
    assert_eq!(food.name, "Food", "the name is normalized");
    assert_eq!(food.kind, Kind::Expense, "the kind is the parent's");
    assert_eq!(food.parent_id, Some(expenses));
    assert_eq!(food.owner_user_id, None);
    assert!(!food.is_balance_only);
    assert_eq!(food.archived_at, None);
    assert_eq!(food.system_key, None);
    assert!(!food.is_root() && !food.is_system() && !food.is_archived());
    assert_eq!((food.created_by, food.created_at), (editor, later()));
    assert_eq!((food.updated_by, food.updated_at), (editor, later()));
    let full = accounts::full_name(&mut tx, food.id)
        .await
        .unwrap_or_else(|error| panic!("cannot name Food: {error}"));
    assert_eq!(full, "Expenses:Food");

    let mut parent = food.id;
    for name in ["Three", "Four", "Five", "Six"] {
        parent = create(&mut tx, admin, name, parent).await.id;
    }
    let deepest = accounts::full_name(&mut tx, parent)
        .await
        .unwrap_or_else(|error| panic!("cannot name the deepest account: {error}"));
    assert_eq!(deepest, "Expenses:Food:Three:Four:Five:Six");
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Seven", parent)).await,
        |error| {
            matches!(
                error,
                AccountError::TooDeep {
                    depth: 7,
                    inside: false,
                    ..
                }
            )
        },
        "a seventh segment",
    );

    let income = account_named(&mut tx, "Income").await;
    let income_food = create(&mut tx, admin, "Food", income).await;
    assert_eq!(income_food.kind, Kind::Income);
    let children = accounts::children(&mut tx, expenses)
        .await
        .unwrap_or_else(|error| panic!("cannot read the children of Expenses: {error}"));
    assert!(
        children.iter().any(|child| child.id == food.id),
        "Food is not among the children of Expenses"
    );
    let read = accounts::account(&mut tx, food.id)
        .await
        .unwrap_or_else(|error| panic!("cannot read Food: {error}"));
    assert_eq!(read.as_ref(), Some(&food));
    let missing = accounts::account(&mut tx, AccountId(999))
        .await
        .unwrap_or_else(|error| panic!("cannot look up account 999: {error}"));
    assert_eq!(missing, None);
    roll_back(tx, "create").await;
    storage.close().await;
}

/// `create_account` refuses an empty, blank, over-long or colon-bearing name (a fullwidth colon
/// included), a control character, and a name a sibling already has ignoring case, white space,
/// compatibility characters and archive state; the stored name is the normalized one, so a
/// later exact duplicate of it is refused too.
///
/// Proves: SPEC 6.2 (the name rules), INV-10 (Core half), decision 95.
#[tokio::test]
async fn create_account_refuses_bad_names_and_duplicates() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "names").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let long = "x".repeat(65);
    for (name, expected, label) in [
        ("", Some(NameError::Empty), "an empty name"),
        ("   ", Some(NameError::Empty), "a blank name"),
        (
            long.as_str(),
            Some(NameError::TooLong { chars: 65 }),
            "a 65-character name",
        ),
        ("Food:Fruit", Some(NameError::Colon), "a colon"),
        (
            "Food\u{FF1A}Fruit",
            Some(NameError::Colon),
            "a fullwidth colon",
        ),
        ("Food\tFruit", Some(NameError::Control), "a tab"),
        ("groceries", None, "a sibling's name in lowercase"),
        ("GROCERIES", None, "a sibling's name in capitals"),
        (
            "\u{FF27}roceries",
            None,
            "a sibling's name with a fullwidth letter",
        ),
        (
            "\u{210D}ousing",
            None,
            "a sibling's name with a double-struck capital",
        ),
        (
            " Groceries ",
            None,
            "a sibling's name with white space around it",
        ),
    ] {
        assert_refused(
            create_account(&mut tx, admin, later(), &new(name, expenses)).await,
            |error| match expected {
                Some(name_error) => name_refused(error, name_error),
                None => name_taken(error),
            },
            label,
        );
    }
    let dining_out = create(&mut tx, admin, "Dining  Out", expenses).await;
    assert_eq!(dining_out.name, "Dining Out");
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Dining Out", expenses)).await,
        name_taken,
        "the collapsed twin",
    );
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    archive_account(&mut tx, admin, later(), gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Gifts: {error}"));
    assert_refused(
        create_account(&mut tx, admin, later(), &new("gifts", expenses)).await,
        name_taken,
        "an archived sibling's name",
    );
    roll_back(tx, "names").await;
    storage.close().await;
}

/// `create_account` needs an existing, live parent and allows an owner only under Assets or
/// Liabilities and only for an existing user, a deactivated one included.
///
/// Proves: SPEC 6.2 (owners on Assets and Liabilities only), 5.1 (ownership is a label),
/// INV-12 (Core half: nothing live under an archived parent), decision 96.
#[tokio::test]
async fn create_account_checks_the_parent_and_the_owner() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "parents and owners").await;
    let assets = account_named(&mut tx, "Assets").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let equity = account_named(&mut tx, "Equity").await;
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Orphan", AccountId(999))).await,
        |error| matches!(error, AccountError::PlaceNotListed { .. }),
        "an unknown parent",
    );
    let travel = account_named(&mut tx, "Expenses:Travel").await;
    archive_account(&mut tx, admin, later(), travel)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Travel: {error}"));
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Flights", travel)).await,
        |error| {
            matches!(
                error,
                AccountError::ParentArchived { parent, .. } if parent == "Expenses:Travel"
            )
        },
        "an archived parent",
    );
    for (parent, label) in [(expenses, "an expense"), (equity, "an equity account")] {
        let with_owner = NewAccount {
            owner: Some(admin),
            ..new("Owned", parent)
        };
        assert_refused(
            create_account(&mut tx, admin, later(), &with_owner).await,
            |error| matches!(error, AccountError::OwnerNotAllowed { .. }),
            &format!("an owner on {label}"),
        );
    }
    let unknown_owner = NewAccount {
        owner: Some(UserId(99)),
        ..new("Owned", assets)
    };
    assert_refused(
        create_account(&mut tx, admin, later(), &unknown_owner).await,
        |error| matches!(error, AccountError::OwnerNotListed { .. }),
        "an unknown owner",
    );
    let owned = create_account(
        &mut tx,
        admin,
        later(),
        &NewAccount {
            owner: Some(admin),
            ..new("Admin's Checking", assets)
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create an owned account: {error}"));
    assert_eq!(owned.owner_user_id, Some(admin));
    let retired = member(&mut tx, admin, "retired").await;
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE users SET disabled_by = {}, disabled_at = '{LATER_TEXT}' WHERE id = {}",
        admin.0, retired.0
    )))
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("cannot deactivate the member: {error}"));
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let owned_by_retired = create_account(
        &mut tx,
        admin,
        later(),
        &NewAccount {
            owner: Some(retired),
            ..new("Old Card", liabilities)
        },
    )
    .await
    .unwrap_or_else(|error| panic!("a deactivated owner was refused: {error}"));
    assert_eq!(owned_by_retired.owner_user_id, Some(retired));
    roll_back(tx, "parents and owners").await;
    storage.close().await;
}

/// `rename_account` stores the normalized name and the actor's stamp, allows a change of case
/// alone and the rename of a renamable system account (Uncategorized, which stays found by its
/// key), renames an archived account, and refuses a root, a sibling's name ignoring case, a bad
/// name and an unknown account.
///
/// Proves: SPEC 6.2 (roots never renamed, a non-root system account renamable), INV-10 (Core
/// half), INV-11 (Core half: roots keep their names), W2, decision 96.
#[tokio::test]
async fn rename_account_keeps_roots_and_siblings_apart() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "rename").await;
    let editor = member(&mut tx, admin, "editor").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let renamed = rename_account(&mut tx, editor, later(), groceries, "  Food  Shopping ")
        .await
        .unwrap_or_else(|error| panic!("cannot rename Groceries: {error}"));
    assert_eq!(renamed.name, "Food Shopping");
    assert_eq!((renamed.updated_by, renamed.updated_at), (editor, later()));
    assert_eq!((renamed.created_by, renamed.created_at), (admin, now()));
    let shouted = rename_account(&mut tx, editor, later(), groceries, "FOOD SHOPPING")
        .await
        .unwrap_or_else(|error| panic!("a change of case alone was refused: {error}"));
    assert_eq!(shouted.name, "FOOD SHOPPING");
    let expenses = account_named(&mut tx, "Expenses").await;
    assert_refused(
        rename_account(&mut tx, editor, later(), expenses, "Spending").await,
        |error| matches!(error, AccountError::Section { .. }),
        "renaming a root",
    );
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    let unsorted = rename_account(&mut tx, editor, later(), uncategorized, "Unsorted")
        .await
        .unwrap_or_else(|error| panic!("cannot rename Uncategorized: {error}"));
    assert_eq!(unsorted.name, "Unsorted");
    assert_eq!(unsorted.system_key, Some(SystemKey::Uncategorized));
    let found = accounts::system_account(&mut tx, SystemKey::Uncategorized)
        .await
        .unwrap_or_else(|error| panic!("the renamed system account is lost: {error}"));
    assert_eq!(found.id, uncategorized);
    let full = accounts::full_name(&mut tx, uncategorized)
        .await
        .unwrap_or_else(|error| panic!("cannot name the renamed account: {error}"));
    assert_eq!(full, "Expenses:Unsorted");
    let dining = account_named(&mut tx, "Expenses:Dining").await;
    for (name, expected, label) in [
        ("Housing", None, "a sibling's name"),
        ("housing", None, "a sibling's name in lowercase"),
        ("", Some(NameError::Empty), "an empty name"),
        ("a:b", Some(NameError::Colon), "a colon"),
    ] {
        assert_refused(
            rename_account(&mut tx, editor, later(), dining, name).await,
            |error| match expected {
                Some(name_error) => name_refused(error, name_error),
                None => name_taken(error),
            },
            label,
        );
    }
    assert_refused(
        rename_account(&mut tx, editor, later(), AccountId(999), "Ghost").await,
        |error| matches!(error, AccountError::NoSuchAccount),
        "an unknown account",
    );
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    archive_account(&mut tx, admin, later(), gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Gifts: {error}"));
    let presents = rename_account(&mut tx, editor, later(), gifts, "Presents")
        .await
        .unwrap_or_else(|error| panic!("renaming an archived account was refused: {error}"));
    assert_eq!(presents.name, "Presents");
    assert!(presents.is_archived());
    roll_back(tx, "rename").await;
    storage.close().await;
}

/// `move_account` re-parents within the kind, re-roots the full name and stamps the row with
/// the actor (a member, not the row's creator) and the instant, the created pair staying the
/// creator's; a move under the current parent succeeds and changes no parent; an archived
/// account moves under a live parent and stays archived; it refuses a root, a system account,
/// the account itself or a descendant as the new parent, a parent of another kind, an archived
/// parent, a move that would take the subtree past six segments, a new sibling with the same
/// name, and unknown ids.
///
/// Proves: SPEC 6.2 (moves within the kind, never under a descendant, never past six), INV-8
/// (Core half: roots stay roots), INV-9 (Core half), INV-11 (Core half: system accounts never
/// re-parented), INV-12 (Core half: nothing live under an archived parent), INV-26 (Core half:
/// the updated pair names the acting user), W2, decision 96 (an archived account may be moved).
#[tokio::test]
async fn move_account_checks_kind_descendants_depth_and_names() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "move").await;
    let editor = member(&mut tx, admin, "editor").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let income = account_named(&mut tx, "Income").await;
    let food = create(&mut tx, admin, "Food", expenses).await.id;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let moved = move_account(&mut tx, editor, later(), groceries, food)
        .await
        .unwrap_or_else(|error| panic!("cannot move Groceries under Food: {error}"));
    assert_eq!(moved.parent_id, Some(food));
    assert_eq!(moved.kind, Kind::Expense);
    assert_eq!((moved.updated_by, moved.updated_at), (editor, later()));
    assert_eq!((moved.created_by, moved.created_at), (admin, now()));
    let full = accounts::full_name(&mut tx, groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot name the moved account: {error}"));
    assert_eq!(full, "Expenses:Food:Groceries");

    assert_refused(
        move_account(&mut tx, admin, later(), food, groceries).await,
        |error| {
            matches!(
                error,
                AccountError::MoveUnderDescendant { account, target, .. }
                    if account == "Expenses:Food" && target == "Expenses:Food:Groceries"
            )
        },
        "a move under a child",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), food, food).await,
        |error| {
            matches!(
                error,
                AccountError::MoveUnderDescendant { account, target, .. } if account == target
            )
        },
        "a move under itself",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), groceries, income).await,
        |error| {
            matches!(
                error,
                AccountError::KindMismatch { word: Word::Category, section, .. } if section == "Income"
            )
        },
        "a move to another kind",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), expenses, income).await,
        |error| matches!(error, AccountError::Section { .. }),
        "moving a root",
    );
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    assert_refused(
        move_account(&mut tx, admin, later(), uncategorized, food).await,
        |error| matches!(error, AccountError::BuiltIn { .. }),
        "moving a system account",
    );
    let travel = account_named(&mut tx, "Expenses:Travel").await;
    archive_account(&mut tx, admin, later(), travel)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Travel: {error}"));
    let dining = account_named(&mut tx, "Expenses:Dining").await;
    let stayed = move_account(&mut tx, admin, later(), dining, expenses)
        .await
        .unwrap_or_else(|error| panic!("a move under the current parent was refused: {error}"));
    assert_eq!(
        stayed.parent_id,
        Some(expenses),
        "a move under the current parent changes no parent"
    );
    assert_refused(
        move_account(&mut tx, admin, later(), dining, travel).await,
        |error| matches!(error, AccountError::ParentArchived { .. }),
        "a move under an archived parent",
    );
    let fees = account_named(&mut tx, "Expenses:Fees").await;
    archive_account(&mut tx, admin, later(), fees)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Fees: {error}"));
    let moved_fees = move_account(&mut tx, admin, later(), fees, food)
        .await
        .unwrap_or_else(|error| panic!("moving an archived account was refused: {error}"));
    assert_eq!(moved_fees.parent_id, Some(food));
    assert!(moved_fees.is_archived());
    let mut parent = expenses;
    let mut chain = Vec::new();
    for name in ["Two", "Three", "Four", "Five"] {
        parent = create(&mut tx, admin, name, parent).await.id;
        chain.push(parent);
    }
    assert_refused(
        move_account(&mut tx, admin, later(), food, chain[3]).await,
        |error| {
            matches!(
                error,
                AccountError::TooDeep {
                    depth: 7,
                    inside: true,
                    ..
                }
            )
        },
        "a two-level subtree under a fifth-level parent",
    );
    let under_four = move_account(&mut tx, admin, later(), food, chain[2])
        .await
        .unwrap_or_else(|error| panic!("a move to the sixth level was refused: {error}"));
    assert_eq!(under_four.parent_id, Some(chain[2]));
    let deepest = accounts::full_name(&mut tx, groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot name Groceries: {error}"));
    assert_eq!(deepest, "Expenses:Two:Three:Four:Food:Groceries");
    assert_refused(
        move_account(&mut tx, admin, later(), dining, AccountId(999)).await,
        |error| matches!(error, AccountError::PlaceNotListed { .. }),
        "an unknown new parent",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), AccountId(999), food).await,
        |error| matches!(error, AccountError::NoSuchAccount),
        "moving an unknown account",
    );
    create(&mut tx, admin, "Dining", food).await;
    assert_refused(
        move_account(&mut tx, admin, later(), dining, food).await,
        name_taken,
        "a move beside a same-named sibling",
    );
    roll_back(tx, "move").await;
    storage.close().await;
}

/// `set_owner` sets and clears the owner of an Assets or Liabilities account and stamps the
/// row with the actor (a member, neither the row's creator nor the new owner) and the instant,
/// the created pair staying the creator's; it gives an archived account an owner, the account
/// staying archived; it refuses an income account, a root, a system account, an unknown user
/// and an unknown account.
///
/// Proves: SPEC 6.2 (owners on Assets and Liabilities only), 5.1, INV-26 (Core half: the
/// updated pair names the acting user), W2, decision 96 (an archived account may be re-owned).
#[tokio::test]
async fn set_owner_is_for_assets_and_liabilities_only() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "owners").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let owned = set_owner(&mut tx, editor, later(), checking, Some(admin))
        .await
        .unwrap_or_else(|error| panic!("cannot set the owner of Checking: {error}"));
    assert_eq!(owned.owner_user_id, Some(admin));
    assert_eq!((owned.updated_by, owned.updated_at), (editor, later()));
    assert_eq!((owned.created_by, owned.created_at), (admin, now()));
    let joint = set_owner(&mut tx, admin, later(), checking, None)
        .await
        .unwrap_or_else(|error| panic!("cannot clear the owner of Checking: {error}"));
    assert_eq!(joint.owner_user_id, None);
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let owned_card = set_owner(&mut tx, admin, later(), card, Some(admin))
        .await
        .unwrap_or_else(|error| panic!("cannot set the owner of the card: {error}"));
    assert_eq!(owned_card.owner_user_id, Some(admin));
    let savings = account_named(&mut tx, "Assets:Savings").await;
    archive_account(&mut tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Savings: {error}"));
    let owned_savings = set_owner(&mut tx, admin, later(), savings, Some(admin))
        .await
        .unwrap_or_else(|error| panic!("re-owning an archived account was refused: {error}"));
    assert_eq!(owned_savings.owner_user_id, Some(admin));
    assert!(owned_savings.is_archived());
    let salary = account_named(&mut tx, "Income:Salary").await;
    let assets = account_named(&mut tx, "Assets").await;
    let opening = account_named(&mut tx, "Equity:Opening Balances").await;
    for (account, label) in [(salary, "an income account"), (opening, "a system account")] {
        assert_refused(
            set_owner(&mut tx, admin, later(), account, Some(admin)).await,
            |error| matches!(error, AccountError::OwnerNotAllowed { .. }),
            &format!("an owner on {label}"),
        );
    }
    assert_refused(
        set_owner(&mut tx, admin, later(), assets, Some(admin)).await,
        |error| {
            matches!(
                error,
                AccountError::Section { refused } if refused.place.as_deref() == Some("Assets")
            )
        },
        "an owner on a root",
    );
    assert_refused(
        set_owner(&mut tx, admin, later(), checking, Some(UserId(99))).await,
        |error| matches!(error, AccountError::OwnerNotListed { .. }),
        "an unknown owner",
    );
    assert_refused(
        set_owner(&mut tx, admin, later(), AccountId(999), Some(admin)).await,
        |error| matches!(error, AccountError::NoSuchAccount),
        "an unknown account",
    );
    roll_back(tx, "owners").await;
    storage.close().await;
}

/// `archive_account` stamps the archive instant and the actor (a member, not the row's creator)
/// and refuses a second archive, a live child, a system account or root, a nonzero balance over
/// live postings (a deleted transaction's postings do not count, and two postings that cancel
/// out do); `unarchive_account` clears the stamp, writes the updated pair of its own actor (the
/// member again, where the admin archived) and instant (a third one) and refuses a live account
/// and a child whose parent is archived until the parent is unarchived first.
///
/// Proves: SPEC 6.2 (archive: children first, zero balance; unarchive with a live parent),
/// INV-11 (Core half: system accounts never archived), INV-12 (Core half: the archive and
/// unarchive rules), INV-25 (deleted postings excluded), INV-26 (Core half: the updated pair
/// names the acting user), W2, decisions 96 and 104.
#[tokio::test]
async fn archive_and_unarchive_follow_the_tree_and_the_balance() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "archive").await;
    let editor = member(&mut tx, admin, "editor").await;
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    let archived = archive_account(&mut tx, editor, later(), gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Gifts: {error}"));
    assert_eq!(archived.archived_at, Some(later()));
    assert!(archived.is_archived());
    assert_eq!(
        (archived.updated_by, archived.updated_at),
        (editor, later())
    );
    assert_eq!((archived.created_by, archived.created_at), (admin, now()));
    assert_refused(
        archive_account(&mut tx, admin, later(), gifts).await,
        |error| matches!(error, AccountError::AlreadyArchived { account } if account == "Expenses:Gifts"),
        "a second archive",
    );
    let expenses = account_named(&mut tx, "Expenses").await;
    let food = create(&mut tx, admin, "Food", expenses).await.id;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    move_account(&mut tx, admin, later(), groceries, food)
        .await
        .unwrap_or_else(|error| panic!("cannot move Groceries under Food: {error}"));
    assert_refused(
        archive_account(&mut tx, admin, later(), food).await,
        |error| {
            matches!(
                error,
                AccountError::ChildrenNotArchived { account, child }
                    if account == "Expenses:Food" && child == "Expenses:Food:Groceries"
            )
        },
        "a parent with a live child",
    );
    archive_account(&mut tx, admin, later(), groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Groceries: {error}"));
    archive_account(&mut tx, admin, later(), food)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Food after its child: {error}"));
    assert_refused(
        unarchive_account(&mut tx, admin, later(), groceries).await,
        |error| {
            matches!(
                error,
                AccountError::ParentArchivedUnarchiveFirst { parent, .. } if parent == "Expenses:Food"
            )
        },
        "unarchiving a child under an archived parent",
    );
    let after = stamp(LATER_SECOND + 60);
    let food_again = unarchive_account(&mut tx, editor, after, food)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive Food: {error}"));
    assert_eq!(food_again.archived_at, None);
    assert_eq!(
        (food_again.updated_by, food_again.updated_at),
        (editor, after)
    );
    assert_eq!(
        (food_again.created_by, food_again.created_at),
        (admin, later())
    );
    let groceries_again = unarchive_account(&mut tx, admin, later(), groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive Groceries: {error}"));
    assert!(!groceries_again.is_archived());
    let dining = account_named(&mut tx, "Expenses:Dining").await;
    assert_refused(
        unarchive_account(&mut tx, admin, later(), dining).await,
        |error| matches!(error, AccountError::NotArchived { .. }),
        "unarchiving a live account",
    );
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    assert_refused(
        archive_account(&mut tx, admin, later(), expenses).await,
        |error| matches!(error, AccountError::Section { .. }),
        "archiving a root",
    );
    assert_refused(
        archive_account(&mut tx, admin, later(), uncategorized).await,
        |error| matches!(error, AccountError::BuiltIn { .. }),
        "archiving a system account",
    );
    assert_refused(
        archive_account(&mut tx, admin, later(), AccountId(999)).await,
        |error| matches!(error, AccountError::NoSuchAccount),
        "archiving an unknown account",
    );

    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let other = account_named(&mut tx, "Income:Other").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-15",
        &[(checking, 10_000), (salary, -10_000)],
    )
    .await;
    assert_refused(
        archive_account(&mut tx, admin, later(), checking).await,
        |error| {
            matches!(
                error,
                AccountError::BalanceNotZero { kind: Kind::Asset, balance, .. }
                    if *balance == Money::from_minor(10_000)
            )
        },
        "an account with a live balance",
    );
    assert_refused(
        archive_account(&mut tx, admin, later(), salary).await,
        |error| {
            matches!(
                error,
                AccountError::BalanceNotZero { kind: Kind::Income, balance, .. }
                    if *balance == Money::from_minor(10_000)
            )
        },
        "a counter account with a live balance",
    );
    post(
        &mut tx,
        admin,
        now(),
        "2026-02-15",
        &[(checking, -10_000), (salary, 10_000)],
    )
    .await;
    let closed = archive_account(&mut tx, admin, later(), checking)
        .await
        .unwrap_or_else(|error| {
            panic!("an account whose postings cancel out was refused: {error}")
        });
    assert!(closed.is_archived());
    insert_transaction(
        &mut tx,
        "2026-03-01",
        "manual",
        true,
        NOW_TEXT,
        &[(savings, 5_000), (other, -5_000)],
    )
    .await;
    let unaffected = archive_account(&mut tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("a deleted transaction blocked an archive: {error}"));
    assert!(unaffected.is_archived());
    roll_back(tx, "archive").await;
    storage.close().await;
}

/// Full names are built from the root down and resolved segment by segment ignoring case,
/// white space and compatibility characters, archived accounts and roots included; an empty,
/// leading, trailing or doubled segment and an unknown segment resolve to nothing; an unknown
/// id has no full name.
///
/// Proves: SPEC 6.2 (full names join the path with a colon), decision 96.
#[tokio::test]
async fn full_names_resolve_in_both_directions() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "full names").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let food = create(&mut tx, admin, "Food", expenses).await.id;
    move_account(&mut tx, admin, later(), groceries, food)
        .await
        .unwrap_or_else(|error| panic!("cannot move Groceries under Food: {error}"));
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    archive_account(&mut tx, admin, later(), gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Gifts: {error}"));
    let opening = account_named(&mut tx, "Equity:Opening Balances").await;
    for (full_name, expected) in [
        ("Expenses:Food:Groceries", Some(groceries)),
        ("expenses:food:groceries", Some(groceries)),
        ("EXPENSES:FOOD:GROCERIES", Some(groceries)),
        ("Expenses", Some(expenses)),
        ("Expenses:Food", Some(food)),
        ("Equity:Opening Balances", Some(opening)),
        ("Equity:opening  balances", Some(opening)),
        ("\u{FF25}xpenses:Gifts", Some(gifts)),
        ("Expenses:Groceries", None),
        ("Expenses:Nope", None),
        ("", None),
        (":Expenses", None),
        ("Expenses:", None),
        ("Expenses::Food", None),
        ("Nope", None),
    ] {
        let resolved = accounts::resolve(&mut tx, full_name)
            .await
            .unwrap_or_else(|error| panic!("cannot resolve {full_name:?}: {error}"))
            .map(|account| account.id);
        assert_eq!(resolved, expected, "resolve({full_name:?})");
    }
    let named = accounts::full_name(&mut tx, groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot name Groceries: {error}"));
    assert_eq!(named, "Expenses:Food:Groceries");
    let root = accounts::full_name(&mut tx, expenses)
        .await
        .unwrap_or_else(|error| panic!("cannot name Expenses: {error}"));
    assert_eq!(root, "Expenses");
    assert_refused(
        accounts::full_name(&mut tx, AccountId(999)).await,
        |error| matches!(error, AccountError::NoSuchAccount),
        "the full name of an unknown account",
    );
    roll_back(tx, "full names").await;
    storage.close().await;
}

/// The eight system accounts are found by key; a missing one is reported as such.
///
/// Proves: SPEC 6.2 (the system accounts are found by their keys; a missing one is reported),
/// decision 96.
#[tokio::test]
async fn system_accounts_are_found_by_key_and_a_missing_one_is_reported() {
    let (_dir, storage, _initialized) = seeded(false).await;
    let mut tx = begin(&storage, "system keys").await;
    for key in SystemKey::ALL {
        let account = accounts::system_account(&mut tx, key)
            .await
            .unwrap_or_else(|error| panic!("no {key} account: {error}"));
        assert_eq!(account.system_key, Some(key));
        assert_eq!(account.kind, key.kind());
    }
    sqlx::query("DELETE FROM accounts WHERE system_key = 'uncategorized'")
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot delete Uncategorized: {error}"));
    assert_refused(
        accounts::system_account(&mut tx, SystemKey::Uncategorized).await,
        |error| {
            matches!(
                error,
                AccountError::MissingSystemAccount(SystemKey::Uncategorized)
            )
        },
        "a deleted system account",
    );
    roll_back(tx, "system keys").await;
    storage.close().await;
}

/// Every refusal has its own message that a form can show, and the kinds, keys and ids print
/// as their stored texts.
///
/// Proves: W2 (every refusal explains itself), decision 96.
#[test]
fn account_refusals_explain_themselves() {
    let new_account = Refused {
        word: Word::Account,
        place: None,
        action: Action::Created,
    };
    let savings = |action| Refused {
        word: Word::Account,
        place: Some(String::from("Assets:Savings")),
        action,
    };
    let messages: Vec<String> = [
        AccountError::Name {
            refused: new_account.clone(),
            error: NameError::Empty,
        },
        AccountError::NameTaken {
            refused: new_account.clone(),
            holder: String::from("Assets"),
            name: String::from("Checking"),
        },
        AccountError::NoSuchAccount,
        AccountError::PlaceNotListed {
            refused: new_account.clone(),
        },
        AccountError::OwnerNotListed {
            refused: new_account.clone(),
        },
        AccountError::ParentArchived {
            refused: new_account.clone(),
            parent: String::from("Assets:Old savings"),
        },
        AccountError::Section {
            refused: Refused {
                word: Word::Account,
                place: Some(String::from("Assets")),
                action: Action::Renamed,
            },
        },
        AccountError::BuiltIn {
            refused: Refused {
                word: Word::Account,
                place: Some(String::from("Equity:Opening Balances")),
                action: Action::Archived,
            },
        },
        AccountError::MoveUnderDescendant {
            word: Word::Account,
            account: String::from("Assets:Savings"),
            target: String::from("Assets:Savings:Emergency fund"),
        },
        AccountError::TooDeep {
            refused: savings(Action::Moved),
            depth: 7,
            inside: false,
        },
        AccountError::KindMismatch {
            word: Word::Category,
            account: String::from("Expenses:Groceries"),
            section: String::from("Income"),
        },
        AccountError::OwnerNotAllowed {
            refused: new_account,
        },
        AccountError::ChildrenNotArchived {
            account: String::from("Assets:Savings"),
            child: String::from("Assets:Savings:Emergency fund"),
        },
        AccountError::BalanceNotZero {
            kind: Kind::Asset,
            account: String::from("Assets:Checking"),
            balance: Money::from_minor(248_015),
            digits: MinorDigits::TWO,
        },
        AccountError::BalanceNotZero {
            kind: Kind::Liability,
            account: String::from("Liabilities:Credit Card"),
            balance: Money::from_minor(120_480),
            digits: MinorDigits::TWO,
        },
        AccountError::ArchivedMeanwhile {
            account: String::from("Assets:Old savings"),
            kind: Kind::Asset,
        },
        AccountError::AlreadyArchived {
            account: String::from("Assets:Old savings"),
        },
        AccountError::NotArchived {
            account: String::from("Assets:Checking"),
        },
        AccountError::ParentArchivedUnarchiveFirst {
            account: String::from("Assets:Old savings:Jar"),
            parent: String::from("Assets:Old savings"),
        },
        AccountError::MissingSystemAccount(SystemKey::Uncategorized),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "The account was not created because its name is empty. Type a name, then create it again.",
            "The account was not created because Assets already holds one named Checking. Capitals and extra spaces do not count, and archived ones count too. Choose another name.",
            "Nage could not find this account, because no account has that number. Go to Accounts and choose it from the list.",
            "The account was not created because the place chosen for it is not one of the places in the list. Choose its place again.",
            "The account was not created because the owner chosen is not one of the users in the list. Choose Joint or one of the users listed.",
            "The account was not created because Assets:Old savings is archived, and nothing new goes inside an archived account. Choose another place, or unarchive it first.",
            "Assets was not renamed because it is one of Nage's five sections, which stay as they are.",
            "Equity:Opening Balances was not archived because it is built in: Nage itself uses it. You can still rename it.",
            "Assets:Savings was not moved because Assets:Savings:Emergency fund is inside it, and an account cannot go inside itself. Choose another place.",
            "Assets:Savings was not moved because it would sit 7 levels deep, and Nage allows 6, the section included. Choose a place higher up.",
            "Expenses:Groceries was not moved because a category stays in its section. To have it under Income, make a new one there and archive this one.",
            "The account was not created because only accounts under Assets and Liabilities have an owner. Leave it without one.",
            "Assets:Savings was not archived because Assets:Savings:Emergency fund inside it is not archived. Archive that one first.",
            "Assets:Checking was not archived because it holds 2,480.15, and only an account at zero can be archived. Bring it to zero first.",
            "Liabilities:Credit Card was not archived because 1,204.80 is owed on it, and only an account at zero can be archived. Bring it to zero first.",
            "Assets:Old savings was archived while you were working, by another user or in another window, so only its name can be changed now. Unarchive it first to move it or to change its owner.",
            "Assets:Old savings was not archived because it is archived already. Nothing needs to be done.",
            "Assets:Checking was not unarchived because it is not archived. Nothing needs to be done.",
            "Assets:Old savings:Jar was not unarchived because Assets:Old savings, which holds it, is archived. Unarchive that one first.",
            "the system account uncategorized is missing",
        ]
    );
    let mut distinct = messages.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        messages.len(),
        "two refusals share a message"
    );
    assert_eq!(Kind::Liability.to_string(), "liability");
    assert_eq!(SystemKey::OpeningBalances.to_string(), "opening_balances");
    assert_eq!(AccountId(7).to_string(), "7");
    assert_eq!(UserId(3).to_string(), "3");
    assert_eq!("expense".parse::<Kind>(), Ok(Kind::Expense));
    assert!(
        "expenses".parse::<Kind>().is_err(),
        "the plural is not a kind"
    );
    assert_eq!("expenses".parse::<SystemKey>(), Ok(SystemKey::Expenses));
    assert!(
        "expense".parse::<SystemKey>().is_err(),
        "the singular is not a key"
    );
    assert_eq!(Kind::Income.root_key(), SystemKey::Income);
    assert_eq!(
        SystemKey::Uncategorized.parent_key(),
        Some(SystemKey::Expenses)
    );
    assert_eq!(SystemKey::Assets.parent_key(), None);
    assert_eq!(
        Kind::ALL.map(Kind::sign),
        [1, -1, -1, -1, 1],
        "the signs of SPEC 6.3"
    );
    assert_eq!(
        Kind::ALL.map(Kind::allows_owner),
        [true, true, false, false, false]
    );
}

/// The made-up household of the refusal and change tests (decision 139), built through the
/// core's operations on the standard setup with the starter chart: a member Sam; Savings holding
/// Emergency fund and Holiday fund, and a second Holiday fund directly under Assets; Old savings
/// holding Jar, both archived; the archived category Old gym; a chain of Assets accounts down to
/// the fifth level and a chain of categories down to the sixth; Checking holding 2,480.15 against
/// Salary and 1,204.80 owed on the Credit Card against Groceries.
struct Scene {
    admin: UserId,
    sam: UserId,
    assets: AccountId,
    liabilities: AccountId,
    equity: AccountId,
    income: AccountId,
    expenses: AccountId,
    checking: AccountId,
    savings: AccountId,
    cash: AccountId,
    card: AccountId,
    salary: AccountId,
    groceries: AccountId,
    uncategorized: AccountId,
    opening: AccountId,
    emergency: AccountId,
    holiday: AccountId,
    loose_holiday: AccountId,
    old_savings: AccountId,
    jar: AccountId,
    old_gym: AccountId,
    fourth: AccountId,
    fifth: AccountId,
    sixth: AccountId,
}

/// Builds the [`Scene`] in `tx`, the admin `admin` acting.
async fn scene(tx: &mut WriteTransaction, admin: UserId) -> Scene {
    let sam = member(tx, admin, "sam").await;
    let assets = account_named(tx, "Assets").await;
    let liabilities = account_named(tx, "Liabilities").await;
    let equity = account_named(tx, "Equity").await;
    let income = account_named(tx, "Income").await;
    let expenses = account_named(tx, "Expenses").await;
    let checking = account_named(tx, "Assets:Checking").await;
    let savings = account_named(tx, "Assets:Savings").await;
    let cash = account_named(tx, "Assets:Cash").await;
    let card = account_named(tx, "Liabilities:Credit Card").await;
    let salary = account_named(tx, "Income:Salary").await;
    let groceries = account_named(tx, "Expenses:Groceries").await;
    let uncategorized = account_named(tx, "Expenses:Uncategorized").await;
    let opening = account_named(tx, "Equity:Opening Balances").await;
    let emergency = create(tx, admin, "Emergency fund", savings).await.id;
    let holiday = create(tx, admin, "Holiday fund", savings).await.id;
    let loose_holiday = create(tx, admin, "Holiday fund", assets).await.id;
    let old_savings = create(tx, admin, "Old savings", assets).await.id;
    let jar = create(tx, admin, "Jar", old_savings).await.id;
    let old_gym = create(tx, admin, "Old gym", expenses).await.id;
    for archived in [jar, old_savings, old_gym] {
        archive_account(tx, admin, later(), archived)
            .await
            .unwrap_or_else(|error| panic!("cannot archive account {archived}: {error}"));
    }
    let mut level = assets;
    let mut chain = Vec::new();
    for name in ["Level two", "Level three", "Level four", "Level five"] {
        level = create(tx, admin, name, level).await.id;
        chain.push(level);
    }
    let mut category = expenses;
    for name in ["Home", "Repairs", "Roof", "Gutters", "Leaves"] {
        category = create(tx, admin, name, category).await.id;
    }
    post(
        tx,
        admin,
        later(),
        "2026-11-01",
        &[(checking, 248_015), (salary, -248_015)],
    )
    .await;
    post(
        tx,
        admin,
        later(),
        "2026-11-02",
        &[(groceries, 120_480), (card, -120_480)],
    )
    .await;
    Scene {
        admin,
        sam,
        assets,
        liabilities,
        equity,
        income,
        expenses,
        checking,
        savings,
        cash,
        card,
        salary,
        groceries,
        uncategorized,
        opening,
        emergency,
        holiday,
        loose_holiday,
        old_savings,
        jar,
        old_gym,
        fourth: chain[2],
        fifth: chain[3],
        sixth: category,
    }
}

/// The boxes of an account form: the name, the place it goes inside and the owner.
fn sent(name: &str, inside: AccountId, owner: Option<UserId>) -> SentBoxes {
    SentBoxes {
        name: Sent::Value(name.to_owned()),
        inside: Sent::Value(inside),
        owner: Sent::Value(owner),
    }
}

/// The boxes of a category's form, which has no owner.
fn sent_category(name: &str, inside: AccountId) -> SentBoxes {
    SentBoxes {
        owner: Sent::Absent,
        ..sent(name, inside, None)
    }
}

/// The boxes of a create form holding `name` alone: directly under the section, Joint.
fn sent_name(name: &str) -> SentBoxes {
    SentBoxes {
        name: Sent::Value(name.to_owned()),
        inside: Sent::Absent,
        owner: Sent::Absent,
    }
}

/// The refusals of a form `result` holds, panicking when it was accepted or refused otherwise.
#[track_caller]
fn form_refusals(result: Result<Account, AccountError>, label: &str) -> FormRefusals {
    match result {
        Ok(account) => panic!("{label} was accepted: {account:?}"),
        Err(AccountError::Form(refusals)) => *refusals,
        Err(error) => panic!("{label}: refused as {error:?}, not as a form"),
    }
}

/// The refusals of a change `result` holds, panicking when it was saved, clashed or failed.
#[track_caller]
fn change_refusals(result: Result<Changed, ChangeError>, label: &str) -> FormRefusals {
    match result {
        Ok(changed) => panic!("{label} was accepted: {changed:?}"),
        Err(ChangeError::Refused(refusals)) => *refusals,
        Err(error) => panic!("{label}: {error:?}, not refused"),
    }
}

/// The clash of a change `result`, panicking when it was saved, refused or failed.
#[track_caller]
fn clash_of(result: Result<Changed, ChangeError>, label: &str) -> Clash {
    match result {
        Ok(changed) => panic!("{label} was accepted: {changed:?}"),
        Err(ChangeError::Clash(clash)) => *clash,
        Err(error) => panic!("{label}: {error:?}, not a clash"),
    }
}

/// The account a change `result` saved, panicking when it saved nothing or failed.
#[track_caller]
fn saved_by(result: Result<Changed, ChangeError>, label: &str) -> Account {
    match result {
        Ok(Changed::Saved(account)) => account,
        Ok(Changed::Nothing) => panic!("{label} saved nothing"),
        Err(error) => panic!("{label}: {error}"),
    }
}

/// Every account as `tx` sees it: what a refused, clashing or empty change must leave as it
/// found it (decision 99).
async fn chart_rows(tx: &mut WriteTransaction) -> Vec<Account> {
    accounts::accounts(tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"))
}

/// One case of the change form's Inside box: what the request sends, the refusal expected under
/// Inside, and the case's label.
type InsideCase = (SentBoxes, fn(&AccountError) -> bool, &'static str);

/// The first part of a refusal of a new account worded `word`.
fn new_refused(word: Word) -> Refused {
    Refused {
        word,
        place: None,
        action: Action::Created,
    }
}

/// The first part of a refusal of `action` on the account at `place`, worded `word`.
fn refused_at(word: Word, place: &str, action: Action) -> Refused {
    Refused {
        word,
        place: Some(place.to_owned()),
        action,
    }
}

/// `create_account` refuses with the places, names and figures its sentence needs: the word by
/// the section, the parent's full place and the sibling's stored name for a name taken, the
/// depth, the archived place, and a place or an owner no account or user is.
///
/// Proves: Decision 136 with the pause's P9: each refusal of create_account carries the word, the parent's full place, the sibling's name, the depth, the archived place, and the place or owner that is not in the list.
#[tokio::test]
async fn create_account_refusals_name_their_places() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "create refusals").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let refused = |result: Result<Account, AccountError>, label: &str| refusal(result, label);
    let error = refused(
        create_account(&mut tx, admin, later(), &new("", s.assets)).await,
        "an empty name",
    );
    assert!(
        matches!(&error, AccountError::Name { refused, error: NameError::Empty } if *refused == new_refused(Word::Account)),
        "an empty name: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("Bills: Hydro", s.expenses)).await,
        "a colon",
    );
    assert!(
        matches!(&error, AccountError::Name { refused, error: NameError::Colon } if *refused == new_refused(Word::Category)),
        "a colon: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("  checking ", s.assets)).await,
        "a name taken",
    );
    assert!(
        matches!(
            &error,
            AccountError::NameTaken { refused, holder, name }
                if *refused == new_refused(Word::Account) && holder == "Assets" && name == "Checking"
        ),
        "a name taken: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("emergency FUND", s.savings)).await,
        "a name taken inside Savings",
    );
    assert!(
        matches!(
            &error,
            AccountError::NameTaken { holder, name, .. }
                if holder == "Assets:Savings" && name == "Emergency fund"
        ),
        "a name taken inside Savings: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("Moss", s.sixth)).await,
        "a seventh level",
    );
    assert!(
        matches!(
            &error,
            AccountError::TooDeep { refused, depth: 7, inside: false }
                if *refused == new_refused(Word::Category)
        ),
        "a seventh level: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("Coins", s.old_savings)).await,
        "an archived place",
    );
    assert!(
        matches!(
            &error,
            AccountError::ParentArchived { refused, parent }
                if *refused == new_refused(Word::Account) && parent == "Assets:Old savings"
        ),
        "an archived place: {error:?}"
    );
    let error = refused(
        create_account(&mut tx, admin, later(), &new("Coins", AccountId(999))).await,
        "a place no account is",
    );
    assert!(
        matches!(&error, AccountError::PlaceNotListed { refused } if refused.place.is_none() && refused.action == Action::Created),
        "a place no account is: {error:?}"
    );
    let with_owner = |name: &str, parent: AccountId, owner: UserId| NewAccount {
        owner: Some(owner),
        ..new(name, parent)
    };
    let error = refused(
        create_account(
            &mut tx,
            admin,
            later(),
            &with_owner("Coins", s.assets, UserId(99)),
        )
        .await,
        "an owner no user is",
    );
    assert!(
        matches!(&error, AccountError::OwnerNotListed { refused } if *refused == new_refused(Word::Account)),
        "an owner no user is: {error:?}"
    );
    let error = refused(
        create_account(
            &mut tx,
            admin,
            later(),
            &with_owner("Rent", s.expenses, s.sam),
        )
        .await,
        "an owner under Expenses",
    );
    assert!(
        matches!(&error, AccountError::OwnerNotAllowed { refused } if *refused == new_refused(Word::Category)),
        "an owner under Expenses: {error:?}"
    );
    assert_sound(&mut tx, "the create refusals").await;
    roll_back(tx, "create refusals").await;
    storage.close().await;
}

/// `rename_account`, `move_account` and `set_owner` name the account by its full place in each
/// refusal, a section as `Section` (an owner given to one included) and a built-in account as
/// `BuiltIn`, with the places the sentences need.
///
/// Proves: Decision 136 with P14: each refusal of rename_account, move_account and set_owner names the account by its full place, a section (an owner given to one included) as Section and a built-in account apart.
#[tokio::test]
async fn rename_move_and_owner_refusals_name_their_places() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "rename, move and owner refusals").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let error = refusal(
        rename_account(&mut tx, admin, later(), s.savings, " ").await,
        "an empty new name",
    );
    assert!(
        matches!(
            &error,
            AccountError::Name { refused, error: NameError::Empty }
                if *refused == refused_at(Word::Account, "Assets:Savings", Action::Renamed)
        ),
        "an empty new name: {error:?}"
    );
    let error = refusal(
        rename_account(&mut tx, admin, later(), s.holiday, "emergency FUND").await,
        "a sibling's name",
    );
    assert!(
        matches!(
            &error,
            AccountError::NameTaken { refused, holder, name }
                if *refused == refused_at(Word::Account, "Assets:Savings:Holiday fund", Action::Renamed)
                    && holder == "Assets:Savings"
                    && name == "Emergency fund"
        ),
        "a sibling's name: {error:?}"
    );
    let error = refusal(
        rename_account(&mut tx, admin, later(), s.assets, "Things").await,
        "a section renamed",
    );
    assert!(
        matches!(&error, AccountError::Section { refused } if *refused == refused_at(Word::Account, "Assets", Action::Renamed)),
        "a section renamed: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.assets, s.liabilities).await,
        "a section moved",
    );
    assert!(
        matches!(&error, AccountError::Section { refused } if *refused == refused_at(Word::Account, "Assets", Action::Moved)),
        "a section moved: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.opening, s.equity).await,
        "a built-in account moved",
    );
    assert!(
        matches!(&error, AccountError::BuiltIn { refused } if *refused == refused_at(Word::Account, "Equity:Opening Balances", Action::Moved)),
        "a built-in account moved: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.savings, s.emergency).await,
        "a move inside itself",
    );
    assert!(
        matches!(
            &error,
            AccountError::MoveUnderDescendant { word: Word::Account, account, target }
                if account == "Assets:Savings" && target == "Assets:Savings:Emergency fund"
        ),
        "a move inside itself: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.groceries, s.income).await,
        "a move to another section",
    );
    assert!(
        matches!(
            &error,
            AccountError::KindMismatch { word: Word::Category, account, section }
                if account == "Expenses:Groceries" && section == "Income"
        ),
        "a move to another section: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.holiday, s.old_savings).await,
        "a move into an archived place",
    );
    assert!(
        matches!(
            &error,
            AccountError::ParentArchived { refused, parent }
                if *refused == refused_at(Word::Account, "Assets:Savings:Holiday fund", Action::Moved)
                    && parent == "Assets:Old savings"
        ),
        "a move into an archived place: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.savings, s.fifth).await,
        "a move too deep",
    );
    assert!(
        matches!(
            &error,
            AccountError::TooDeep { refused, depth: 7, inside: true }
                if *refused == refused_at(Word::Account, "Assets:Savings", Action::Moved)
        ),
        "a move too deep: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.loose_holiday, s.savings).await,
        "a move beside a namesake",
    );
    assert!(
        matches!(
            &error,
            AccountError::NameTaken { refused, holder, name }
                if *refused == refused_at(Word::Account, "Assets:Holiday fund", Action::Moved)
                    && holder == "Assets:Savings"
                    && name == "Holiday fund"
        ),
        "a move beside a namesake: {error:?}"
    );
    let error = refusal(
        move_account(&mut tx, admin, later(), s.savings, AccountId(999)).await,
        "a move to a place no account is",
    );
    assert!(
        matches!(&error, AccountError::PlaceNotListed { refused } if *refused == refused_at(Word::Account, "Assets:Savings", Action::Moved)),
        "a move to a place no account is: {error:?}"
    );
    let error = refusal(
        set_owner(&mut tx, admin, later(), s.assets, Some(s.sam)).await,
        "an owner given to a section",
    );
    assert!(
        matches!(&error, AccountError::Section { refused } if *refused == refused_at(Word::Account, "Assets", Action::Changed)),
        "an owner given to a section: {error:?}"
    );
    let error = refusal(
        set_owner(&mut tx, admin, later(), s.groceries, Some(s.sam)).await,
        "an owner given to a category",
    );
    assert!(
        matches!(&error, AccountError::OwnerNotAllowed { refused } if *refused == refused_at(Word::Category, "Expenses:Groceries", Action::Changed)),
        "an owner given to a category: {error:?}"
    );
    let error = refusal(
        set_owner(&mut tx, admin, later(), s.opening, Some(s.sam)).await,
        "an owner given to a built-in account",
    );
    assert!(
        matches!(&error, AccountError::OwnerNotAllowed { refused } if *refused == refused_at(Word::Account, "Equity:Opening Balances", Action::Changed)),
        "an owner given to a built-in account: {error:?}"
    );
    let error = refusal(
        set_owner(&mut tx, admin, later(), s.checking, Some(UserId(99))).await,
        "an owner no user is",
    );
    assert!(
        matches!(&error, AccountError::OwnerNotListed { refused } if *refused == refused_at(Word::Account, "Assets:Checking", Action::Changed)),
        "an owner no user is: {error:?}"
    );
    assert_sound(&mut tx, "the rename, move and owner refusals").await;
    roll_back(tx, "rename, move and owner refusals").await;
    storage.close().await;
}

/// `archive_account` and `unarchive_account` name the account and the child or the holder in
/// each refusal, and a balance as the account's own entries over all dates in natural sign, an
/// entry dated after today included, with the book's digits; the sentence says what is owed on
/// an account under Liabilities, and calls a category a category.
///
/// Proves: Decision 136 and INV-12 with P9: archive and unarchive refusals name the child, the holder and the own entries over all dates in natural sign, in the book's digits, under Liabilities as owed.
#[tokio::test]
async fn archive_and_unarchive_refusals_name_their_places() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "archive refusals").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.savings).await,
        "a holder of live accounts",
    );
    assert!(
        matches!(
            &error,
            AccountError::ChildrenNotArchived { account, child }
                if account == "Assets:Savings" && child == "Assets:Savings:Emergency fund"
        ),
        "a holder of live accounts: {error:?}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.checking).await,
        "an account holding money",
    );
    assert!(
        matches!(
            &error,
            AccountError::BalanceNotZero { kind: Kind::Asset, account, balance, digits }
                if account == "Assets:Checking"
                    && *balance == Money::from_minor(248_015)
                    && *digits == MinorDigits::TWO
        ),
        "an account holding money: {error:?}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.card).await,
        "a card with money owed on it",
    );
    assert!(
        matches!(
            &error,
            AccountError::BalanceNotZero { kind: Kind::Liability, account, balance, digits }
                if account == "Liabilities:Credit Card"
                    && *balance == Money::from_minor(120_480)
                    && *digits == MinorDigits::TWO
        ),
        "a card with money owed on it: {error:?}"
    );
    assert!(
        error.to_string().contains("1,204.80 is owed on it"),
        "the card's sentence: {error}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.groceries).await,
        "a category with entries",
    );
    assert!(
        matches!(&error, AccountError::BalanceNotZero { kind: Kind::Expense, account, .. } if account == "Expenses:Groceries"),
        "a category with entries: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("only a category at zero can be archived"),
        "the category's sentence: {error}"
    );
    post(
        &mut tx,
        admin,
        later(),
        "2027-01-15",
        &[(s.cash, 2_500), (s.salary, -2_500)],
    )
    .await;
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.cash).await,
        "an account whose money comes after today",
    );
    assert!(
        matches!(
            &error,
            AccountError::BalanceNotZero { account, balance, .. }
                if account == "Assets:Cash" && *balance == Money::from_minor(2_500)
        ),
        "an account whose money comes after today: {error:?}"
    );
    let error = refusal(
        unarchive_account(&mut tx, admin, later(), s.jar).await,
        "an unarchive inside an archived holder",
    );
    assert!(
        matches!(
            &error,
            AccountError::ParentArchivedUnarchiveFirst { account, parent }
                if account == "Assets:Old savings:Jar" && parent == "Assets:Old savings"
        ),
        "an unarchive inside an archived holder: {error:?}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.old_savings).await,
        "a second archive",
    );
    assert!(
        matches!(&error, AccountError::AlreadyArchived { account } if account == "Assets:Old savings"),
        "a second archive: {error:?}"
    );
    let error = refusal(
        unarchive_account(&mut tx, admin, later(), s.checking).await,
        "an unarchive of a live account",
    );
    assert!(
        matches!(&error, AccountError::NotArchived { account } if account == "Assets:Checking"),
        "an unarchive of a live account: {error:?}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.uncategorized).await,
        "a built-in category archived",
    );
    assert!(
        matches!(&error, AccountError::BuiltIn { refused } if *refused == refused_at(Word::Category, "Expenses:Uncategorized", Action::Archived)),
        "a built-in category archived: {error:?}"
    );
    let error = refusal(
        archive_account(&mut tx, admin, later(), s.liabilities).await,
        "a section archived",
    );
    assert!(
        matches!(&error, AccountError::Section { refused } if *refused == refused_at(Word::Account, "Liabilities", Action::Archived)),
        "a section archived: {error:?}"
    );
    assert_sound(&mut tx, "the archive refusals").await;
    roll_back(tx, "archive refusals").await;
    storage.close().await;
}

/// Every account refusal as the operations produce it on the made-up household, in its three
/// parts (what was refused, why, what to do) except a section's, where nothing can be done, and
/// the clash's, whose page offers the two choices: one text snapshot of every sentence.
///
/// Proves: Decisions 102 and 136 with P4, P9 and P14: every account refusal, produced by the operations on a made-up book, in its three parts except a section's and the clash's, pinned as one text snapshot.
#[tokio::test]
async fn the_account_refusals_in_words() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "the refusals in words").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let mut said: Vec<(&str, String)> = Vec::new();
    let label = "A new account with an empty name, under Assets";
    let result = create_account(&mut tx, admin, later(), &new("", s.assets)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new category named Bills: Hydro, under Expenses";
    let result = create_account(&mut tx, admin, later(), &new("Bills: Hydro", s.expenses)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings renamed to a name of 71 characters";
    let result = rename_account(&mut tx, admin, later(), s.savings, &"x".repeat(71)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings renamed to a name with a tab in it";
    let result = rename_account(&mut tx, admin, later(), s.savings, "Rainy\tday").await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new account named with an escape character in it, under Assets";
    let result = create_account(&mut tx, admin, later(), &new("Pay\u{1b}day", s.assets)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new account named checking, under Assets";
    let result = create_account(&mut tx, admin, later(), &new("checking", s.assets)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Assets:Holiday fund moved inside Savings, which holds a Holiday fund";
    let result = move_account(&mut tx, admin, later(), s.loose_holiday, s.savings).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings moved inside its own Emergency fund";
    let result = move_account(&mut tx, admin, later(), s.savings, s.emergency).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings moved inside itself";
    let result = move_account(&mut tx, admin, later(), s.savings, s.savings).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new category inside a category at the sixth level";
    let result = create_account(&mut tx, admin, later(), &new("Moss", s.sixth)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings, which holds Emergency fund, moved inside an account at the fifth level";
    let result = move_account(&mut tx, admin, later(), s.savings, s.fifth).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Groceries moved under Income";
    let result = move_account(&mut tx, admin, later(), s.groceries, s.income).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new account inside the archived Old savings";
    let result = create_account(&mut tx, admin, later(), &new("Coins", s.old_savings)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Holiday fund moved inside the archived Old savings";
    let result = move_account(&mut tx, admin, later(), s.holiday, s.old_savings).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings archived while Emergency fund inside it is not";
    let result = archive_account(&mut tx, admin, later(), s.savings).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Checking archived while it holds 2,480.15";
    let result = archive_account(&mut tx, admin, later(), s.checking).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Credit Card archived while 1,204.80 is owed on it";
    let result = archive_account(&mut tx, admin, later(), s.card).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Jar unarchived while Old savings, which holds it, is archived";
    let result = unarchive_account(&mut tx, admin, later(), s.jar).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Opening Balances archived";
    let result = archive_account(&mut tx, admin, later(), s.opening).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Opening Balances moved";
    let result = move_account(&mut tx, admin, later(), s.opening, s.equity).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Assets renamed";
    let result = rename_account(&mut tx, admin, later(), s.assets, "Things").await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Assets moved";
    let result = move_account(&mut tx, admin, later(), s.assets, s.liabilities).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Assets archived";
    let result = archive_account(&mut tx, admin, later(), s.assets).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "An owner given to Assets";
    let result = set_owner(&mut tx, admin, later(), s.assets, Some(s.sam)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new category with an owner, under Expenses";
    let rent = NewAccount {
        owner: Some(s.sam),
        ..new("Rent", s.expenses)
    };
    let result = create_account(&mut tx, admin, later(), &rent).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "An owner given to Groceries";
    let result = set_owner(&mut tx, admin, later(), s.groceries, Some(s.sam)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new account inside a place the list does not offer";
    let elsewhere = SentBoxes {
        inside: Sent::Value(s.card),
        ..sent_name("Spare")
    };
    let result = create_from_form(&mut tx, admin, later(), Kind::Asset, &elsewhere).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A new account with an owner the list does not offer";
    let nobody = SentBoxes {
        owner: Sent::Value(Some(UserId(99))),
        ..sent_name("Spare")
    };
    let result = create_from_form(&mut tx, admin, later(), Kind::Asset, &nobody).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings moved inside a place the list does not offer";
    let result = move_account(&mut tx, admin, later(), s.savings, AccountId(999)).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Savings given an owner the list does not offer";
    let result = set_owner(&mut tx, admin, later(), s.savings, Some(UserId(99))).await;
    said.push((label, refusal(result, label).to_string()));
    let label =
        "A full change form of Old savings saved with another place after another save archived it";
    let result = change_account(
        &mut tx,
        admin,
        later(),
        s.old_savings,
        &sent("Old savings", s.assets, None),
        &sent("Old savings", s.savings, None),
    )
    .await;
    said.push((label, refusal(result, label).to_string()));
    let label =
        "A full change form of Old gym saved with another place after another save archived it";
    let result = change_account(
        &mut tx,
        admin,
        later(),
        s.old_gym,
        &sent_category("Old gym", s.expenses),
        &sent_category("Old gym", s.groceries),
    )
    .await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Old savings archived a second time";
    let result = archive_account(&mut tx, admin, later(), s.old_savings).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "Checking unarchived while it is not archived";
    let result = unarchive_account(&mut tx, admin, later(), s.checking).await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A change of Savings's name after another save changed its name";
    let result = change_account(
        &mut tx,
        admin,
        later(),
        s.savings,
        &sent("Savings box", s.assets, None),
        &sent("Rainy day", s.assets, None),
    )
    .await;
    said.push((label, refusal(result, label).to_string()));
    let label = "A change of Savings's name and owner after another save changed both";
    let result = change_account(
        &mut tx,
        admin,
        later(),
        s.savings,
        &sent("Savings box", s.assets, Some(s.sam)),
        &sent("Rainy day", s.assets, Some(admin)),
    )
    .await;
    said.push((label, refusal(result, label).to_string()));
    move_account(&mut tx, admin, later(), s.cash, s.savings)
        .await
        .unwrap_or_else(|error| panic!("cannot move Cash inside Savings: {error}"));
    set_owner(&mut tx, admin, later(), s.cash, Some(s.sam))
        .await
        .unwrap_or_else(|error| panic!("cannot give Cash to Sam: {error}"));
    let label = "A change of Cash's place and owner after another save changed both";
    let result = change_account(
        &mut tx,
        admin,
        later(),
        s.cash,
        &sent("Cash", s.assets, None),
        &sent("Cash", s.checking, Some(admin)),
    )
    .await;
    said.push((label, refusal(result, label).to_string()));
    let label = "An account number nobody has";
    let result = rename_account(&mut tx, admin, later(), AccountId(999), "Ghost").await;
    said.push((label, refusal(result, label).to_string()));
    let refusals = said
        .iter()
        .map(|(label, sentence)| format!("{label}:\n{sentence}"))
        .collect::<Vec<String>>()
        .join("\n\n");
    insta::assert_snapshot!("account_refusals", refusals);
    assert_sound(&mut tx, "the refusals in words").await;
    roll_back(tx, "the refusals in words").await;
    storage.close().await;
}

/// `create_from_form` returns every refusal of the form at once, at most one per box in the
/// order name, inside, owner, leaves the name taken out when the place itself is refused, and
/// writes nothing; a form without a problem creates the account where it was asked, for the
/// owner chosen, Joint when the box is empty.
///
/// Proves: Decision 132 with P3 and decision 99: create_from_form returns every refusal at once, one per box in the order name, inside, owner, leaves out the name taken when the place is refused, and writes nothing.
#[tokio::test]
async fn a_create_form_refused_in_two_boxes_writes_nothing() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a form refused in two boxes").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let before = chart_rows(&mut tx).await;
    let three = sent("Bills: Hydro", s.old_savings, Some(UserId(99)));
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Asset, &three).await,
        "a form with three problems",
    );
    assert!(refusals.top.is_none(), "a top refusal: {refusals:?}");
    assert!(
        matches!(
            &refusals.name,
            Some(AccountError::Name {
                error: NameError::Colon,
                ..
            })
        ),
        "the name box: {refusals:?}"
    );
    assert!(
        matches!(&refusals.inside, Some(AccountError::ParentArchived { parent, .. }) if parent == "Assets:Old savings"),
        "the inside box: {refusals:?}"
    );
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotListed { .. })),
        "the owner box: {refusals:?}"
    );
    let sentences = refusals.to_string();
    let name_at = sentences.find("its name contains a colon");
    let inside_at = sentences.find("is archived, and nothing new");
    let owner_at = sentences.find("the owner chosen");
    assert!(
        name_at < inside_at && inside_at < owner_at && name_at.is_some(),
        "the sentences in the order name, inside, owner: {sentences}"
    );
    let taken_but_refused = sent("Jar", s.old_savings, None);
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Asset, &taken_but_refused).await,
        "a name taken in a refused place",
    );
    assert!(
        refusals.name.is_none(),
        "the name taken in a refused place is left out: {refusals:?}"
    );
    assert!(
        refusals.inside.is_some() && refusals.owner.is_none(),
        "{refusals:?}"
    );
    let taken = SentBoxes {
        owner: Sent::Value(Some(UserId(99))),
        ..sent_name("checking")
    };
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Asset, &taken).await,
        "a name taken and an owner no user is",
    );
    assert!(
        matches!(&refusals.name, Some(AccountError::NameTaken { holder, name, .. }) if holder == "Assets" && name == "Checking"),
        "the name box: {refusals:?}"
    );
    assert!(refusals.inside.is_none(), "the inside box: {refusals:?}");
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotListed { .. })),
        "the owner box: {refusals:?}"
    );
    let refusals = form_refusals(
        create_from_form(
            &mut tx,
            admin,
            later(),
            Kind::Asset,
            &sent("holiday FUND", s.savings, None),
        )
        .await,
        "a name taken inside Savings",
    );
    assert!(
        matches!(&refusals.name, Some(AccountError::NameTaken { holder, name, .. }) if holder == "Assets:Savings" && name == "Holiday fund"),
        "the name box names the place chosen: {refusals:?}"
    );
    assert_eq!(
        chart_rows(&mut tx).await,
        before,
        "a refused form wrote nothing"
    );
    let rainy = create_from_form(
        &mut tx,
        admin,
        later(),
        Kind::Asset,
        &sent(" Rainy  day ", s.savings, Some(s.sam)),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create Rainy day: {error}"));
    assert_eq!(rainy.name, "Rainy day");
    assert_eq!(rainy.parent_id, Some(s.savings));
    assert_eq!(rainy.owner_user_id, Some(s.sam));
    assert_eq!((rainy.created_by, rainy.created_at), (admin, later()));
    let tips = create_from_form(&mut tx, admin, later(), Kind::Income, &sent_name("Tips"))
        .await
        .unwrap_or_else(|error| panic!("cannot create Tips: {error}"));
    assert_eq!(tips.parent_id, Some(s.income));
    assert_eq!(tips.owner_user_id, None, "an empty owner box is Joint");
    assert_eq!(tips.kind, Kind::Income);
    assert_sound(&mut tx, "a form refused in two boxes").await;
    roll_back(tx, "a form refused in two boxes").await;
    storage.close().await;
}

/// The create form refuses under Inside, as not in the list, a number no account has, a
/// malformed one, a place in another section, another section and a built-in account, and a
/// place at the sixth level as too deep, the new account at the seventh; and
/// under Owner, as not in the list, a number no user has, a malformed one and a deactivated
/// user; an owner sent to a category's form is refused as not allowed.
///
/// Proves: Decisions 132 and 136 with P3 and P6: an unknown or malformed number, another section's place, a section or a built-in account under Inside, an unknown, malformed or deactivated owner under Owner, as not in the list.
#[tokio::test]
async fn the_create_form_refuses_what_its_lists_do_not_offer() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "the create form's lists").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let admin = s.admin;
    let before = chart_rows(&mut tx).await;
    for (under, inside, label) in [
        (
            Kind::Asset,
            Sent::Value(AccountId(999)),
            "a number no account has",
        ),
        (Kind::Asset, Sent::Malformed, "a malformed number"),
        (
            Kind::Asset,
            Sent::Value(s.groceries),
            "a place in another section",
        ),
        (Kind::Asset, Sent::Value(s.liabilities), "another section"),
        (
            Kind::Asset,
            Sent::Value(s.opening),
            "a built-in account of Equity",
        ),
        (
            Kind::Expense,
            Sent::Value(s.uncategorized),
            "a built-in category",
        ),
        (Kind::Expense, Sent::Value(s.salary), "an income category"),
    ] {
        let boxes = SentBoxes {
            inside,
            ..sent_name("Spare")
        };
        let refusals = form_refusals(
            create_from_form(&mut tx, admin, later(), under, &boxes).await,
            label,
        );
        assert!(
            matches!(
                &refusals.inside,
                Some(AccountError::PlaceNotListed { refused }) if refused.place.is_none() && refused.action == Action::Created
            ),
            "{label}: {refusals:?}"
        );
        assert!(
            refusals.name.is_none() && refusals.owner.is_none() && refusals.top.is_none(),
            "{label}: {refusals:?}"
        );
    }
    let deep = SentBoxes {
        inside: Sent::Value(s.sixth),
        ..sent_name("Moss")
    };
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Expense, &deep).await,
        "a place at the sixth level",
    );
    assert!(
        matches!(
            &refusals.inside,
            Some(AccountError::TooDeep {
                depth: 7,
                inside: false,
                ..
            })
        ),
        "a place at the sixth level: {refusals:?}"
    );
    let retired = member(&mut tx, admin, "retired").await;
    test_support::deactivate_unchecked(&mut tx, retired, admin, later()).await;
    for (owner, label) in [
        (Sent::Value(Some(UserId(99))), "a number no user has"),
        (Sent::Malformed, "a malformed owner"),
        (Sent::Value(Some(retired)), "a deactivated user"),
    ] {
        let boxes = SentBoxes {
            owner,
            ..sent_name("Spare")
        };
        let refusals = form_refusals(
            create_from_form(&mut tx, admin, later(), Kind::Liability, &boxes).await,
            label,
        );
        assert!(
            matches!(&refusals.owner, Some(AccountError::OwnerNotListed { refused }) if refused.word == Word::Account),
            "{label}: {refusals:?}"
        );
        assert!(
            refusals.name.is_none() && refusals.inside.is_none(),
            "{label}: {refusals:?}"
        );
    }
    let owned_category = SentBoxes {
        owner: Sent::Value(Some(s.sam)),
        ..sent_name("Rent")
    };
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Expense, &owned_category).await,
        "an owner sent to a category's form",
    );
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotAllowed { refused }) if refused.word == Word::Category),
        "an owner sent to a category's form: {refusals:?}"
    );
    let malformed_on_income = SentBoxes {
        owner: Sent::Malformed,
        ..sent_name("Tips")
    };
    let refusals = form_refusals(
        create_from_form(&mut tx, admin, later(), Kind::Income, &malformed_on_income).await,
        "a malformed owner sent to an income category's form",
    );
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotAllowed { refused }) if refused.word == Word::Category),
        "a malformed owner sent to an income category's form: {refusals:?}"
    );
    assert_eq!(
        chart_rows(&mut tx).await,
        before,
        "the refused forms wrote nothing"
    );
    assert_sound(&mut tx, "the create form's lists").await;
    roll_back(tx, "the create form's lists").await;
    storage.close().await;
}

/// `change_account` judges each box against what the form showed: the name after it is
/// normalized, a box the request does not carry as unchanged; it saves only the boxes the person
/// changed, keeping what another save wrote in the others, and writes the updated pair of the
/// acting user and instant, the created pair untouched; an owner changed back to Joint, and a
/// name and an owner changed in one save, are written.
///
/// Proves: Decision 134 with P2 and INV-26: changed is judged per box against what the form showed, the name normalized, a box not sent unchanged; another save's other boxes are kept and the updated pair stamped.
#[tokio::test]
async fn change_account_saves_only_the_changed_boxes() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "changed boxes").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let shown = sent("Savings", s.assets, None);
    set_owner(&mut tx, s.admin, later(), s.savings, Some(s.admin))
        .await
        .unwrap_or_else(|error| panic!("the other save failed: {error}"));
    let at = stamp(LATER_SECOND + 60);
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            at,
            s.savings,
            &shown,
            &sent("  Rainy   day ", s.assets, None),
        )
        .await,
        "a new name",
    );
    assert_eq!(saved.name, "Rainy day", "the name is normalized");
    assert_eq!(saved.parent_id, Some(s.assets));
    assert_eq!(
        saved.owner_user_id,
        Some(s.admin),
        "the other save's owner is kept"
    );
    assert_eq!((saved.updated_by, saved.updated_at), (s.sam, at));
    assert_eq!((saved.created_by, saved.created_at), (s.admin, now()));
    let shown = sent("Rainy day", s.assets, Some(s.admin));
    let wanted = SentBoxes {
        name: Sent::Absent,
        inside: Sent::Absent,
        owner: Sent::Value(Some(s.sam)),
    };
    let later_still = stamp(LATER_SECOND + 120);
    let saved = saved_by(
        change_account(&mut tx, s.admin, later_still, s.savings, &shown, &wanted).await,
        "a new owner alone",
    );
    assert_eq!(saved.name, "Rainy day", "a box not sent is unchanged");
    assert_eq!(saved.owner_user_id, Some(s.sam));
    assert_eq!((saved.updated_by, saved.updated_at), (s.admin, later_still));
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later_still,
            s.savings,
            &sent("Rainy day", s.assets, Some(s.sam)),
            &sent("Rainy day", s.assets, None),
        )
        .await,
        "an owned account made joint again",
    );
    assert_eq!(saved.owner_user_id, None, "Joint is a change of owner");
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later_still,
            s.checking,
            &sent("Checking", s.assets, None),
            &sent("Main account", s.assets, Some(s.sam)),
        )
        .await,
        "a new name and a new owner in one save",
    );
    assert_eq!(
        (saved.name.as_str(), saved.owner_user_id),
        ("Main account", Some(s.sam)),
        "both boxes are written"
    );
    let moved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.groceries,
            &sent_category("Groceries", s.expenses),
            &sent_category("groceries", s.expenses),
        )
        .await,
        "a change of capitals alone",
    );
    assert_eq!(moved.name, "groceries", "a change of capitals is a change");
    let moved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.holiday,
            &sent("Holiday fund", s.savings, None),
            &sent("Holiday fund", s.checking, None),
        )
        .await,
        "a move",
    );
    assert_eq!(moved.parent_id, Some(s.checking));
    assert_sound(&mut tx, "changed boxes").await;
    roll_back(tx, "changed boxes").await;
    storage.close().await;
}

/// A box the person changed that another save changed since the form was shown, to another
/// value, is a clash: it names the clashing boxes, every box the person changed and what is
/// saved, is judged before every refusal, and writes nothing; two saves of the same value do
/// not clash; a shown value that is malformed counts as changed by another save, and a malformed
/// value sent in a box another save changed is a clash.
///
/// Proves: Decision 134 with P2: a box another save changed meanwhile to another value is a clash naming the boxes, what is saved and what was changed, judged first, nothing written; the same value is no clash.
#[tokio::test]
async fn change_account_refuses_a_clash_and_writes_nothing() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a clash").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let shown = sent("Savings", s.assets, None);
    rename_account(&mut tx, s.sam, later(), s.savings, "Nest egg")
        .await
        .unwrap_or_else(|error| panic!("the other rename failed: {error}"));
    set_owner(&mut tx, s.sam, later(), s.savings, Some(s.sam))
        .await
        .unwrap_or_else(|error| panic!("the other owner failed: {error}"));
    let before = chart_rows(&mut tx).await;
    let clash = clash_of(
        change_account(
            &mut tx,
            s.admin,
            later(),
            s.savings,
            &shown,
            &sent("Rainy day", s.assets, None),
        )
        .await,
        "a rename after another rename",
    );
    assert_eq!(
        clash,
        Clash {
            place: String::from("Assets:Nest egg"),
            boxes: vec![BoxName::Name],
            changed: vec![BoxName::Name],
            saved: AccountBoxes {
                name: String::from("Nest egg"),
                parent: Some(s.assets),
                owner: Some(s.sam),
            },
        }
    );
    let clash = clash_of(
        change_account(
            &mut tx,
            s.admin,
            later(),
            s.savings,
            &sent("Nest egg", s.assets, None),
            &sent("Rainy day", s.assets, Some(s.admin)),
        )
        .await,
        "a rename and an owner after another owner",
    );
    assert_eq!(clash.boxes, vec![BoxName::Owner]);
    assert_eq!(clash.changed, vec![BoxName::Name, BoxName::Owner]);
    let clash = clash_of(
        change_account(
            &mut tx,
            s.admin,
            later(),
            s.savings,
            &sent("Nest egg", s.assets, None),
            &sent("a:b", s.groceries, Some(UserId(99))),
        )
        .await,
        "a clash with refusals besides",
    );
    assert_eq!(
        clash.boxes,
        vec![BoxName::Owner],
        "the clash is judged before the refusals"
    );
    let malformed_shown = SentBoxes {
        inside: Sent::Malformed,
        ..sent("Nest egg", s.assets, Some(s.sam))
    };
    let clash = clash_of(
        change_account(
            &mut tx,
            s.admin,
            later(),
            s.savings,
            &malformed_shown,
            &sent("Nest egg", s.checking, Some(s.sam)),
        )
        .await,
        "a malformed shown place",
    );
    assert_eq!(clash.boxes, vec![BoxName::Inside]);
    let malformed_wanted = SentBoxes {
        owner: Sent::Malformed,
        ..sent("Nest egg", s.assets, None)
    };
    let clash = clash_of(
        change_account(
            &mut tx,
            s.admin,
            later(),
            s.savings,
            &sent("Nest egg", s.assets, None),
            &malformed_wanted,
        )
        .await,
        "a malformed owner after another owner",
    );
    assert_eq!(clash.boxes, vec![BoxName::Owner]);
    assert_eq!(chart_rows(&mut tx).await, before, "a clash wrote nothing");
    let same = change_account(
        &mut tx,
        s.admin,
        later(),
        s.savings,
        &shown,
        &sent("Nest egg", s.assets, Some(s.sam)),
    )
    .await
    .unwrap_or_else(|error| panic!("the same values clashed: {error}"));
    assert_eq!(
        same,
        Changed::Nothing,
        "both saves changed the boxes to the same values"
    );
    assert_eq!(chart_rows(&mut tx).await, before, "nothing to write");
    assert_sound(&mut tx, "a clash").await;
    roll_back(tx, "a clash").await;
    storage.close().await;
}

/// A form that sends what it showed, one that sends what another save already wrote, one whose
/// name differs only by spaces the core would trim, and one that carries no box, all write
/// nothing and leave the updated pair as it was.
///
/// Proves: Decision 134 with P2: a form that sends what it showed, what is already saved, or a name with a trailing space added, writes nothing and leaves the updated pair as it was.
#[tokio::test]
async fn change_account_that_changes_nothing_writes_nothing() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "nothing changed").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let before = chart_rows(&mut tx).await;
    let shown = sent("Savings", s.assets, None);
    for (wanted, label) in [
        (sent("Savings", s.assets, None), "what the form showed"),
        (sent("Savings ", s.assets, None), "a trailing space"),
        (sent("  Savings", s.assets, None), "a leading space"),
        (
            SentBoxes {
                name: Sent::Absent,
                inside: Sent::Absent,
                owner: Sent::Absent,
            },
            "no box",
        ),
    ] {
        let changed = change_account(&mut tx, s.sam, later(), s.savings, &shown, &wanted)
            .await
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(changed, Changed::Nothing, "{label}");
    }
    assert_eq!(chart_rows(&mut tx).await, before, "nothing was written");
    set_owner(&mut tx, s.admin, later(), s.savings, Some(s.sam))
        .await
        .unwrap_or_else(|error| panic!("the other save failed: {error}"));
    let after_other = chart_rows(&mut tx).await;
    let changed = change_account(
        &mut tx,
        s.sam,
        stamp(LATER_SECOND + 60),
        s.savings,
        &shown,
        &sent("Savings", s.assets, Some(s.sam)),
    )
    .await
    .unwrap_or_else(|error| panic!("what is already saved: {error}"));
    assert_eq!(changed, Changed::Nothing, "what is already saved");
    assert_eq!(
        chart_rows(&mut tx).await,
        after_other,
        "the updated pair is the other save's"
    );
    assert_sound(&mut tx, "nothing changed").await;
    roll_back(tx, "nothing changed").await;
    storage.close().await;
}

/// A rename and a move in one save are judged where the form ends: the name taken in the new
/// place with the new name, the depth and the account's own inside with the new place.
///
/// Proves: Decision 134 with P2: a rename and a move in one save are judged in the new place with the new name, so Assets:Spare renamed to Checking inside Assets:Savings passes; depth and into-itself use the new place.
#[tokio::test]
async fn change_account_judges_a_form_where_it_ends() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "where the form ends").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let spare = create(&mut tx, s.admin, "Spare", s.assets).await.id;
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later(),
            spare,
            &sent("Spare", s.assets, None),
            &sent("Checking", s.savings, None),
        )
        .await,
        "Spare renamed to Checking inside Savings",
    );
    assert_eq!(saved.name, "Checking");
    assert_eq!(saved.parent_id, Some(s.savings));
    let full = accounts::full_name(&mut tx, spare)
        .await
        .unwrap_or_else(|error| panic!("cannot name the account: {error}"));
    assert_eq!(full, "Assets:Savings:Checking");
    let other = create(&mut tx, s.admin, "Other spare", s.assets).await.id;
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            other,
            &sent("Other spare", s.assets, None),
            &sent("Holiday fund", s.savings, None),
        )
        .await,
        "a new name taken in the new place",
    );
    assert!(
        matches!(&refusals.name, Some(AccountError::NameTaken { holder, name, refused }) if holder == "Assets:Savings" && name == "Holiday fund" && refused.place.as_deref() == Some("Assets:Other spare") && refused.action == Action::Renamed),
        "{refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.loose_holiday,
            &sent("Holiday fund", s.assets, None),
            &sent("Holiday fund", s.savings, None),
        )
        .await,
        "a move alone into a place holding its name",
    );
    assert!(
        matches!(&refusals.name, Some(AccountError::NameTaken { holder, name, refused }) if holder == "Assets:Savings" && name == "Holiday fund" && refused.action == Action::Moved),
        "{refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &sent("Savings", s.assets, None),
            &sent("Savings", s.fifth, None),
        )
        .await,
        "Savings inside the fifth level",
    );
    assert!(
        matches!(
            &refusals.inside,
            Some(AccountError::TooDeep {
                depth: 7,
                inside: true,
                ..
            })
        ),
        "{refusals:?}"
    );
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &sent("Savings", s.assets, None),
            &sent("Deep savings", s.fourth, None),
        )
        .await,
        "Savings inside the fourth level",
    );
    assert_eq!(saved.parent_id, Some(s.fourth));
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &sent("Deep savings", s.fourth, None),
            &sent("Deep savings", s.holiday, None),
        )
        .await,
        "Savings inside its own Holiday fund",
    );
    assert!(
        matches!(&refusals.inside, Some(AccountError::MoveUnderDescendant { account, target, .. }) if account == "Assets:Level two:Level three:Level four:Deep savings" && target == "Assets:Level two:Level three:Level four:Deep savings:Holiday fund"),
        "{refusals:?}"
    );
    assert_sound(&mut tx, "where the form ends").await;
    roll_back(tx, "where the form ends").await;
    storage.close().await;
}

/// A save with a good new name and a refused place writes nothing; a save refused in two boxes
/// returns both refusals at once; each names the account by its saved full place.
///
/// Proves: Decisions 99 and 134 with P2 and P3: a good new name and a refused place write nothing, every refusal returned at once, each naming the account by its saved full place.
#[tokio::test]
async fn change_account_refused_in_its_second_box_writes_nothing() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "the second box refused").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let before = chart_rows(&mut tx).await;
    let shown = sent("Savings", s.assets, None);
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &shown,
            &sent("Rainy day", s.card, None),
        )
        .await,
        "a good name and a place in another section",
    );
    assert!(refusals.name.is_none(), "{refusals:?}");
    assert!(
        matches!(&refusals.inside, Some(AccountError::KindMismatch { account, section, .. }) if account == "Assets:Savings" && section == "Liabilities"),
        "{refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &shown,
            &sent("Checking", s.card, None),
        )
        .await,
        "a name taken where it stands and a place in another section",
    );
    assert!(
        refusals.name.is_none(),
        "the name is not judged in the old place when the new one is refused: {refusals:?}"
    );
    assert!(
        matches!(&refusals.inside, Some(AccountError::KindMismatch { .. })),
        "{refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.savings,
            &shown,
            &sent("Rainy: day", s.assets, Some(UserId(99))),
        )
        .await,
        "a bad name and an owner no user is",
    );
    assert!(
        matches!(&refusals.name, Some(AccountError::Name { refused, error: NameError::Colon }) if *refused == refused_at(Word::Account, "Assets:Savings", Action::Renamed)),
        "{refusals:?}"
    );
    assert!(refusals.inside.is_none(), "{refusals:?}");
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotListed { refused }) if *refused == refused_at(Word::Account, "Assets:Savings", Action::Changed)),
        "{refusals:?}"
    );
    assert_eq!(chart_rows(&mut tx).await, before, "nothing was written");
    assert_sound(&mut tx, "the second box refused").await;
    roll_back(tx, "the second box refused").await;
    storage.close().await;
}

/// The change form refuses, each in its sentence: a place no account is, a malformed one and a
/// built-in account as not in the list; another section; an archived place; the account itself
/// and a place inside it; a place too deep; an owner no user is, a malformed one and a
/// deactivated user who is not the saved owner; a section's change at the top; a built-in
/// category's move; an owner given to a category.
///
/// Proves: Decisions 134 and 136 with P3: an unknown, malformed or built-in place as not in the list, another section, an archived place, itself or inside it, too deep, and an owner outside the list, each in its sentence.
#[tokio::test]
async fn the_change_form_refuses_what_its_lists_do_not_offer() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "the change form's lists").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let retired = member(&mut tx, s.admin, "retired").await;
    test_support::deactivate_unchecked(&mut tx, retired, s.admin, later()).await;
    let before = chart_rows(&mut tx).await;
    let shown = sent("Savings", s.assets, None);
    let with_inside = |inside: Sent<AccountId>| SentBoxes {
        inside,
        ..sent("Savings", s.assets, None)
    };
    let not_listed = |error: &AccountError| matches!(error, AccountError::PlaceNotListed { refused } if refused.place.as_deref() == Some("Assets:Savings") && refused.action == Action::Moved);
    let cases: [InsideCase; 7] = [
        (
            with_inside(Sent::Value(AccountId(999))),
            not_listed,
            "a number no account has",
        ),
        (
            with_inside(Sent::Malformed),
            not_listed,
            "a malformed number",
        ),
        (
            with_inside(Sent::Value(s.liabilities)),
            |error| matches!(error, AccountError::KindMismatch { section, .. } if section == "Liabilities"),
            "another section",
        ),
        (
            with_inside(Sent::Value(s.old_savings)),
            |error| matches!(error, AccountError::ParentArchived { parent, .. } if parent == "Assets:Old savings"),
            "an archived place",
        ),
        (
            with_inside(Sent::Value(s.savings)),
            |error| matches!(error, AccountError::MoveUnderDescendant { account, target, .. } if account == target),
            "itself",
        ),
        (
            with_inside(Sent::Value(s.emergency)),
            |error| matches!(error, AccountError::MoveUnderDescendant { target, .. } if target == "Assets:Savings:Emergency fund"),
            "a place inside it",
        ),
        (
            with_inside(Sent::Value(s.fifth)),
            |error| {
                matches!(
                    error,
                    AccountError::TooDeep {
                        depth: 7,
                        inside: true,
                        ..
                    }
                )
            },
            "too deep",
        ),
    ];
    for (wanted, check, label) in cases {
        let refusals = change_refusals(
            change_account(&mut tx, s.sam, later(), s.savings, &shown, &wanted).await,
            label,
        );
        let inside = refusals
            .inside
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: nothing under Inside: {refusals:?}"));
        assert!(check(inside), "{label}: {inside:?}");
        assert!(
            refusals.name.is_none() && refusals.owner.is_none() && refusals.top.is_none(),
            "{label}: {refusals:?}"
        );
    }
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.groceries,
            &sent_category("Groceries", s.expenses),
            &sent_category("Groceries", s.uncategorized),
        )
        .await,
        "a built-in category",
    );
    assert!(
        matches!(&refusals.inside, Some(AccountError::PlaceNotListed { refused }) if *refused == refused_at(Word::Category, "Expenses:Groceries", Action::Moved)),
        "a built-in category: {refusals:?}"
    );
    for (owner, label) in [
        (Sent::Value(Some(UserId(99))), "a number no user has"),
        (Sent::Malformed, "a malformed owner"),
        (Sent::Value(Some(retired)), "a deactivated user"),
    ] {
        let wanted = SentBoxes {
            owner,
            ..sent("Savings", s.assets, None)
        };
        let refusals = change_refusals(
            change_account(&mut tx, s.sam, later(), s.savings, &shown, &wanted).await,
            label,
        );
        assert!(
            matches!(&refusals.owner, Some(AccountError::OwnerNotListed { refused }) if *refused == refused_at(Word::Account, "Assets:Savings", Action::Changed)),
            "{label}: {refusals:?}"
        );
    }
    let section_shown = SentBoxes {
        name: Sent::Value(String::from("Assets")),
        inside: Sent::Absent,
        owner: Sent::Absent,
    };
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.assets,
            &section_shown,
            &SentBoxes {
                name: Sent::Value(String::from("Our things")),
                ..section_shown.clone()
            },
        )
        .await,
        "a section renamed",
    );
    assert!(
        matches!(&refusals.top, Some(AccountError::Section { refused }) if *refused == refused_at(Word::Account, "Assets", Action::Changed)),
        "a section renamed: {refusals:?}"
    );
    assert!(
        refusals.name.is_none() && refusals.inside.is_none() && refusals.owner.is_none(),
        "a section renamed: {refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.uncategorized,
            &sent_category("Uncategorized", s.expenses),
            &sent_category("Uncategorized", s.groceries),
        )
        .await,
        "a built-in category moved",
    );
    assert!(
        matches!(&refusals.inside, Some(AccountError::BuiltIn { refused }) if *refused == refused_at(Word::Category, "Expenses:Uncategorized", Action::Moved)),
        "a built-in category moved: {refusals:?}"
    );
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.groceries,
            &sent("Groceries", s.expenses, None),
            &sent("Groceries", s.expenses, Some(s.sam)),
        )
        .await,
        "an owner given to a category",
    );
    assert!(
        matches!(&refusals.owner, Some(AccountError::OwnerNotAllowed { refused }) if *refused == refused_at(Word::Category, "Expenses:Groceries", Action::Changed)),
        "an owner given to a category: {refusals:?}"
    );
    assert_eq!(chart_rows(&mut tx).await, before, "nothing was written");
    assert_sound(&mut tx, "the change form's lists").await;
    roll_back(tx, "the change form's lists").await;
    storage.close().await;
}

/// A full change form shown before another save archived the account, saved with another place
/// or another owner, is refused at the top with nothing written, for an account and a category
/// alike; a save of its name alone is written, with the updated pair.
///
/// Proves: Decision 134 with P4: a full form saved after another save archived the account, changing its place or owner, is refused at the top with nothing written; its name alone is saved.
#[tokio::test]
async fn a_move_of_an_account_archived_meanwhile_is_refused_at_the_top() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "archived meanwhile").await;
    let s = scene(&mut tx, initialized.admin.id).await;
    let before = chart_rows(&mut tx).await;
    let shown = sent("Old savings", s.assets, None);
    for (wanted, label) in [
        (sent("Old savings", s.savings, None), "another place"),
        (sent("Old savings", s.assets, Some(s.sam)), "another owner"),
        (
            sent("Closed savings", s.checking, None),
            "a new name and another place",
        ),
    ] {
        let refusals = change_refusals(
            change_account(&mut tx, s.sam, later(), s.old_savings, &shown, &wanted).await,
            label,
        );
        assert!(
            matches!(&refusals.top, Some(AccountError::ArchivedMeanwhile { account, kind: Kind::Asset }) if account == "Assets:Old savings"),
            "{label}: {refusals:?}"
        );
        assert!(
            refusals.inside.is_none() && refusals.owner.is_none(),
            "{label}: {refusals:?}"
        );
    }
    let refusals = change_refusals(
        change_account(
            &mut tx,
            s.sam,
            later(),
            s.old_gym,
            &sent_category("Old gym", s.expenses),
            &sent_category("Old gym", s.groceries),
        )
        .await,
        "a category moved",
    );
    assert!(
        matches!(&refusals.top, Some(AccountError::ArchivedMeanwhile { account, kind: Kind::Expense }) if account == "Expenses:Old gym"),
        "a category moved: {refusals:?}"
    );
    assert_eq!(chart_rows(&mut tx).await, before, "nothing was written");
    let at = stamp(LATER_SECOND + 60);
    let saved = saved_by(
        change_account(
            &mut tx,
            s.sam,
            at,
            s.old_savings,
            &shown,
            &sent("Closed savings", s.assets, None),
        )
        .await,
        "the name alone",
    );
    assert_eq!(saved.name, "Closed savings");
    assert!(saved.is_archived(), "it stays archived");
    assert_eq!((saved.updated_by, saved.updated_at), (s.sam, at));
    assert_sound(&mut tx, "archived meanwhile").await;
    roll_back(tx, "archived meanwhile").await;
    storage.close().await;
}
