//! The opening balance's pages over HTTP (SPEC 6.6, W2, INV-7, INV-13, INV-27, INV-28,
//! decisions 140 to 149): the four addresses behind the sign-in and the Origin check, the page for
//! an unknown address for every account that takes no opening balance, the address judged before
//! the body, an archived account's addresses, the form that enters and changes one with its
//! one-time message, every problem at once, a typed zero, the three clashes, an entry of another
//! account never touched, the Remove confirmation and a second removal, a busy write, today in the
//! configured zone, and the limit that keeps Home, the list and an account's page answering. The
//! router is driven in process with a clock the tests set; every scenario is built through the
//! core's operations or the pages themselves.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::accounts::{self, AccountId};
use nage_core::date::{Date, DateError};
use nage_core::money::{MinorDigits, Money, ParseMoneyError};
use nage_core::storage::StorageOptions;
use nage_core::transactions::{
    self, NewPosting, NewTransaction, OpeningBalance, OpeningBalanceEdit, OpeningBalanceError,
    OpeningBoxRefusal, OpeningClash,
};
use nage_core::users::UserId;
use nage_web::messages::{Notice, Refusal};

use common::app::{
    ADMIN, App, Client, DAY, HOUR, OTHER, OTHER_PASSWORD, Options, PASSWORD, Reply, Req,
    assert_security_headers, attribute, main_of,
};

/// A book set up with the starter accounts, and Sam, its admin, signed in.
async fn household() -> (App, Client) {
    household_with(Options::default()).await
}

/// A book set up with the starter accounts under `options`, and Sam signed in.
async fn household_with(options: Options) -> (App, Client) {
    let app = App::start_with(options).await;
    app.set_up_as(true).await;
    let sam = app.signed_in(ADMIN, PASSWORD).await;
    (app, sam)
}

/// The admin's id.
async fn admin(app: &App) -> UserId {
    app.credentials(ADMIN).await.user.id
}

/// The live opening balance of `account`, read through the core.
async fn live(app: &App, account: AccountId) -> Option<OpeningBalance> {
    let mut read = app.read().await;
    transactions::opening_balance(&mut read, account)
        .await
        .unwrap_or_else(|error| panic!("cannot read the opening balance of {account}: {error}"))
}

/// The live opening balance of `account`, which must exist.
async fn entered(app: &App, account: AccountId) -> OpeningBalance {
    live(app, account)
        .await
        .unwrap_or_else(|| panic!("{account} has no opening balance"))
}

/// Changes the opening balance of `account` to `minor` minor units on `on`, through the core,
/// as another user or another window would.
async fn change_elsewhere(app: &App, account: AccountId, on: &str, minor: i64) -> OpeningBalance {
    let saved = entered(app, account).await;
    let by = admin(app).await;
    let mut tx = app.write().await;
    let changed = transactions::edit_opening_balance(
        &mut tx,
        by,
        app.clock.now(),
        app.today(),
        &OpeningBalanceEdit {
            id: saved.id,
            expected_version: saved.version,
            date: date(on),
            amount: Money::from_minor(minor),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the opening balance of {account}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the change: {error}"));
    changed
}

/// Removes the opening balance of `account` through the core, as another window would.
async fn remove_elsewhere(app: &App, account: AccountId) {
    let saved = entered(app, account).await;
    let by = admin(app).await;
    let mut tx = app.write().await;
    transactions::remove_opening_balance(&mut tx, by, app.clock.now(), saved.id, saved.version)
        .await
        .unwrap_or_else(|error| panic!("cannot remove the opening balance of {account}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the removal: {error}"));
}

/// Archives `id` through the core, as another user or another window would.
async fn archive(app: &App, id: AccountId) {
    let by = admin(app).await;
    let mut tx = app.write().await;
    accounts::archive_account(&mut tx, by, app.clock.now(), id)
        .await
        .unwrap_or_else(|error| panic!("cannot archive {id}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the archive of {id}: {error}"));
}

/// The date `text`.
fn date(text: &str) -> Date {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is no date: {error}"))
}

/// The form's address of `account`.
fn form_path(account: AccountId) -> String {
    format!("/accounts/{account}/opening-balance")
}

/// The confirmation's address of `account`.
fn remove_path(account: AccountId) -> String {
    format!("/accounts/{account}/opening-balance/remove")
}

/// The page of `account`.
fn page_path(account: AccountId) -> String {
    format!("/accounts/{account}")
}

/// What a browser sends from the form in the page's main content that posts to `action`: every
/// hidden, text and date box with its value, and the tick box when it is ticked.
fn fields(reply: &Reply, action: &str) -> Vec<(String, String)> {
    let main = main_of(&reply.body);
    let needle = format!(r#"action="{action}""#);
    let start = main
        .find(&needle)
        .unwrap_or_else(|| panic!("no form posts to {action}:\n{main}"));
    let end = main[start..]
        .find("</form>")
        .map_or(main.len(), |index| index + start);
    let mut out = Vec::new();
    for tag in main[start..end].split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or_default();
        if !tag.starts_with("input ") {
            continue;
        }
        let name = attribute(tag, "name").unwrap_or_default();
        match attribute(tag, "type").as_deref() {
            Some("hidden" | "text" | "date") => {
                out.push((name, attribute(tag, "value").unwrap_or_default()));
            }
            Some("checkbox") if tag.contains(" checked") => {
                out.push((name, attribute(tag, "value").unwrap_or_default()));
            }
            _ => {}
        }
    }
    out
}

/// `sent` as the pairs a request takes.
fn pairs(sent: &[(String, String)]) -> Vec<(&str, &str)> {
    sent.iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

/// The value of the box called `name` in the main content of `reply`.
fn box_value(reply: &Reply, name: &str) -> Option<String> {
    let main = main_of(&reply.body);
    let needle = format!(r#"name="{name}""#);
    main.split('<')
        .map(|tag| tag.split('>').next().unwrap_or_default())
        .find(|tag| tag.starts_with("input ") && tag.contains(&needle))
        .map(|tag| attribute(tag, "value").unwrap_or_default())
}

/// Whether the tick box of `reply` is ticked.
fn ticked(reply: &Reply) -> bool {
    let main = main_of(&reply.body);
    main.split('<')
        .map(|tag| tag.split('>').next().unwrap_or_default())
        .any(|tag| tag.contains(r#"name="below_zero""#) && tag.contains(" checked"))
}

/// Asserts that `reply` is the page for an unknown address (decision 118).
fn assert_unknown(reply: &Reply, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "status of {what}:\n{}",
        reply.body
    );
    assert!(
        reply.shows(&Refusal::NotFound.to_string()),
        "{what} is not the page for an unknown address:\n{}",
        reply.body
    );
}

/// Asserts that `reply` is a 303 to `location`.
fn assert_see_other(reply: &Reply, location: &str, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of {what}:\n{}",
        reply.body
    );
    assert_eq!(reply.location(), Some(location), "where {what} leads");
}

/// Asserts that `reply` sets no one-time message.
fn assert_no_message(reply: &Reply, what: &str) {
    assert_eq!(
        reply.message_cookie(),
        None,
        "{what} sets a one-time message"
    );
}

/// Asserts that `reply` sets the one-time message `value`.
fn assert_message(reply: &Reply, value: &str, what: &str) {
    assert_eq!(
        reply.message_cookie().map(|(held, _)| held).as_deref(),
        Some(value),
        "the one-time message {what} sets"
    );
}

/// The form of an opening balance that enters `amount` on `on`.
fn entering<'a>(on: &'a str, amount: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![("date", on), ("amount", amount)]
}

/// Each of the four addresses leads a person without a session to the sign-in page; a POST
/// from another site is refused with 403 and writes nothing, beside the same request from
/// Nage's own page, which the route accepts; and every answer carries `Cache-Control:
/// no-store` with the other security headers. The form and the confirmation show a waiting
/// message of the older codes and clear it, and clear an account's message unseen.
///
/// Proves: INV-27 and decision 110 with A2: each of the four addresses without a session leads to the sign-in page, a POST from another site is refused with 403 beside a control the route accepts, and every answer carries Cache-Control no-store.
#[tokio::test]
async fn the_four_addresses_stand_behind_the_sign_in_and_the_origin_check() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let saved = entered(&app, checking).await;
    let entry = saved.id.to_string();
    let version = saved.version.to_string();
    let change = [
        ("entry", entry.as_str()),
        ("version", version.as_str()),
        ("date", "2026-09-30"),
        ("amount", "2,500.00"),
    ];
    let removal = [("entry", entry.as_str()), ("version", version.as_str())];

    let mut nobody = app.client();
    for reply in [
        nobody.get(&form_path(checking)).await,
        nobody.get(&remove_path(checking)).await,
        nobody.post(&form_path(checking), &change).await,
        nobody.post(&remove_path(checking), &removal).await,
    ] {
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status without a session:\n{}",
            reply.body
        );
        assert!(
            reply.location().is_some_and(|to| to.starts_with("/login")),
            "a request without a session does not lead to the sign-in page: {:?}",
            reply.location()
        );
        assert_security_headers(&reply, "an answer without a session");
    }

    for (path, form) in [
        (form_path(checking), &change[..]),
        (remove_path(checking), &removal[..]),
    ] {
        let reply = sam
            .send(Req::form(&path, form).header("origin", "http://elsewhere.test"))
            .await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "status of POST {path} from another site:\n{}",
            reply.body
        );
        assert_no_message(&reply, "a POST from another site");
        assert_security_headers(&reply, "a POST from another site");
    }
    assert_eq!(
        live(&app, checking).await,
        Some(saved),
        "the opening balance after the POSTs from another site"
    );

    // The form and the confirmation show a message of the older codes and clear it; an account's
    // message, even one about this account, they clear unseen (decision 146).
    for path in [form_path(checking), remove_path(checking)] {
        sam.message = Some(String::from("password-changed"));
        let page = sam.get(&path).await;
        assert_eq!(
            page.notices(),
            [(String::from("status"), Notice::PasswordChanged.to_string())],
            "{path} asked with an older message"
        );
        assert!(
            page.clears_the_message(),
            "{path} does not clear an older message"
        );
        sam.message = Some(format!("opening-balance-saved.{checking}"));
        let page = sam.get(&path).await;
        assert!(
            page.notices().is_empty(),
            "{path} shows an account's message: {:?}",
            page.notices()
        );
        assert!(
            page.clears_the_message(),
            "{path} does not clear an account's message unseen"
        );
    }
    sam.message = None;

    let form = sam.get(&form_path(checking)).await;
    assert_eq!(
        form.status,
        StatusCode::OK,
        "status of the form:\n{}",
        form.body
    );
    assert_security_headers(&form, "the form");
    let confirm = sam.get(&remove_path(checking)).await;
    assert_eq!(
        confirm.status,
        StatusCode::OK,
        "status of the confirmation:\n{}",
        confirm.body
    );
    assert_security_headers(&confirm, "the confirmation");
    let saved_reply = sam.post(&form_path(checking), &change).await;
    assert_see_other(
        &saved_reply,
        &page_path(checking),
        "the save from Nage's page",
    );
    assert_security_headers(&saved_reply, "the save");
    let changed = entered(&app, checking).await;
    let version = changed.version.to_string();
    let removed = sam
        .post(
            &remove_path(checking),
            &[("entry", entry.as_str()), ("version", version.as_str())],
        )
        .await;
    assert_see_other(
        &removed,
        &page_path(checking),
        "the removal from Nage's page",
    );
    assert_security_headers(&removed, "the removal");
    assert_eq!(
        live(&app, checking).await,
        None,
        "the opening balance after the removal"
    );
}

