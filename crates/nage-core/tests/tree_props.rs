//! The property of SPEC 10.3's chart row (decision 98): random sequences of create, rename,
//! move, set-owner, archive and unarchive against a fresh database with the standard setup never
//! violate INV-8 to INV-11 (nor INV-12's tree half), every refusal is a domain refusal and
//! never a database error (the schema's constraints are never the first to refuse), and
//! `check` reports nothing at the end. proptest is synchronous, so each case runs on its own
//! current-thread runtime.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use nage_core::accounts::{
    self, Account, AccountError, AccountId, Kind, NewAccount, SystemKey, archive_account,
    create_account, move_account, rename_account, set_owner, unarchive_account,
};
use nage_core::storage::Storage;
use nage_core::test_support;
use nage_core::users::UserId;
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;

use common::{begin, block_on, check_report, later, reader, seeded};

/// One operation on the tree; indices are resolved against the accounts by id at replay time,
/// so every operation is total and the core does the refusing.
#[derive(Clone, Debug)]
enum Op {
    Create {
        parent: Index,
        name: String,
        owner: bool,
    },
    Rename {
        account: Index,
        name: String,
    },
    Move {
        account: Index,
        parent: Index,
    },
    SetOwner {
        account: Index,
        owner: bool,
    },
    Archive {
        account: Index,
    },
    Unarchive {
        account: Index,
    },
}

/// Names that collide in case, in white space, in compatibility characters and in the starter
/// chart, hold colons, or are empty, beside random short names.
const WORDS: &[&str] = &[
    "Cash",
    "cash",
    "CASH",
    "Bank",
    "bank",
    "Joint",
    "a:b",
    "x",
    "X",
    "",
    "Dining  Out",
    "Dining Out",
    "Caf\u{00E9}",
    "Cafe\u{0301}",
    "O\u{FB03}ce",
    "Office",
    "  Fees ",
    "Fees",
    "Other",
    "Groceries",
    "Stra\u{00DF}e",
    "STRASSE",
    "Food\u{FF1A}Fruit",
];

/// A name: a word from the list, a short random word, a word with a colon, or a short word
/// with white space around it.
fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => prop::sample::select(WORDS).prop_map(str::to_owned),
        2 => "[A-Za-z]{1,3}",
        1 => "[A-Za-z]{0,2}:[A-Za-z]{0,2}",
        1 => " ?[a-z]{1,2} ?",
    ]
}

/// One random operation, creations most often.
fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<Index>(), name(), any::<bool>())
            .prop_map(|(parent, name, owner)| Op::Create { parent, name, owner }),
        2 => (any::<Index>(), name()).prop_map(|(account, name)| Op::Rename { account, name }),
        2 => (any::<Index>(), any::<Index>()).prop_map(|(account, parent)| Op::Move { account, parent }),
        1 => (any::<Index>(), any::<bool>()).prop_map(|(account, owner)| Op::SetOwner { account, owner }),
        1 => any::<Index>().prop_map(|account| Op::Archive { account }),
        1 => any::<Index>().prop_map(|account| Op::Unarchive { account }),
    ]
}

/// Every account, by id.
async fn load(storage: &Storage) -> Result<Vec<Account>, TestCaseError> {
    let mut conn = reader(storage).await;
    accounts::accounts(&mut conn)
        .await
        .map_err(|error| TestCaseError::fail(format!("cannot load the accounts: {error}")))
}

/// Applies `op` in one write transaction, committed on success and dropped on a refusal: `None`
/// for a success, `Some(error)` for a domain refusal; a database error fails the case, since
/// the core must refuse before the schema does.
async fn apply(
    storage: &Storage,
    admin: UserId,
    accounts: &[Account],
    op: &Op,
) -> Result<Option<AccountError>, TestCaseError> {
    let pick = |index: &Index| accounts[index.index(accounts.len())].id;
    let mut tx = begin(storage, "an operation").await;
    let result = match op {
        Op::Create {
            parent,
            name,
            owner,
        } => {
            create_account(
                &mut tx,
                admin,
                later(),
                &NewAccount {
                    name: name.clone(),
                    parent: pick(parent),
                    owner: owner.then_some(admin),
                },
            )
            .await
        }
        Op::Rename { account, name } => {
            rename_account(&mut tx, admin, later(), pick(account), name).await
        }
        Op::Move { account, parent } => {
            move_account(&mut tx, admin, later(), pick(account), pick(parent)).await
        }
        Op::SetOwner { account, owner } => {
            set_owner(
                &mut tx,
                admin,
                later(),
                pick(account),
                owner.then_some(admin),
            )
            .await
        }
        Op::Archive { account } => archive_account(&mut tx, admin, later(), pick(account)).await,
        Op::Unarchive { account } => {
            unarchive_account(&mut tx, admin, later(), pick(account)).await
        }
    };
    match result {
        Ok(_) => {
            tx.commit()
                .await
                .map_err(|error| TestCaseError::fail(format!("cannot commit {op:?}: {error}")))?;
            Ok(None)
        }
        Err(AccountError::Database(error)) => Err(TestCaseError::fail(format!(
            "{op:?} was refused by the database, not by the core: {error}"
        ))),
        Err(refusal) => Ok(Some(refusal)),
    }
}

