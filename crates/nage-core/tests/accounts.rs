//! The chart of accounts (SPEC 6.2, W2, the Core halves of INV-9 to INV-11 and the tree halves of
//! INV-8 and INV-12, decisions 95 and 96): create, rename, move, owner, archive, unarchive, their
//! refusals and their stamps, the reads by id, key and full name. Every database test works inside
//! one write transaction on the standard setup and rolls it back; postings, where a test needs
//! them, are inserted with unchecked SQL (decision 98).

mod common;

use nage_core::accounts::{
    self, Account, AccountError, AccountId, Kind, NewAccount, SystemKey, archive_account,
    create_account, move_account, rename_account, set_owner, unarchive_account,
};
use nage_core::fold::NameError;
use nage_core::storage::WriteTransaction;
use nage_core::users::{self, NewUser, UserId};
use sqlx::AssertSqlSafe;

use common::{
    LATER_SECOND, LATER_TEXT, NOW_TEXT, account_named, begin, insert_transaction, later, now,
    roll_back, seeded, stamp,
};

/// Asserts that `result` is the refusal whose `Debug` text is `expected`.
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(
    result: Result<T, AccountError>,
    expected: &str,
    label: &str,
) {
    match result {
        Ok(value) => panic!("{label} was accepted: {value:?}"),
        Err(error) => assert_eq!(format!("{error:?}"), expected, "{label}"),
    }
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

/// Creates a non-admin member `username` by `admin`.
async fn member(tx: &mut WriteTransaction, admin: UserId, username: &str) -> UserId {
    users::create_user(
        tx,
        Some(admin),
        later(),
        &NewUser {
            username: username.to_owned(),
            display_name: username.to_owned(),
            password_hash: String::from("hash"),
            is_admin: false,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the member {username}: {error}"))
    .id
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
        "TooDeep",
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
        ("", "Name(Empty)", "an empty name"),
        ("   ", "Name(Empty)", "a blank name"),
        (long.as_str(), "Name(TooLong)", "a 65-character name"),
        ("Food:Fruit", "Name(Colon)", "a colon"),
        ("Food\u{FF1A}Fruit", "Name(Colon)", "a fullwidth colon"),
        ("Food\tFruit", "Name(Control)", "a tab"),
        ("groceries", "NameTaken", "a sibling's name in lowercase"),
        ("GROCERIES", "NameTaken", "a sibling's name in capitals"),
        (
            "\u{FF27}roceries",
            "NameTaken",
            "a sibling's name with a fullwidth letter",
        ),
        (
            "\u{210D}ousing",
            "NameTaken",
            "a sibling's name with a double-struck capital",
        ),
        (
            " Groceries ",
            "NameTaken",
            "a sibling's name with white space around it",
        ),
    ] {
        assert_refused(
            create_account(&mut tx, admin, later(), &new(name, expenses)).await,
            expected,
            label,
        );
    }
    let dining_out = create(&mut tx, admin, "Dining  Out", expenses).await;
    assert_eq!(dining_out.name, "Dining Out");
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Dining Out", expenses)).await,
        "NameTaken",
        "the collapsed twin",
    );
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    archive_account(&mut tx, admin, later(), gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Gifts: {error}"));
    assert_refused(
        create_account(&mut tx, admin, later(), &new("gifts", expenses)).await,
        "NameTaken",
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
        "NoSuchParent",
        "an unknown parent",
    );
    let travel = account_named(&mut tx, "Expenses:Travel").await;
    archive_account(&mut tx, admin, later(), travel)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Travel: {error}"));
    assert_refused(
        create_account(&mut tx, admin, later(), &new("Flights", travel)).await,
        "ParentArchived",
        "an archived parent",
    );
    for (parent, label) in [(expenses, "an expense"), (equity, "an equity account")] {
        let with_owner = NewAccount {
            owner: Some(admin),
            ..new("Owned", parent)
        };
        assert_refused(
            create_account(&mut tx, admin, later(), &with_owner).await,
            "OwnerNotAllowed",
            &format!("an owner on {label}"),
        );
    }
    let unknown_owner = NewAccount {
        owner: Some(UserId(99)),
        ..new("Owned", assets)
    };
    assert_refused(
        create_account(&mut tx, admin, later(), &unknown_owner).await,
        "NoSuchUser",
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
        "RootRenamed",
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
        ("Housing", "NameTaken", "a sibling's name"),
        ("housing", "NameTaken", "a sibling's name in lowercase"),
        ("", "Name(Empty)", "an empty name"),
        ("a:b", "Name(Colon)", "a colon"),
    ] {
        assert_refused(
            rename_account(&mut tx, editor, later(), dining, name).await,
            expected,
            label,
        );
    }
    assert_refused(
        rename_account(&mut tx, editor, later(), AccountId(999), "Ghost").await,
        "NoSuchAccount",
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
        "MoveUnderDescendant",
        "a move under a child",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), food, food).await,
        "MoveUnderDescendant",
        "a move under itself",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), groceries, income).await,
        "KindMismatch",
        "a move to another kind",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), expenses, income).await,
        "RootMoved",
        "moving a root",
    );
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    assert_refused(
        move_account(&mut tx, admin, later(), uncategorized, food).await,
        "SystemAccountMoved",
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
        "ParentArchived",
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
        "TooDeep",
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
        "NoSuchParent",
        "an unknown new parent",
    );
    assert_refused(
        move_account(&mut tx, admin, later(), AccountId(999), food).await,
        "NoSuchAccount",
        "moving an unknown account",
    );
    create(&mut tx, admin, "Dining", food).await;
    assert_refused(
        move_account(&mut tx, admin, later(), dining, food).await,
        "NameTaken",
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
    for (account, label) in [
        (salary, "an income account"),
        (assets, "a root"),
        (opening, "a system account"),
    ] {
        assert_refused(
            set_owner(&mut tx, admin, later(), account, Some(admin)).await,
            "OwnerNotAllowed",
            &format!("an owner on {label}"),
        );
    }
    assert_refused(
        set_owner(&mut tx, admin, later(), checking, Some(UserId(99))).await,
        "NoSuchUser",
        "an unknown owner",
    );
    assert_refused(
        set_owner(&mut tx, admin, later(), AccountId(999), Some(admin)).await,
        "NoSuchAccount",
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
/// names the acting user), W2, decisions 96 and 98.
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
        "AlreadyArchived",
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
        "ChildrenNotArchived",
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
        "ParentArchivedUnarchiveFirst",
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
        "NotArchived",
        "unarchiving a live account",
    );
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    for (account, label) in [(expenses, "a root"), (uncategorized, "a system account")] {
        assert_refused(
            archive_account(&mut tx, admin, later(), account).await,
            "SystemAccountArchived",
            &format!("archiving {label}"),
        );
    }
    assert_refused(
        archive_account(&mut tx, admin, later(), AccountId(999)).await,
        "NoSuchAccount",
        "archiving an unknown account",
    );

    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let other = account_named(&mut tx, "Income:Other").await;
    insert_transaction(
        &mut tx,
        "2026-01-15",
        "manual",
        false,
        NOW_TEXT,
        &[(checking, 10_000), (salary, -10_000)],
    )
    .await;
    assert_refused(
        archive_account(&mut tx, admin, later(), checking).await,
        "BalanceNotZero",
        "an account with a live balance",
    );
    assert_refused(
        archive_account(&mut tx, admin, later(), salary).await,
        "BalanceNotZero",
        "a counter account with a live balance",
    );
    insert_transaction(
        &mut tx,
        "2026-02-15",
        "manual",
        false,
        NOW_TEXT,
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
        "NoSuchAccount",
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
        "MissingSystemAccount(Uncategorized)",
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
    let messages: Vec<String> = [
        AccountError::Name(NameError::Empty),
        AccountError::NameTaken,
        AccountError::NoSuchAccount,
        AccountError::NoSuchParent,
        AccountError::NoSuchUser,
        AccountError::ParentArchived,
        AccountError::RootRenamed,
        AccountError::RootMoved,
        AccountError::SystemAccountMoved,
        AccountError::SystemAccountArchived,
        AccountError::MoveUnderDescendant,
        AccountError::TooDeep,
        AccountError::KindMismatch,
        AccountError::OwnerNotAllowed,
        AccountError::ChildrenNotArchived,
        AccountError::BalanceNotZero,
        AccountError::AlreadyArchived,
        AccountError::NotArchived,
        AccountError::ParentArchivedUnarchiveFirst,
        AccountError::MissingSystemAccount(SystemKey::Uncategorized),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "the name is required",
            "an account with this name already exists here",
            "no such account",
            "the parent account does not exist",
            "no such user",
            "the parent account is archived",
            "a root account cannot be renamed",
            "a root account cannot be moved",
            "a system account cannot be moved",
            "a system account cannot be archived",
            "an account cannot be moved under itself or one of its descendants",
            "the chart allows at most six levels, the root included",
            "an account can only be moved within its kind",
            "only accounts under Assets or Liabilities have owners",
            "archive the child accounts first",
            "the account's balance is not zero",
            "the account is already archived",
            "the account is not archived",
            "unarchive the parent account first",
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