/// An unknown, malformed or percent-escaped number, a section, a built-in account, a category
/// and an archived category get the page for an unknown address at each of the four addresses,
/// GET and POST, also when the POST's body is no form, never the 303 of an archived account; the
/// page never prints the number a request chose.
///
/// Proves: Decisions 118, 128 and 140 with the hand check's H4: an unknown, malformed or percent-escaped number, a section, a built-in account, a category and an archived category, GET and POST, and a POST whose body is no form, answer the 404 page, the number never printed.
#[tokio::test]
async fn an_account_that_takes_no_opening_balance_gets_the_page_for_an_unknown_address() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let assets = app.account("Assets").await;
    let liabilities = app.account("Liabilities").await;
    let built_in = app.account("Equity:Opening Balances").await;
    let category = app.add_account("Expenses", "Hobbies", None).await;
    // An archived category takes no opening balance before it is archived: the page for an
    // unknown address, never the 303 of an archived account (the hand check's H4).
    let archived_category = app.add_account("Expenses", "Old hobby", None).await;
    archive(&app, archived_category).await;
    let mut segments = vec![
        String::from("987654"),
        String::from("abc"),
        String::from("-5"),
        String::from("+77"),
        format!("0{checking}"),
        checking
            .to_string()
            .bytes()
            .map(|byte| format!("%{byte:02X}"))
            .collect(),
        String::from("99999999999999999999999"),
    ];
    let printed = segments.clone();
    segments.extend(
        [assets, liabilities, built_in, category, archived_category].map(|id| id.to_string()),
    );
    let form = [("date", "2026-09-30"), ("amount", "12.00")];
    for segment in &segments {
        let form_address = format!("/accounts/{segment}/opening-balance");
        let remove_address = format!("/accounts/{segment}/opening-balance/remove");
        let replies = [
            (format!("GET {form_address}"), sam.get(&form_address).await),
            (
                format!("GET {remove_address}"),
                sam.get(&remove_address).await,
            ),
            (
                format!("POST {form_address}"),
                sam.post(&form_address, &form).await,
            ),
            (
                format!("POST {remove_address}"),
                sam.post(&remove_address, &[("entry", "1"), ("version", "1")])
                    .await,
            ),
            (
                format!("POST {form_address} with no form"),
                sam.send(
                    Req::new("POST", &form_address)
                        .header("origin", "http://nage.test")
                        .header("content-type", "text/plain")
                        .body(b"not a form".to_vec()),
                )
                .await,
            ),
            (
                format!("POST {remove_address} with no form"),
                sam.send(
                    Req::new("POST", &remove_address)
                        .header("origin", "http://nage.test")
                        .header("content-type", "application/json")
                        .body(b"{}".to_vec()),
                )
                .await,
            ),
        ];
        for (what, reply) in replies {
            assert_unknown(&reply, &what);
            assert_no_message(&reply, &what);
            if printed.contains(segment) {
                assert!(
                    !main_of(&reply.body).contains(segment.as_str()),
                    "{what} prints the number:\n{}",
                    reply.body
                );
            }
        }
    }
    for id in [assets, liabilities, built_in, category, archived_category] {
        assert_eq!(live(&app, id).await, None, "an opening balance of {id}");
    }
}