/// The oracle of INV-8 to INV-11 and INV-12's tree half over the loaded rows, independent of
/// `check`'s queries and of the fold (ASCII case-insensitivity is a necessary condition of
/// "ignoring case"): five roots, one per kind, each with its key and its name; every child's
/// kind equals its parent's and every parent chain reaches a root within six segments; no
/// colon in a name; no two siblings or roots whose names are equal ignoring ASCII case; the
/// eight keys present once each, no system account archived, the three non-roots under their
/// root; no live child of an archived parent; every name stored normalized.
fn oracle(accounts: &[Account]) -> Result<(), TestCaseError> {
    let by_id: BTreeMap<AccountId, &Account> = accounts
        .iter()
        .map(|account| (account.id, account))
        .collect();
    let roots: Vec<&Account> = accounts
        .iter()
        .filter(|account| account.is_root())
        .collect();
    prop_assert_eq!(roots.len(), 5, "roots: {:?}", roots);
    for kind in Kind::ALL {
        let of_kind: Vec<&&Account> = roots.iter().filter(|root| root.kind == kind).collect();
        prop_assert_eq!(of_kind.len(), 1, "roots of kind {}: {:?}", kind, of_kind);
        prop_assert_eq!(of_kind[0].system_key, Some(kind.root_key()));
        prop_assert_eq!(of_kind[0].name.as_str(), kind.root_name());
    }
    for account in accounts {
        if let Some(parent_id) = account.parent_id {
            let parent = by_id
                .get(&parent_id)
                .ok_or_else(|| TestCaseError::fail(format!("{account:?} has no parent row")))?;
            prop_assert_eq!(account.kind, parent.kind, "the kind of {:?}", account);
            prop_assert!(
                !(parent.is_archived() && !account.is_archived()),
                "{:?} is live under the archived {:?}",
                account,
                parent
            );
        }
        let mut depth = 1;
        let mut cursor = account;
        while let Some(parent_id) = cursor.parent_id {
            depth += 1;
            prop_assert!(depth <= 6, "{:?} is deeper than six", account);
            cursor = by_id
                .get(&parent_id)
                .ok_or_else(|| TestCaseError::fail(format!("{cursor:?} has no parent row")))?;
        }
        prop_assert!(!account.name.contains(':'), "{:?} holds a colon", account);
        prop_assert_eq!(
            account.name.trim(),
            account.name.as_str(),
            "{:?} is not trimmed",
            account
        );
        prop_assert!(
            !account.name.contains("  "),
            "{:?} holds two spaces",
            account
        );
        prop_assert!(!account.name.is_empty(), "{:?} has an empty name", account);
        prop_assert!(
            !account.name.chars().any(char::is_control),
            "{:?} holds a control character",
            account
        );
    }
    let mut seen: BTreeSet<(Option<AccountId>, String)> = BTreeSet::new();
    for account in accounts {
        let key = (account.parent_id, account.name.to_ascii_lowercase());
        prop_assert!(
            seen.insert(key),
            "{:?} shares its name with a sibling ignoring ASCII case",
            account
        );
    }
    for key in SystemKey::ALL {
        let holders: Vec<&Account> = accounts
            .iter()
            .filter(|account| account.system_key == Some(key))
            .collect();
        prop_assert_eq!(holders.len(), 1, "holders of {}: {:?}", key, holders);
        let holder = holders[0];
        prop_assert!(!holder.is_archived(), "{:?} is archived", holder);
        prop_assert_eq!(holder.kind, key.kind());
        match key.parent_key() {
            None => prop_assert!(holder.is_root(), "{:?} is not a root", holder),
            Some(parent_key) => {
                let parent = holder
                    .parent_id
                    .and_then(|id| by_id.get(&id))
                    .ok_or_else(|| TestCaseError::fail(format!("{holder:?} has no parent")))?;
                prop_assert_eq!(
                    parent.system_key,
                    Some(parent_key),
                    "the parent of {:?}",
                    holder
                );
            }
        }
    }
    Ok(())
}

/// Replays `ops` on a fresh database with the standard setup, checking the oracle after every
/// operation and `check` at the end.
async fn replay(ops: &[Op]) -> Result<(), TestCaseError> {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut accepted = 0_usize;
    for (index, op) in ops.iter().enumerate() {
        let before = load(&storage).await?;
        let outcome = apply(&storage, admin, &before, op).await?;
        if outcome.is_none() {
            accepted += 1;
        }
        let after = load(&storage).await?;
        oracle(&after).map_err(|error| {
            TestCaseError::fail(format!(
                "after operation {index} ({op:?}, outcome {outcome:?}): {error}"
            ))
        })?;
        if outcome.is_some() {
            prop_assert_eq!(
                &after,
                &before,
                "a refused operation changed the tree: {:?}",
                op
            );
        }
    }
    let report = check_report(&storage).await;
    prop_assert!(
        report.is_sound(),
        "check after {} operations ({} accepted): {:?}",
        ops.len(),
        accepted,
        report.violations()
    );
    storage.close().await;
    Ok(())
}

proptest! {
    #![proptest_config(test_support::proptest_config(file!()))]

    /// Random trees of up to forty operations never violate INV-8 to INV-11 or INV-12's tree
    /// half, a refused operation leaves no trace once its transaction is dropped, no refusal
    /// comes from the database, and `check` reports nothing at the end.
    ///
    /// Proves: INV-8 (Core half: the five keyed roots), INV-9, INV-10, INV-11 (Core halves),
    /// INV-12 (Core half, the tree rules), SPEC 10.3 (the chart row's property), decision 98.
    #[test]
    fn random_trees_never_violate_inv_8_to_11(ops in prop::collection::vec(op(), 1..=40)) {
        block_on(replay(&ops))?;
    }
}