/// An archived account's form and confirmation answer 303 to its page, without an opening
/// balance and while it keeps one (its own postings brought to zero); a save and a removal
/// that meet an account archived after the form was shown answer 422 with the account's page
/// and the core's sentence at its top, nothing written and no message set.
///
/// Proves: Decision 140 with INV-12: an archived account's form and confirmation answer 303 to its page; a save or a removal that meets an account archived meanwhile answers 422 with its page and the sentence at its top, nothing written.
#[tokio::test]
async fn an_archived_accounts_addresses_lead_to_its_page() {
    let (app, mut sam) = household().await;
    let tin = app.add_account("Assets", "Old tin", None).await;
    archive(&app, tin).await;
    let reply = sam.get(&form_path(tin)).await;
    assert_see_other(
        &reply,
        &page_path(tin),
        "the form of an archived account without an opening balance",
    );
    let reply = sam.get(&remove_path(tin)).await;
    assert_see_other(
        &reply,
        &page_path(tin),
        "the confirmation of an archived account without an opening balance",
    );

    let old = app.add_account("Assets", "Old jar", None).await;
    // Old jar has an opening balance, its own postings brought to zero by a transfer, so that
    // its confirmation is refused for being archived and not for having nothing to remove.
    app.open_balance(old, "2026-09-30", 5_000).await;
    let checking = app.account("Assets:Checking").await;
    let by = admin(&app).await;
    let mut tx = app.write().await;
    transactions::create_transaction(
        &mut tx,
        by,
        app.clock.now(),
        &NewTransaction {
            date: date("2026-10-01"),
            postings: vec![
                NewPosting {
                    account: old,
                    amount: Money::from_minor(-5_000),
                },
                NewPosting {
                    account: checking,
                    amount: Money::from_minor(5_000),
                },
            ],
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot move Old jar's balance: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the transfer: {error}"));
    archive(&app, old).await;
    assert!(
        live(&app, old).await.is_some(),
        "the archived Old jar keeps its opening balance"
    );
    let reply = sam.get(&form_path(old)).await;
    assert_see_other(&reply, &page_path(old), "the form of an archived account");
    let reply = sam.get(&remove_path(old)).await;
    assert_see_other(
        &reply,
        &page_path(old),
        "the confirmation of an archived account",
    );

    let spare = app.add_account("Assets", "Spare", None).await;
    let form = sam.get(&form_path(spare)).await;
    assert_eq!(
        form.status,
        StatusCode::OK,
        "status of the form:\n{}",
        form.body
    );
    archive(&app, spare).await;
    let sentence = OpeningBalanceError::ArchivedMeanwhile {
        account: String::from("Assets:Spare"),
    }
    .to_string();
    for (what, reply) in [
        (
            "the save",
            sam.post(&form_path(spare), &entering("2026-09-30", "12.00"))
                .await,
        ),
        (
            "the removal",
            sam.post(&remove_path(spare), &[("entry", "1"), ("version", "1")])
                .await,
        ),
    ] {
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of {what} on an account archived meanwhile:\n{}",
            reply.body
        );
        assert!(
            reply.notices() == [(String::from("alert"), sentence.clone())]
                && main_of(&reply.body).contains("<h1>Spare</h1>"),
            "{what} does not answer the account's page with the sentence at its top:\n{}",
            reply.body
        );
        assert_no_message(&reply, what);
    }
    assert_eq!(live(&app, spare).await, None, "an opening balance of Spare");
}

/// The form that enters an opening balance saves it in one write and answers 303 to the
/// account's page with the message's code and the account's number; the page shows The opening
/// balance was saved. once, then the fact with its figure and Change opening balance.
///
/// Proves: Decisions 140, 141 and 146 with INV-28: a save is one write and a 303 to the account's page with the message code and the number, shown once; the page then shows the fact and Change opening balance.
#[tokio::test]
async fn the_form_enters_an_opening_balance_and_the_page_says_so_once() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let reply = sam
        .post(&form_path(checking), &entering("2026-09-30", " 2,480.15 "))
        .await;
    assert_see_other(&reply, &page_path(checking), "the save");
    assert_message(
        &reply,
        &format!("opening-balance-saved.{checking}"),
        "the save",
    );
    let saved = entered(&app, checking).await;
    assert_eq!(
        (saved.date, saved.amount, saved.version),
        (date("2026-09-30"), Money::from_minor(248_015), 1),
        "the opening balance saved"
    );
    let page = sam.get(&page_path(checking)).await;
    assert_eq!(
        page.notices(),
        [(
            String::from("status"),
            Notice::OpeningBalanceSaved.to_string()
        )],
        "the account's page after the save:\n{}",
        page.body
    );
    assert!(
        page.clears_the_message(),
        "the page does not clear the message"
    );
    let main = main_of(&page.body);
    assert!(
        main.contains("<dt>Opening balance</dt><dd><span class=\"amount\">2,480.15</span> <span class=\"on\">on 2026-09-30</span></dd>")
            && main.contains(">Change opening balance</a>")
            && !main.contains("Enter opening balance"),
        "the page does not show the fact and Change opening balance:\n{main}"
    );
    let again = sam.get(&page_path(checking)).await;
    assert!(again.notices().is_empty(), "the message shows twice");
}

/// A second user, signed in through the sign-in page, enters an opening balance through the
/// form, changes it through the change form an hour later and removes it through the
/// confirmation an hour after that: the created pair, the updated pair and the deleted pair each
/// name that user at the instant of that request, never the admin who set the book up, so the
/// web hands the core the user of the session it signed in.
///
/// Proves: INV-26 with decisions 140 and 143 and the hand check's H3: created_by, updated_by and deleted_by of an opening balance saved, changed and removed through the pages by a second user name that user and not the first.
#[tokio::test]
async fn the_stamps_name_the_second_user_who_saves_changes_and_removes() {
    let (app, _sam) = household().await;
    let first = admin(&app).await;
    let second = app.add_user(OTHER, OTHER_PASSWORD).await.id;
    assert_ne!(first, second, "the two users");
    let mut alex = app.signed_in(OTHER, OTHER_PASSWORD).await;
    let checking = app.account("Assets:Checking").await;
    let stamps = |found: &transactions::Transaction| {
        (
            found.created_by,
            found.created_at,
            found.updated_by,
            found.updated_at,
            found.deleted_by,
            found.deleted_at,
        )
    };
    let stored = |id| {
        let app = &app;
        async move {
            let mut read = app.read().await;
            transactions::transaction(&mut read, id)
                .await
                .unwrap_or_else(|error| panic!("cannot read the entry {id}: {error}"))
                .unwrap_or_else(|| panic!("no entry {id}"))
        }
    };

    let entered_at = app.clock.now();
    let reply = alex
        .post(&form_path(checking), &entering("2026-09-30", "2,480.15"))
        .await;
    assert_see_other(&reply, &page_path(checking), "the second user's save");
    let saved = entered(&app, checking).await;
    assert_eq!(
        stamps(&stored(saved.id).await),
        (second, entered_at, second, entered_at, None, None),
        "the stamps of the entry the second user saved"
    );

    app.clock.advance(HOUR);
    let changed_at = app.clock.now();
    let form = alex.get(&form_path(checking)).await;
    let mut sent = fields(&form, &form_path(checking));
    for (name, value) in &mut sent {
        if name == "amount" {
            *value = String::from("2,500.00");
        }
    }
    let reply = alex.post(&form_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "the second user's change");
    let changed = entered(&app, checking).await;
    assert_eq!(
        (changed.id, changed.amount, changed.version),
        (saved.id, Money::from_minor(250_000), 2),
        "the entry after the second user's change"
    );
    assert_eq!(
        stamps(&stored(saved.id).await),
        (second, entered_at, second, changed_at, None, None),
        "the stamps of the entry the second user changed"
    );

    app.clock.advance(HOUR);
    let removed_at = app.clock.now();
    let confirmation = alex.get(&remove_path(checking)).await;
    assert_eq!(
        confirmation.status,
        StatusCode::OK,
        "status of the confirmation:\n{}",
        confirmation.body
    );
    let sent = fields(&confirmation, &remove_path(checking));
    let reply = alex.post(&remove_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "the second user's removal");
    assert_eq!(
        live(&app, checking).await,
        None,
        "the entry after the removal"
    );
    let removed = stored(saved.id).await;
    assert_eq!(
        stamps(&removed),
        (
            second,
            entered_at,
            second,
            changed_at,
            Some(second),
            Some(removed_at)
        ),
        "the stamps of the entry the second user removed"
    );
    assert_ne!(
        (removed.created_by, removed.updated_by, removed.deleted_by),
        (first, first, Some(first)),
        "a stamp names the admin"
    );
}

/// The change form holds what is saved, an amount below zero without its minus and with the
/// box ticked, names the entry and the version shown and offers Remove, outlined beside the one
/// yellow Save, and Back to the account's page; a change raises the version by one and sets its
/// message; a save of what is saved, even naming a version no longer saved, writes nothing and
/// sets no message. For a loan under Liabilities inside another account the form, the change
/// form, the confirmation and a clash page name what holds it in the trail and ask for the
/// amount owed. In a book without decimals and in one with three the change form holds the
/// saved amount in the book's digits, and sending it as it stands writes nothing.
///
/// Proves: Decisions 100 and 141 with INV-7: the change form holds what is saved, an amount below zero without its minus and ticked, with Remove; a change raises the version by one; a save of what is saved writes nothing and sets no message.
#[tokio::test]
async fn the_change_form_shows_what_is_saved_and_changes_it() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", -3_520).await;
    let saved = entered(&app, checking).await;
    let form = sam.get(&form_path(checking)).await;
    assert_eq!(
        form.status,
        StatusCode::OK,
        "status of the change form:\n{}",
        form.body
    );
    assert_eq!(
        (
            box_value(&form, "date"),
            box_value(&form, "amount"),
            ticked(&form),
            box_value(&form, "entry"),
            box_value(&form, "version"),
        ),
        (
            Some(String::from("2026-09-30")),
            Some(String::from("35.20")),
            true,
            Some(saved.id.to_string()),
            Some(String::from("1")),
        ),
        "the change form's boxes:\n{}",
        form.body
    );
    assert!(
        form.shows("Change opening balance")
            && main_of(&form.body).contains(&format!(
                r#"<a class="button outline apart" href="{}">Remove opening balance</a>"#,
                remove_path(checking)
            )),
        "the change form does not offer Remove outlined, its one yellow button Save:\n{}",
        form.body
    );
    assert!(
        main_of(&form.body).contains(&format!(
            r#"<a class="back" href="{}">Back to Checking</a>"#,
            page_path(checking)
        )),
        "Back to does not lead to the account's page:\n{}",
        form.body
    );

    let mut sent = fields(&form, &form_path(checking));
    for (name, value) in &mut sent {
        if name == "amount" {
            *value = String::from("40.00");
        }
    }
    let reply = sam.post(&form_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "the change");
    assert_message(
        &reply,
        &format!("opening-balance-saved.{checking}"),
        "the change",
    );
    let changed = entered(&app, checking).await;
    assert_eq!(
        (changed.id, changed.amount, changed.version),
        (saved.id, Money::from_minor(-4_000), 2),
        "the opening balance after the change"
    );

    let reply = sam.post(&form_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "a save of what is saved");
    assert_no_message(&reply, "a save of what is saved");
    assert_eq!(
        live(&app, checking).await,
        Some(changed),
        "the opening balance after a save of what is saved"
    );

    // An account under Liabilities inside another: every page names what holds it in the trail
    // and words the boxes for what is owed.
    app.add_account("Liabilities", "Loans", None).await;
    let car = app.add_account("Liabilities:Loans", "Car loan", None).await;
    let owed = |reply: &Reply, what: &str| {
        assert!(
            reply.shows("Amount owed")
                && reply.shows("The account was in credit (the amount was owed to you)")
                && !reply.shows("The account was overdrawn")
                && !main_of(&reply.body).contains(">Balance<"),
            "{what} of a loan is not worded for what is owed:\n{}",
            reply.body
        );
    };
    let form = sam.get(&form_path(car)).await;
    assert_eq!(form.status, StatusCode::OK, "status of the loan's form");
    owed(&form, "the form");
    assert!(
        form.shows("What the statement says you owed at the end of that day."),
        "the loan's form does not carry the help line for what is owed:\n{}",
        form.body
    );
    assert_eq!(
        trail(&form),
        ["Accounts", "Loans", "Car loan", "Opening balance"],
        "the trail of the loan's form"
    );
    app.open_balance(car, "2026-09-30", 120_000).await;
    let form = sam.get(&form_path(car)).await;
    owed(&form, "the change form");
    assert_eq!(
        box_value(&form, "amount"),
        Some(String::from("1,200.00")),
        "the loan's change form"
    );
    assert_eq!(
        trail(&form),
        ["Accounts", "Loans", "Car loan", "Opening balance"],
        "the trail of the loan's change form"
    );
    let confirm = sam.get(&remove_path(car)).await;
    assert_eq!(
        trail(&confirm),
        ["Accounts", "Loans", "Car loan", "Remove opening balance"],
        "the trail of the loan's confirmation"
    );
    let stale = fields(&form, &form_path(car));
    change_elsewhere(&app, car, "2026-09-30", 125_000).await;
    let clash = sam.post(&form_path(car), &pairs(&stale)).await;
    assert_eq!(
        clash.status,
        StatusCode::CONFLICT,
        "status of the loan's clash:\n{}",
        clash.body
    );
    owed(&clash, "the clash page");
    assert_eq!(
        trail(&clash),
        ["Accounts", "Loans", "Car loan", "Opening balance"],
        "the trail of the loan's clash page"
    );

    // In a book without decimals and in one with three, the change form holds the saved amount
    // in the book's digits, and sending it as it stands writes nothing.
    for (code, minor, shown) in [("JPY", 150_000, "150,000"), ("KWD", 1_500_250, "1,500.250")] {
        let app = App::start_with(Options::default()).await;
        app.set_up_in(code, true).await;
        let mut sam = app.signed_in(ADMIN, PASSWORD).await;
        let checking = app.account("Assets:Checking").await;
        app.open_balance(checking, "2026-09-30", minor).await;
        let form = sam.get(&form_path(checking)).await;
        assert_eq!(
            box_value(&form, "amount"),
            Some(String::from(shown)),
            "{code}: the change form's amount:\n{}",
            form.body
        );
        let sent = fields(&form, &form_path(checking));
        let reply = sam.post(&form_path(checking), &pairs(&sent)).await;
        assert_see_other(&reply, &page_path(checking), code);
        assert_no_message(&reply, code);
        assert_eq!(
            entered(&app, checking).await.amount,
            Money::from_minor(minor),
            "{code}: the change form sent as it stands"
        );
    }
}

/// The steps of the trail of `reply`, their words in order.
fn trail(reply: &Reply) -> Vec<String> {
    let main = main_of(&reply.body);
    let Some(start) = main.find(r#"<nav class="trail""#) else {
        return Vec::new();
    };
    let end = main[start..]
        .find("</nav>")
        .map_or(main.len(), |at| at + start);
    main[start..end]
        .split("<li>")
        .skip(1)
        .map(|step| {
            let mut text = String::new();
            let mut inside = false;
            for ch in step.chars() {
                match ch {
                    '<' => inside = true,
                    '>' => inside = false,
                    _ if !inside => text.push(ch),
                    _ => {}
                }
            }
            text
        })
        .collect()
}

/// A form refused in both boxes answers 422 with the form's line at the top, each problem in
/// the core's words under its box and what was typed kept; a minus with the box ticked is
/// refused with the box kept ticked; nothing is written and no message is set. A refused change
/// form comes back as the change form, Remove and the entry and version it names kept, so that
/// the corrected form changes the opening balance.
///
/// Proves: Decision 145 with INV-28: 422 with both problems under their boxes, the form's line at the top, what was typed and the tick kept, a minus with the tick refused, no row and no message cookie.
#[tokio::test]
async fn a_refused_form_shows_every_problem_and_writes_nothing() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let reply = sam
        .post(&form_path(checking), &entering("", "1.234,56"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of the refused form:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            Refusal::OpeningBalanceNotSaved.to_string()
        )],
        "the line at the top"
    );
    for refusal in [
        OpeningBoxRefusal::Date(DateError::Empty),
        OpeningBoxRefusal::Amount {
            error: ParseMoneyError::MisplacedComma,
            digits: MinorDigits::TWO,
        },
    ] {
        assert!(
            main_of(&reply.body).contains(&format!(
                r#"<small class="problem">{}</small>"#,
                refusal.to_string().replace('\'', "&#39;")
            )),
            "the problem {refusal:?} is not under its box:\n{}",
            reply.body
        );
    }
    assert_eq!(
        (
            box_value(&reply, "date"),
            box_value(&reply, "amount"),
            ticked(&reply)
        ),
        (Some(String::new()), Some(String::from("1.234,56")), false),
        "what the refused form keeps"
    );
    assert_no_message(&reply, "the refused form");

    let reply = sam
        .post(
            &form_path(checking),
            &[
                ("date", "2026-09-30"),
                ("amount", "-35.20"),
                ("below_zero", "1"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of two signs:\n{}",
        reply.body
    );
    assert!(
        reply.shows(&OpeningBoxRefusal::TwoSigns.to_string()),
        "two signs are not refused under the amount box:\n{}",
        reply.body
    );
    assert_eq!(
        (box_value(&reply, "amount"), ticked(&reply)),
        (Some(String::from("-35.20")), true),
        "what the form refused for two signs keeps"
    );
    assert_no_message(&reply, "two signs");
    assert_eq!(
        live(&app, checking).await,
        None,
        "an opening balance after the refusals"
    );

    let cash = app.account("Assets:Cash").await;
    app.open_balance(cash, "2026-09-30", 4_000).await;
    let saved = entered(&app, cash).await;
    let form = sam.get(&form_path(cash)).await;
    let mut sent = fields(&form, &form_path(cash));
    for (name, value) in &mut sent {
        if name == "amount" {
            *value = String::from("1.234,56");
        }
    }
    let reply = sam.post(&form_path(cash), &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of the refused change form:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains("<h1>Change opening balance</h1>")
            && main_of(&reply.body).contains(&format!(
                r#"href="{}">Remove opening balance</a>"#,
                remove_path(cash)
            )),
        "the refused change form does not come back as the change form:\n{}",
        reply.body
    );
    assert_eq!(
        (box_value(&reply, "entry"), box_value(&reply, "version")),
        (Some(saved.id.to_string()), Some(saved.version.to_string())),
        "the refused change form keeps the entry and the version it names:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the refused change form");
    assert_eq!(
        live(&app, cash).await,
        Some(saved),
        "after the refused change"
    );
    let mut corrected = fields(&reply, &form_path(cash));
    for (name, value) in &mut corrected {
        if name == "amount" {
            *value = String::from("45.00");
        }
    }
    let reply = sam.post(&form_path(cash), &pairs(&corrected)).await;
    assert_see_other(&reply, &page_path(cash), "the corrected change");
    assert_message(
        &reply,
        &format!("opening-balance-saved.{cash}"),
        "the corrected change",
    );
    assert_eq!(
        entered(&app, cash).await.amount,
        Money::from_minor(4_500),
        "the corrected change saved"
    );
}

/// Save with an amount of 0 on an account that has an opening balance answers the Remove
/// confirmation with the line You typed 0, writing nothing, also with an empty date box; its
/// Remove then removes the opening balance. On an account without one, a zero is refused under
/// the amount box with 422.
///
/// Proves: Decision 143 with the pause's P2: Save with 0 on an account that has one answers the confirmation with You typed 0 and writes nothing, also with an empty date box, and Remove there removes it; on an account without one the zero is refused under the amount box with 422.
#[tokio::test]
async fn a_typed_zero_asks_to_remove_it() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let saved = entered(&app, checking).await;
    let entry = saved.id.to_string();
    let typed_zero = "You typed 0: an account that starts at zero needs no opening balance.";
    let mut confirmation = None;
    for on in ["2026-09-30", ""] {
        let reply = sam
            .post(
                &form_path(checking),
                &[
                    ("entry", entry.as_str()),
                    ("version", "1"),
                    ("date", on),
                    ("amount", "0"),
                ],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "status of a typed zero with the date {on:?}:\n{}",
            reply.body
        );
        assert!(
            reply.shows(typed_zero)
                && reply.shows("Remove the opening balance?")
                && reply.shows("2,480.15"),
            "a typed zero does not ask to remove the opening balance:\n{}",
            reply.body
        );
        assert_no_message(&reply, "a typed zero");
        assert_eq!(
            live(&app, checking).await,
            Some(saved),
            "after a typed zero"
        );
        confirmation = Some(reply);
    }
    let confirmation = confirmation.unwrap_or_else(|| panic!("no confirmation"));
    let sent = fields(&confirmation, &remove_path(checking));
    let reply = sam.post(&remove_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "Remove after a typed zero");
    assert_message(
        &reply,
        &format!("opening-balance-removed.{checking}"),
        "Remove after a typed zero",
    );
    assert_eq!(live(&app, checking).await, None, "after Remove");

    let cash = app.account("Assets:Cash").await;
    let reply = sam
        .post(&form_path(cash), &entering("2026-09-30", "0.00"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a zero on an account without one:\n{}",
        reply.body
    );
    assert!(
        reply.shows(&OpeningBoxRefusal::Zero.to_string()),
        "a zero is not refused under the amount box:\n{}",
        reply.body
    );
    assert_no_message(&reply, "a refused zero");
    assert_eq!(live(&app, cash).await, None, "an opening balance of Cash");
}

/// Changed, removed and entered meanwhile each answer 409 with the sentence, what is saved now
/// and the boxes and the tick as sent, nothing written, Keep what is saved and Keep it removed
/// outlined and leading to the account's page; Save mine and Save as a new opening balance, sent
/// from those pages, then save.
///
/// Proves: Decision 144 with INV-28: changed, removed and entered meanwhile each answer 409 with what is saved now and the boxes and the tick as sent, nothing written; Save mine and Save as a new opening balance then save.
#[tokio::test]
async fn the_three_clashes_answer_409_with_what_is_saved() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let shown = entered(&app, checking).await;
    let now = change_elsewhere(&app, checking, "2026-09-30", 250_000).await;
    let entry = shown.id.to_string();
    let reply = sam
        .post(
            &form_path(checking),
            &[
                ("entry", entry.as_str()),
                ("version", "1"),
                ("date", "2026-09-29"),
                ("amount", "2,480.15"),
                ("below_zero", "1"),
            ],
        )
        .await;
    let sentence = OpeningClash::Changed {
        account: String::from("Assets:Checking"),
        saved: now,
        digits: MinorDigits::TWO,
    }
    .to_string();
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of changed meanwhile:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(String::from("alert"), sentence)],
        "the sentence of changed meanwhile"
    );
    assert!(
        reply.shows("Saved now") && reply.shows("2,500.00") && reply.shows("Save mine"),
        "changed meanwhile does not show what is saved now:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<a class="button outline" href="{}">Keep what is saved</a>"#,
            page_path(checking)
        )),
        "Keep what is saved is not outlined or does not lead to the account's page:\n{}",
        reply.body
    );
    assert_eq!(
        (
            box_value(&reply, "date"),
            box_value(&reply, "amount"),
            ticked(&reply)
        ),
        (
            Some(String::from("2026-09-29")),
            Some(String::from("2,480.15")),
            true
        ),
        "the boxes of changed meanwhile"
    );
    assert_no_message(&reply, "changed meanwhile");
    assert_eq!(
        live(&app, checking).await,
        Some(now),
        "after changed meanwhile"
    );
    let mine = fields(&reply, &form_path(checking));
    let reply = sam.post(&form_path(checking), &pairs(&mine)).await;
    assert_see_other(&reply, &page_path(checking), "Save mine after a change");
    let saved = entered(&app, checking).await;
    assert_eq!(
        (saved.date, saved.amount, saved.version),
        (date("2026-09-29"), Money::from_minor(-248_015), 3),
        "the opening balance after Save mine"
    );

    let cash = app.account("Assets:Cash").await;
    app.open_balance(cash, "2026-09-30", 4_000).await;
    let shown = entered(&app, cash).await.id.to_string();
    remove_elsewhere(&app, cash).await;
    let reply = sam
        .post(
            &form_path(cash),
            &[
                ("entry", shown.as_str()),
                ("version", "1"),
                ("date", "2026-09-30"),
                ("amount", "40.00"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of removed meanwhile:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            OpeningClash::Removed {
                account: String::from("Assets:Cash")
            }
            .to_string()
        )],
        "the sentence of removed meanwhile"
    );
    assert!(
        reply.shows("No opening balance") && reply.shows("Save as a new opening balance"),
        "removed meanwhile does not say there is none:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<a class="button outline" href="{}">Keep it removed</a>"#,
            page_path(cash)
        )),
        "Keep it removed is not outlined or does not lead to the account's page:\n{}",
        reply.body
    );
    assert_no_message(&reply, "removed meanwhile");
    assert_eq!(live(&app, cash).await, None, "after removed meanwhile");
    let anew = fields(&reply, &form_path(cash));
    let reply = sam.post(&form_path(cash), &pairs(&anew)).await;
    assert_see_other(&reply, &page_path(cash), "Save as a new opening balance");
    let saved = entered(&app, cash).await;
    assert_eq!(
        (saved.amount, saved.version),
        (Money::from_minor(4_000), 1),
        "the opening balance saved anew"
    );

    let savings = app.account("Assets:Savings").await;
    app.open_balance(savings, "2026-09-30", 248_015).await;
    let now = entered(&app, savings).await;
    let reply = sam
        .post(&form_path(savings), &entering("2026-09-30", "2,450.00"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of entered meanwhile:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            OpeningClash::Entered {
                account: String::from("Assets:Savings"),
                saved: now,
                digits: MinorDigits::TWO,
            }
            .to_string()
        )],
        "the sentence of entered meanwhile"
    );
    assert_eq!(
        box_value(&reply, "amount"),
        Some(String::from("2,450.00")),
        "the amount entered meanwhile keeps"
    );
    assert_no_message(&reply, "entered meanwhile");
    assert_eq!(
        live(&app, savings).await,
        Some(now),
        "after entered meanwhile"
    );
    let mine = fields(&reply, &form_path(savings));
    let reply = sam.post(&form_path(savings), &pairs(&mine)).await;
    assert_see_other(&reply, &page_path(savings), "Save mine after an entry");
    let saved = entered(&app, savings).await;
    assert_eq!(
        (saved.id, saved.amount, saved.version),
        (now.id, Money::from_minor(245_000), 2),
        "the opening balance after Save mine"
    );
}

/// At one account's address, a save or a removal naming another account's entry, or an entry
/// replaced meanwhile, answers 409 with that account's own figure while it has one, and as
/// removed meanwhile and nothing to do while it has none; no entry is changed or removed and
/// no message is set.
///
/// Proves: Decision 144 with the pause's P2: at one account's address, a save or a removal naming another account's entry or a replaced entry answers 409 with that account's own figure while it has one, and as removed meanwhile and nothing to do while it has none; no entry of either account is changed or removed and no message is set.
#[tokio::test]
async fn an_entry_of_another_account_is_never_changed_or_removed() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let cash = app.account("Assets:Cash").await;
    let savings = app.account("Assets:Savings").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    app.open_balance(cash, "2026-09-30", 4_000).await;
    let own = entered(&app, checking).await;
    let other = entered(&app, cash).await;
    let entry = other.id.to_string();
    let version = other.version.to_string();
    let named_save = [
        ("entry", entry.as_str()),
        ("version", version.as_str()),
        ("date", "2026-09-01"),
        ("amount", "1.00"),
    ];
    let named_removal = [("entry", entry.as_str()), ("version", version.as_str())];

    let reply = sam.post(&form_path(checking), &named_save).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of a save naming Cash's entry:\n{}",
        reply.body
    );
    assert!(
        reply.shows("2,480.15") && !reply.shows("40.00"),
        "the save naming Cash's entry does not show Checking's own figure:\n{}",
        reply.body
    );
    assert_no_message(&reply, "a save naming another entry");
    let reply = sam.post(&remove_path(checking), &named_removal).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of a removal naming Cash's entry:\n{}",
        reply.body
    );
    assert!(
        reply.shows("2,480.15") && !reply.shows("40.00"),
        "the removal naming Cash's entry does not show Checking's own figure:\n{}",
        reply.body
    );
    assert_eq!(
        box_value(&reply, "entry"),
        Some(own.id.to_string()),
        "the confirmation shown again names Checking's own entry"
    );
    assert_no_message(&reply, "a removal naming another entry");

    let reply = sam.post(&form_path(savings), &named_save).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of a save at Savings:\n{}",
        reply.body
    );
    assert!(
        reply.shows("No opening balance") && !reply.shows("40.00"),
        "a save naming Cash's entry at Savings is not removed meanwhile:\n{}",
        reply.body
    );
    assert_no_message(&reply, "a save at Savings");
    let reply = sam.post(&remove_path(savings), &named_removal).await;
    assert_see_other(&reply, &page_path(savings), "a removal at Savings");
    assert_no_message(&reply, "a removal at Savings");

    remove_elsewhere(&app, checking).await;
    app.open_balance(checking, "2026-09-30", 250_000).await;
    let replaced = own.id.to_string();
    let reply = sam
        .post(
            &form_path(checking),
            &[
                ("entry", replaced.as_str()),
                ("version", "1"),
                ("date", "2026-09-01"),
                ("amount", "1.00"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of a save naming a replaced entry:\n{}",
        reply.body
    );
    assert!(
        reply.shows("2,500.00") && reply.shows("Save mine"),
        "a save naming a replaced entry does not show the new figure:\n{}",
        reply.body
    );
    assert_no_message(&reply, "a save naming a replaced entry");
    assert_eq!(live(&app, cash).await, Some(other), "Cash's entry");
    assert_eq!(live(&app, savings).await, None, "Savings' opening balance");
    assert_eq!(
        entered(&app, checking).await.amount,
        Money::from_minor(250_000),
        "Checking's new entry"
    );
}

/// The enter form posted twice answers 303 twice and leaves one opening balance at version 1;
/// only the first answer sets the message.
///
/// Proves: Decision 100 with the pause's P2 and INV-13: the enter form posted twice answers 303 twice and leaves one opening balance at version 1, the second answer setting no message cookie.
#[tokio::test]
async fn the_enter_form_sent_twice_leaves_one_opening_balance() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let form = [
        ("date", "2026-09-30"),
        ("amount", "35.20"),
        ("below_zero", "on"),
    ];
    let first = sam.post(&form_path(checking), &form).await;
    assert_see_other(&first, &page_path(checking), "the first save");
    assert_message(
        &first,
        &format!("opening-balance-saved.{checking}"),
        "the first save",
    );
    let saved = entered(&app, checking).await;
    let second = sam.post(&form_path(checking), &form).await;
    assert_see_other(&second, &page_path(checking), "the second save");
    assert_no_message(&second, "the second save");
    assert_eq!(
        live(&app, checking).await,
        Some(saved),
        "the opening balance after the second save"
    );
    assert_eq!(
        (saved.amount, saved.version),
        (Money::from_minor(-3_520), 1),
        "the one opening balance"
    );
}

/// Over a book whose totals do not fit in 64 bits, built through the core's operations, the
/// form, the confirmation and a clash page answer as usual, since they never read the roll-up,
/// while Home answers 500; and a removal and a save that goes through answer 303, the way out of
/// such a book.
///
/// Proves: Decision 141 with A7 and the pause's P5: over a book whose totals do not fit, built through the core's operations under the limit, the form, the confirmation and a clash page answer as usual while Home answers 500.
#[tokio::test]
async fn the_opening_balance_pages_answer_while_the_totals_do_not_fit() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let cash = app.account("Assets:Cash").await;
    let savings = app.account("Assets:Savings").await;
    app.open_balance(savings, "2026-09-30", 248_015).await;
    let gift = app.add_account("Income", "Gift", None).await;
    let prize = app.add_account("Income", "Prize", None).await;
    let by = admin(&app).await;
    let huge = Money::from_minor(5_000_000_000_000_000_000);
    let negated = huge
        .checked_neg()
        .unwrap_or_else(|| panic!("the huge amount has no negation"));
    for (account, income) in [(checking, gift), (cash, prize)] {
        let mut tx = app.write().await;
        transactions::create_transaction(
            &mut tx,
            by,
            app.clock.now(),
            &NewTransaction {
                date: date("2026-10-01"),
                postings: vec![
                    NewPosting {
                        account,
                        amount: huge,
                    },
                    NewPosting {
                        account: income,
                        amount: negated,
                    },
                ],
            },
        )
        .await
        .unwrap_or_else(|error| panic!("cannot create the transaction: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the transaction: {error}"));
    }
    let home = sam.get("/").await;
    assert_eq!(
        home.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "status of Home over totals that do not fit:\n{}",
        home.body
    );
    let form = sam.get(&form_path(checking)).await;
    assert_eq!(
        form.status,
        StatusCode::OK,
        "status of the form:\n{}",
        form.body
    );
    let confirm = sam.get(&remove_path(savings)).await;
    assert_eq!(
        confirm.status,
        StatusCode::OK,
        "status of the confirmation:\n{}",
        confirm.body
    );
    let saved = entered(&app, savings).await;
    let entry = saved.id.to_string();
    let reply = sam
        .post(
            &form_path(savings),
            &[
                ("entry", entry.as_str()),
                ("version", "7"),
                ("date", "2026-09-01"),
                ("amount", "1.00"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash page:\n{}",
        reply.body
    );
    assert!(reply.shows("2,480.15"), "the clash page:\n{}", reply.body);

    // The removal and a save that goes through are the way out of such a book: neither reads the
    // roll-up either.
    let removal = fields(&confirm, &remove_path(savings));
    let reply = sam.post(&remove_path(savings), &pairs(&removal)).await;
    assert_see_other(
        &reply,
        &page_path(savings),
        "the removal over totals that do not fit",
    );
    assert_eq!(live(&app, savings).await, None, "Savings after the removal");
    let reply = sam
        .post(&form_path(checking), &entering("2026-09-30", "12.00"))
        .await;
    assert_see_other(
        &reply,
        &page_path(checking),
        "a save over totals that do not fit",
    );
    assert_eq!(
        entered(&app, checking).await.amount,
        Money::from_minor(1_200),
        "Checking after the save"
    );
}

/// GET of the confirmation shows the figure and changes nothing, Keep it outlined and leading to
/// the account's page, and for an account without an opening balance answers 303 to its page; a
/// POST whose entry or version is missing or not written as Nage prints a number (a sign, a
/// leading zero) answers 409 with the confirmation again and removes nothing; the POST that
/// names them as shown removes in one write with its message; a second removal finds nothing and
/// answers 303 with no message.
///
/// Proves: Decisions 143 and 146 with INV-25: GET shows the confirmation and changes nothing, for an account without one a 303 to its page; POST removes in one write with its message; a second removal is a 303 with no message.
#[tokio::test]
async fn remove_asks_first_and_a_second_remove_finds_nothing() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let saved = entered(&app, checking).await;
    let confirm = sam.get(&remove_path(checking)).await;
    assert_eq!(
        confirm.status,
        StatusCode::OK,
        "status of the confirmation:\n{}",
        confirm.body
    );
    assert!(
        confirm.shows("Remove the opening balance?")
            && confirm.shows("Assets:Checking: 2,480.15 on 2026-09-30.")
            && !confirm.shows("You typed 0"),
        "the confirmation does not show the figure:\n{}",
        confirm.body
    );
    assert!(
        main_of(&confirm.body).contains(&format!(
            r#"<a class="button outline" href="{}">Keep it</a>"#,
            page_path(checking)
        )),
        "Keep it is not outlined or does not lead to the account's page:\n{}",
        confirm.body
    );
    assert_eq!(
        live(&app, checking).await,
        Some(saved),
        "after the confirmation"
    );
    let cash = app.account("Assets:Cash").await;
    let reply = sam.get(&remove_path(cash)).await;
    assert_see_other(
        &reply,
        &page_path(cash),
        "the confirmation of an account without one",
    );

    let id = saved.id.to_string();
    let signed = format!("+{id}");
    let padded = format!("0{id}");
    for (what, malformed) in [
        (
            "an entry with a sign",
            vec![("entry", signed.as_str()), ("version", "1")],
        ),
        (
            "an entry with a leading zero",
            vec![("entry", padded.as_str()), ("version", "1")],
        ),
        (
            "a version with a sign",
            vec![("entry", id.as_str()), ("version", "+1")],
        ),
        (
            "a version with a leading zero",
            vec![("entry", id.as_str()), ("version", "01")],
        ),
        ("no version", vec![("entry", id.as_str())]),
        ("no entry", vec![("version", "1")]),
    ] {
        let reply = sam.post(&remove_path(checking), &malformed).await;
        assert_eq!(
            reply.status,
            StatusCode::CONFLICT,
            "status of a removal naming {what}:\n{}",
            reply.body
        );
        assert!(
            reply.shows("Remove the opening balance?")
                && reply.shows("Assets:Checking: 2,480.15 on 2026-09-30."),
            "a removal naming {what} does not show the confirmation again:\n{}",
            reply.body
        );
        assert_no_message(&reply, what);
        assert_eq!(
            live(&app, checking).await,
            Some(saved),
            "after a removal naming {what}"
        );
    }

    let sent = fields(&confirm, &remove_path(checking));
    let reply = sam.post(&remove_path(checking), &pairs(&sent)).await;
    assert_see_other(&reply, &page_path(checking), "the removal");
    assert_message(
        &reply,
        &format!("opening-balance-removed.{checking}"),
        "the removal",
    );
    assert_eq!(live(&app, checking).await, None, "after the removal");
    let page = sam.get(&page_path(checking)).await;
    assert_eq!(
        page.notices(),
        [(
            String::from("status"),
            Notice::OpeningBalanceRemoved.to_string()
        )],
        "the page after the removal"
    );
    assert!(
        page.shows("None yet") && page.shows("Enter opening balance"),
        "the page after the removal does not offer Enter opening balance:\n{}",
        page.body
    );
    let again = sam.post(&remove_path(checking), &pairs(&sent)).await;
    assert_see_other(&again, &page_path(checking), "a second removal");
    assert_no_message(&again, "a second removal");
}

/// A removal naming a version no longer saved answers 409 with the confirmation, the sentence
/// at its top and the figure as it is now, without the line of a typed zero, nothing written;
/// its Remove names the new version and removes the opening balance.
///
/// Proves: Decision 143 with INV-7: a removal naming a version no longer saved answers 409 with the confirmation and the figure as it is now, nothing written; Remove there names the new version and removes it.
#[tokio::test]
async fn a_remove_that_meets_a_change_shows_the_new_figure() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let saved = entered(&app, checking).await;
    let now = change_elsewhere(&app, checking, "2026-09-30", 250_000).await;
    let entry = saved.id.to_string();
    let reply = sam
        .post(
            &remove_path(checking),
            &[("entry", entry.as_str()), ("version", "1")],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the removal:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            OpeningClash::RemoveChanged {
                account: String::from("Assets:Checking"),
                saved: now,
                digits: MinorDigits::TWO,
            }
            .to_string()
        )],
        "the sentence of the removal that met a change"
    );
    assert!(
        reply.shows("Assets:Checking: 2,500.00 on 2026-09-30.") && !reply.shows("You typed 0"),
        "the confirmation does not show the figure as it is now, or claims a typed zero:\n{}",
        reply.body
    );
    assert_eq!(
        box_value(&reply, "version"),
        Some(String::from("2")),
        "the version Remove names"
    );
    assert_no_message(&reply, "the removal that met a change");
    assert_eq!(
        live(&app, checking).await,
        Some(now),
        "after the removal that met a change"
    );
    let sent = fields(&reply, &remove_path(checking));
    let reply = sam.post(&remove_path(checking), &pairs(&sent)).await;
    assert_see_other(
        &reply,
        &page_path(checking),
        "Remove naming the new version",
    );
    assert_eq!(live(&app, checking).await, None, "after Remove");
}

/// With the writer held, the save and the removal answer 503 with the busy page, change no
/// row and set no cookie.
///
/// Proves: INV-28 and SPEC 5.4: with the writer held, the save and the removal answer 503, change no row and set no message cookie.
#[tokio::test]
async fn every_opening_balance_post_answers_busy_and_leaves_no_trace() {
    let (app, mut sam) = household_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await;
    let checking = app.account("Assets:Checking").await;
    let cash = app.account("Assets:Cash").await;
    app.open_balance(checking, "2026-09-30", 248_015).await;
    let saved = entered(&app, checking).await;
    let entry = saved.id.to_string();
    let held = app.write().await;
    let replies = [
        (
            "the save",
            sam.post(&form_path(cash), &entering("2026-09-30", "12.00"))
                .await,
        ),
        (
            "the removal",
            sam.post(
                &remove_path(checking),
                &[("entry", entry.as_str()), ("version", "1")],
            )
            .await,
        ),
    ];
    drop(held);
    for (what, reply) in replies {
        assert_eq!(
            reply.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "status of {what} while the writer is held:\n{}",
            reply.body
        );
        assert!(reply.shows("Nage is busy"), "{what} is not the busy page");
        assert!(
            reply.set_cookies().is_empty(),
            "{what} sets a cookie: {:?}",
            reply.set_cookies()
        );
    }
    assert_eq!(live(&app, cash).await, None, "Cash after the busy save");
    assert_eq!(
        live(&app, checking).await,
        Some(saved),
        "Checking after the busy removal"
    );
}

/// Today is the server's day in the configured zone at the clock's instant: the help line and
/// the date box's hint name it, a date after it is refused by the server whatever the browser
/// allowed, and today itself is taken.
///
/// Proves: A8 and decision 120: the help line and the date box's hint name today in the configured zone at the clock's instant, and a date after it is refused by the server whatever the browser allowed.
#[tokio::test]
async fn today_is_the_servers_day_in_the_configured_zone() {
    let (app, mut sam) = household_with(Options {
        zone: Some("America/Toronto"),
        ..Options::default()
    })
    .await;
    app.clock.set(DAY - 6 * HOUR);
    let checking = app.account("Assets:Checking").await;
    let form = sam.get(&form_path(checking)).await;
    assert_eq!(
        form.status,
        StatusCode::OK,
        "status of the form:\n{}",
        form.body
    );
    let main = main_of(&form.body);
    assert!(
        form.shows("Not after today, 2027-01-15.") && main.contains(r#"max="2027-01-15""#),
        "the form does not name today in Toronto:\n{main}"
    );
    let reply = sam
        .post(&form_path(checking), &entering("2027-01-16", "12.00"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a date after today:\n{}",
        reply.body
    );
    assert!(
        reply.shows(
            &OpeningBoxRefusal::DateAfterToday {
                today: date("2027-01-15")
            }
            .to_string()
        ),
        "a date after today is not refused in Nage's words:\n{}",
        reply.body
    );
    let reply = sam
        .post(&form_path(checking), &entering("2027-01-15", "12.00"))
        .await;
    assert_see_other(&reply, &page_path(checking), "a save dated today");
}

/// Two accounts saved through the form at the largest amount a form takes keep Home, the list
/// and both account pages answering with their figures; one minor unit more is refused.
///
/// Proves: Decision 142: with two accounts saved through the form at the limit, Home, the list and both account pages answer 200 with their figures, and one minor unit more is refused.
#[tokio::test]
async fn the_largest_amounts_keep_home_the_list_and_the_page_answering() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let savings = app.account("Assets:Savings").await;
    for account in [checking, savings] {
        let reply = sam
            .post(
                &form_path(account),
                &entering("2026-09-30", "999,999,999,999.99"),
            )
            .await;
        assert_see_other(&reply, &page_path(account), "a save at the limit");
    }
    for path in [
        String::from("/"),
        String::from("/accounts"),
        page_path(checking),
        page_path(savings),
    ] {
        let reply = sam.get(&path).await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "status of {path}:\n{}",
            reply.body
        );
        assert!(
            reply.shows("999,999,999,999.99"),
            "{path} does not show the figure"
        );
    }
    let home = sam.get("/").await;
    assert!(
        home.shows("1,999,999,999,999.98"),
        "Home does not show the total:\n{}",
        home.body
    );
    let cash = app.account("Assets:Cash").await;
    let reply = sam
        .post(
            &form_path(cash),
            &entering("2026-09-30", "1,000,000,000,000.00"),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of one minor unit more:\n{}",
        reply.body
    );
    assert!(
        reply.shows(
            &OpeningBoxRefusal::TooLarge {
                digits: MinorDigits::TWO
            }
            .to_string()
        ),
        "one minor unit more is not refused:\n{}",
        reply.body
    );
    assert_eq!(live(&app, cash).await, None, "Cash after the refusal");
}
