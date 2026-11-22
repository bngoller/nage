//! The account pages over HTTP (SPEC 6.2, W2, INV-27, INV-28, decisions 128 to 137), driving the
//! router in process with a clock the tests set: the addresses behind the sign-in and the Origin
//! check, the list, the create pages, an account's page and its one-time messages, the change
//! form with its clash, the confirmations, and the refusals with their statuses, every write all
//! or none. Every scenario is built through the core's operations.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::accounts::{self, Account, AccountError, AccountId, Action, Kind, Refused, Word};
use nage_core::fold::NameError;
use nage_core::money::{MinorDigits, Money};
use nage_core::storage::StorageOptions;
use nage_core::users::UserId;
use nage_web::messages::{Notice, Refusal};

use common::app::{
    ADMIN, App, Client, Options, PASSWORD, Reply, Req, assert_security_headers, attribute, control,
    main_of,
};

/// A book set up with the starter accounts, and Sam, its admin, signed in.
async fn household() -> (App, Client) {
    let app = App::start().await;
    app.set_up_as(true).await;
    let sam = app.signed_in(ADMIN, PASSWORD).await;
    (app, sam)
}

/// Every account as stored, by id.
async fn rows(app: &App) -> Vec<Account> {
    let mut read = app.read().await;
    accounts::accounts(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"))
}

/// The account `id` as stored.
async fn row(app: &App, id: AccountId) -> Account {
    rows(app)
        .await
        .into_iter()
        .find(|account| account.id == id)
        .unwrap_or_else(|| panic!("no account {id}"))
}

/// The admin's id.
async fn admin(app: &App) -> UserId {
    app.credentials(ADMIN).await.user.id
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

/// Unarchives `id` through the core, as another user or another window would.
async fn unarchive(app: &App, id: AccountId) {
    let by = admin(app).await;
    let mut tx = app.write().await;
    accounts::unarchive_account(&mut tx, by, app.clock.now(), id)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive {id}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the unarchive of {id}: {error}"));
}

/// Renames `id` through the core, as another save would.
async fn rename(app: &App, id: AccountId, name: &str) {
    let by = admin(app).await;
    let mut tx = app.write().await;
    accounts::rename_account(&mut tx, by, app.clock.now(), id, name)
        .await
        .unwrap_or_else(|error| panic!("cannot rename {id}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the rename of {id}: {error}"));
}

/// Moves `id` inside `parent` through the core, as another save would.
async fn move_into(app: &App, id: AccountId, parent: AccountId) {
    let by = admin(app).await;
    let mut tx = app.write().await;
    accounts::move_account(&mut tx, by, app.clock.now(), id, parent)
        .await
        .unwrap_or_else(|error| panic!("cannot move {id}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the move of {id}: {error}"));
}

/// Gives `id` to `owner` through the core, as another save would.
async fn give(app: &App, id: AccountId, owner: Option<UserId>) {
    let by = admin(app).await;
    let mut tx = app.write().await;
    accounts::set_owner(&mut tx, by, app.clock.now(), id, owner)
        .await
        .unwrap_or_else(|error| panic!("cannot set the owner of {id}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the owner of {id}: {error}"));
}

/// What a browser sends from the form in the page's main content: every hidden and text box
/// with its value, and every list with its chosen value, the first one when none is marked.
fn fields(reply: &Reply) -> Vec<(String, String)> {
    let main = main_of(&reply.body);
    let mut out = Vec::new();
    let mut rest = main;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if tag.starts_with("input ") {
            let kind = attribute(tag, "type").unwrap_or_default();
            if kind == "hidden" || kind == "text" {
                let name = attribute(tag, "name").unwrap_or_default();
                out.push((name, attribute(tag, "value").unwrap_or_default()));
            }
        } else if tag.starts_with("select ") {
            let name = attribute(tag, "name").unwrap_or_default();
            let close = rest.find("</select>").unwrap_or(rest.len());
            let mut first = None;
            let mut chosen = None;
            for option in rest[..close].split("<option").skip(1) {
                let option_tag = option.split('>').next().unwrap_or_default();
                let value = attribute(option_tag, "value").unwrap_or_default();
                if first.is_none() {
                    first = Some(value.clone());
                }
                if option_tag.contains(" selected") {
                    chosen = Some(value);
                }
            }
            out.push((name, chosen.or(first).unwrap_or_default()));
        }
    }
    out
}

/// Sets the field `name` of `sent` to `value`, adding it when the form had none.
fn set(sent: &mut Vec<(String, String)>, name: &str, value: &str) {
    match sent.iter_mut().find(|(field, _)| field == name) {
        Some((_, held)) => *held = value.to_owned(),
        None => sent.push((name.to_owned(), value.to_owned())),
    }
}

/// `sent` as the pairs a request takes.
fn pairs(sent: &[(String, String)]) -> Vec<(&str, &str)> {
    sent.iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
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

/// `html` without its tags, with every attribute value in a tag joined after it, so that a
/// number printed in the text or in an attribute is found and a tag's own name is not.
fn text_of(html: &str) -> String {
    let mut out = String::new();
    for chunk in html.split('<') {
        let (tag, text) = chunk.split_once('>').unwrap_or(("", chunk));
        for value in tag.split('"').skip(1).step_by(2) {
            out.push(' ');
            out.push_str(value);
        }
        out.push(' ');
        out.push_str(text);
    }
    out
}

/// Whether the box `name` of the page's form carries `aria-invalid`.
fn invalid(reply: &Reply, name: &str) -> bool {
    control(main_of(&reply.body), name).contains("aria-invalid")
}

/// The value the box `name` of the page's form holds.
fn value_of(reply: &Reply, name: &str) -> String {
    attribute(control(main_of(&reply.body), name), "value").unwrap_or_default()
}

/// The first part of a refusal of a new account.
fn new_account() -> Refused {
    Refused {
        word: Word::Account,
        place: None,
        action: Action::Created,
    }
}

/// The first part of a refusal of `action` on the account at `place`.
fn refused_on(place: &str, action: Action) -> Refused {
    Refused {
        word: Word::Account,
        place: Some(place.to_owned()),
        action,
    }
}

/// Without a session every account address, GET and POST, leads to the sign-in page and
/// changes nothing, each answer with `Cache-Control: no-store`; signed in, `/accounts` is the
/// list, Accounts marked in both copies of the menu, an archived account hidden and counted,
/// and `?archived=shown` shows it in its place.
///
/// Proves: INV-27 and decisions 128, 131 and 137 with P12: every account address, GET and POST, without a session leads to the sign-in page; GET /accounts marks Accounts and shows archived ones on request.
#[tokio::test]
async fn the_list_answers_behind_the_sign_in_with_the_menu_entry() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let jar = app.add_account("Assets", "Old jar", None).await;
    archive(&app, jar).await;
    let before = rows(&app).await;
    let mut stranger = app.client();
    let addresses = [
        ("GET", String::from("/accounts")),
        ("GET", String::from("/accounts?archived=shown")),
        ("GET", String::from("/accounts/new")),
        ("GET", String::from("/accounts/new?under=assets")),
        ("GET", format!("/accounts/{checking}")),
        ("GET", format!("/accounts/{checking}/edit")),
        ("GET", format!("/accounts/{checking}/archive")),
        ("GET", format!("/accounts/{jar}/unarchive")),
        ("POST", String::from("/accounts/new?under=assets")),
        ("POST", format!("/accounts/{checking}/edit")),
        ("POST", format!("/accounts/{checking}/archive")),
        ("POST", format!("/accounts/{jar}/unarchive")),
    ];
    for (method, uri) in &addresses {
        let what = format!("{method} {uri} without a session");
        let reply = if *method == "GET" {
            stranger.get(uri).await
        } else {
            stranger
                .post(uri, &[("name", "Sneaky"), ("shown_name", "Checking")])
                .await
        };
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "status of {what}");
        let location = reply.location().unwrap_or_default();
        assert!(
            location == "/login" || location.starts_with("/login?next="),
            "{what} leads to {location:?}"
        );
        assert_security_headers(&reply, &what);
    }
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the requests without a session"
    );

    let list = sam.get("/accounts").await;
    assert_eq!(list.status, StatusCode::OK, "status of the list");
    assert_security_headers(&list, "the list");
    assert_eq!(
        list.body
            .matches(r#"<a href="/accounts" aria-current="page">Accounts</a>"#)
            .count(),
        2,
        "the menu's two copies marking Accounts on the list:\n{}",
        list.body
    );
    let main = main_of(&list.body);
    assert!(
        main.contains(">Checking<") && !main.contains(">Old jar<"),
        "the list does not hide the archived account:\n{main}"
    );
    assert!(
        list.shows("Show archived (1)"),
        "the list does not count the archived account:\n{main}"
    );
    let shown = sam.get("/accounts?archived=shown").await;
    assert_eq!(
        shown.status,
        StatusCode::OK,
        "status of the list with the archived accounts"
    );
    assert!(
        main_of(&shown.body).contains(">Old jar<") && shown.shows("Hide archived"),
        "the list does not show the archived account in its place:\n{}",
        shown.body
    );
}

/// `/accounts/new` with no query or an empty one asks what to add, each of its four choices
/// leading to the form of its own section; each of the four sections
/// opens its form, the owner box only under Assets and Liabilities; Equity, a section Nage
/// does not know and an empty one get the page for an unknown address; `inside` with a place
/// the form offers chooses it, with the trail and the way back through it, and `inside` with
/// the section itself chooses Directly under it; `inside` without
/// `under`, or with a place the form does not offer (a category, a built-in account, an
/// archived account, a number written another way, a number no account has), leads to the
/// same address without it.
///
/// Proves: Decisions 128 and 132 with P6: no or an empty query asks what to add, a section opens its form, inside preselects, an unknown section is 404, an inside without under or not offered is a 303 without it.
#[tokio::test]
async fn the_create_pages_answer_by_section() {
    let (app, mut sam) = household().await;
    let savings = app.account("Assets:Savings").await;
    let groceries = app.account("Expenses:Groceries").await;
    let opening = app.account("Equity:Opening Balances").await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;
    for uri in ["/accounts/new", "/accounts/new?"] {
        let reply = sam.get(uri).await;
        assert_eq!(reply.status, StatusCode::OK, "status of {uri}");
        assert!(
            reply.shows("What do you want to add?"),
            "{uri} does not ask what to add:\n{}",
            reply.body
        );
        assert_security_headers(&reply, uri);
        let main = main_of(&reply.body);
        for (under, choice) in [
            ("assets", "Something you have"),
            ("liabilities", "Something you owe"),
            ("income", "A kind of income"),
            ("expenses", "A kind of spending"),
        ] {
            assert!(
                main.contains(&format!(
                    r#"<a href="/accounts/new?under={under}"><strong>{choice}</strong>"#
                )),
                "{choice} on {uri} does not lead to the form under {under}:\n{main}"
            );
        }
    }
    for (under, heading, owner) in [
        ("assets", "New account under Assets", true),
        ("liabilities", "New account under Liabilities", true),
        ("income", "New category under Income", false),
        ("expenses", "New category under Expenses", false),
    ] {
        let uri = format!("/accounts/new?under={under}");
        let reply = sam.get(&uri).await;
        assert_eq!(reply.status, StatusCode::OK, "status of {uri}");
        assert!(
            reply.shows(heading),
            "{uri} is not its form:\n{}",
            reply.body
        );
        assert_eq!(
            main_of(&reply.body).contains(r#"name="owner""#),
            owner,
            "the owner box on {uri}"
        );
    }
    for under in ["equity", "Assets", "nonsense", ""] {
        let uri = format!("/accounts/new?under={under}");
        assert_unknown(&sam.get(&uri).await, &uri);
    }

    let uri = format!("/accounts/new?under=assets&inside={savings}");
    let reply = sam.get(&uri).await;
    assert_eq!(reply.status, StatusCode::OK, "status of {uri}");
    let main = main_of(&reply.body);
    for (what, expected) in [
        (
            "Savings chosen",
            format!(r#"<option value="{savings}" selected>Assets:Savings</option>"#),
        ),
        (
            "the trail through Savings",
            format!(r#"<a href="/accounts/{savings}">Savings</a>"#),
        ),
        (
            "the way back to Savings",
            format!(r#"<a class="back" href="/accounts/{savings}">Back to Savings</a>"#),
        ),
        (
            "the form's address keeping inside",
            format!(r#"action="/accounts/new?under=assets&amp;inside={savings}""#),
        ),
    ] {
        assert!(main.contains(&expected), "{uri} lacks {what}:\n{main}");
    }
    let backs: Vec<String> = common::start_tags(&reply.body, "<input")
        .into_iter()
        .filter(|tag| tag.contains(r#"name="back""#))
        .map(|tag| attribute(tag, "value").unwrap_or_default())
        .collect();
    assert!(
        !backs.is_empty() && backs.iter().all(|back| *back == uri),
        "the appearance form of {uri} does not return to it, Savings's number as Nage prints it: {backs:?}"
    );
    let assets = app.account("Assets").await;
    let uri = format!("/accounts/new?under=assets&inside={assets}");
    let reply = sam.get(&uri).await;
    assert_eq!(reply.status, StatusCode::OK, "status of {uri}");
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{assets}" selected>Directly under Assets</option>"#
        )),
        "{uri} does not choose Directly under Assets:\n{}",
        reply.body
    );

    let reply = sam.get(&format!("/accounts/new?inside={savings}")).await;
    assert_see_other(&reply, "/accounts/new", "inside without under");
    for inside in [
        groceries.to_string(),
        opening.to_string(),
        old.to_string(),
        format!("0{savings}"),
        format!("+{savings}"),
        String::from("987654"),
        String::from("savings"),
    ] {
        let uri = format!("/accounts/new?under=assets&inside={inside}");
        let reply = sam.get(&uri).await;
        assert_see_other(&reply, "/accounts/new?under=assets", &uri);
    }
}

/// A create under Assets inside Savings is one write and a 303 to the new account's page,
/// whose one-time message carries the new account's number and which says once that it was
/// created; a category is said to be a category. A message left for an account's page is
/// cleared unseen by the list, and by another account's page. Behind a proxy, the account
/// message carries Secure exactly when the session cookie does (decision 117).
///
/// Proves: Decisions 132 and 137 with INV-28 and P7: a create is one write and a 303 to the new page, its message cookie carrying the number, shown once there.
#[tokio::test]
async fn a_create_lands_on_the_new_accounts_page_with_its_message() {
    let (app, mut sam) = household().await;
    let savings = app.account("Assets:Savings").await;
    let before = rows(&app).await;
    let reply = sam
        .post(
            "/accounts/new?under=assets",
            &[
                ("name", "  Holiday   fund "),
                ("inside", &savings.to_string()),
                ("owner", ""),
            ],
        )
        .await;
    let created = app.account("Assets:Savings:Holiday fund").await;
    let page = format!("/accounts/{created}");
    assert_see_other(&reply, &page, "the create");
    assert_eq!(
        reply.message_cookie().map(|(value, _)| value),
        Some(format!("account-created.{created}")),
        "the one-time message of the create"
    );
    let after = rows(&app).await;
    assert_eq!(after.len(), before.len() + 1, "rows after the create");
    let new = row(&app, created).await;
    assert_eq!(
        (new.name.as_str(), new.parent_id, new.owner_user_id),
        ("Holiday fund", Some(savings), None),
        "the account created"
    );
    assert_eq!(new.created_by, admin(&app).await, "who created it");

    let shown = sam.get(&page).await;
    assert_eq!(shown.status, StatusCode::OK, "status of the new page");
    assert_eq!(
        shown.notices(),
        [(
            String::from("status"),
            Notice::AccountCreated {
                word: Word::Account
            }
            .to_string()
        )],
        "the new page's message"
    );
    assert!(
        shown.clears_the_message(),
        "the new page clears the message"
    );
    assert!(
        sam.get(&page).await.notices().is_empty(),
        "the new page says it again"
    );

    let reply = sam
        .post("/accounts/new?under=expenses", &[("name", "Pets")])
        .await;
    let pets = app.account("Expenses:Pets").await;
    assert_see_other(
        &reply,
        &format!("/accounts/{pets}"),
        "the create of a category",
    );
    let shown = sam.get(&format!("/accounts/{pets}")).await;
    assert_eq!(
        shown.notices(),
        [(
            String::from("status"),
            Notice::AccountCreated {
                word: Word::Category
            }
            .to_string()
        )],
        "the new category's message"
    );

    sam.post("/accounts/new?under=expenses", &[("name", "Toys")])
        .await;
    let toys = app.account("Expenses:Toys").await;
    let list = sam.get("/accounts").await;
    assert!(
        list.notices().is_empty(),
        "the list shows an account's message"
    );
    assert!(
        list.clears_the_message(),
        "the list leaves the message waiting"
    );
    assert!(
        sam.get(&format!("/accounts/{toys}"))
            .await
            .notices()
            .is_empty(),
        "a message cleared by the list shows later"
    );

    sam.post("/accounts/new?under=expenses", &[("name", "Kites")])
        .await;
    let kites = app.account("Expenses:Kites").await;
    let other = sam.get(&format!("/accounts/{pets}")).await;
    assert!(
        other.notices().is_empty(),
        "another account's page shows the message"
    );
    assert!(
        other.clears_the_message(),
        "another account's page leaves the message waiting"
    );
    assert!(
        sam.get(&format!("/accounts/{kites}"))
            .await
            .notices()
            .is_empty(),
        "a message cleared by another account's page shows later"
    );

    let proxied = App::start_with(Options {
        trust_proxy: true,
        ..Options::default()
    })
    .await;
    proxied.set_up_as(true).await;
    for (proto, secure) in [("https", true), ("http", false)] {
        let mut client = proxied.client();
        let login = client
            .send(
                Req::form("/login", &[("username", ADMIN), ("password", PASSWORD)])
                    .header("x-forwarded-proto", proto),
            )
            .await;
        let (_, session) = login.session_cookie();
        let name = format!("Jar over {proto}");
        let reply = client
            .send(
                Req::form("/accounts/new?under=assets", &[("name", name.as_str())])
                    .header("x-forwarded-proto", proto),
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the create over {proto}:\n{}",
            reply.body
        );
        let (_, message) = reply
            .message_cookie()
            .unwrap_or_else(|| panic!("the create over {proto} sets no one-time message"));
        let is_secure = |attributes: &[String]| attributes.iter().any(|one| one == "Secure");
        assert_eq!(
            (is_secure(&session), is_secure(&message)),
            (secure, secure),
            "Secure on the session cookie and on the account message over {proto}"
        );
    }
}

/// A create refused for its name answers 422 with the line at the top, the core's sentence
/// under the name box alone and what was typed kept, the place and the owner chosen; one
/// opened inside Savings keeps its trail, its way back and its address through Savings, and a
/// refused category whose address names a built-in account as inside keeps none of them and
/// says category at the top; a form
/// without any field is refused for its empty name, directly under the section and Joint; a
/// place and an owner that are not numbers as Nage writes them are each refused under their
/// box. None of them writes a row or sets a one-time message.
///
/// Proves: Decision 132 and INV-28 with P6: 422 with every problem under its box, what was typed kept, missing fields empty, a malformed place or owner refused under its box, no row and no message cookie.
#[tokio::test]
async fn a_refused_create_keeps_what_was_typed_and_writes_nothing() {
    let (app, mut sam) = household().await;
    let savings = app.account("Assets:Savings").await;
    let sam_id = admin(&app).await;
    let before = rows(&app).await;

    let reply = sam
        .post(
            "/accounts/new?under=assets",
            &[
                ("name", "Bills: Hydro"),
                ("inside", &savings.to_string()),
                ("owner", &sam_id.to_string()),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "status");
    assert_no_message(&reply, "the refused create");
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            Refusal::AccountNotCreated {
                word: Word::Account
            }
            .to_string()
        )],
        "the line at the top of the refused form"
    );
    let colon = AccountError::Name {
        refused: new_account(),
        error: NameError::Colon,
    };
    assert!(
        reply.shows(&colon.to_string()),
        "the refused form does not say why:\n{}",
        reply.body
    );
    assert_eq!(value_of(&reply, "name"), "Bills: Hydro", "the name kept");
    let main = main_of(&reply.body);
    assert!(
        main.contains(&format!(
            r#"<option value="{savings}" selected>Assets:Savings</option>"#
        )) && main.contains(&format!(
            r#"<option value="{sam_id}" selected>Sam</option>"#
        )),
        "the place and the owner chosen are not kept:\n{main}"
    );
    assert_eq!(
        (
            invalid(&reply, "name"),
            invalid(&reply, "inside"),
            invalid(&reply, "owner")
        ),
        (true, false, false),
        "the boxes marked"
    );

    let reply = sam
        .post(
            &format!("/accounts/new?under=assets&inside={savings}"),
            &[("name", "Bills: Hydro"), ("inside", &savings.to_string())],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused create opened inside Savings"
    );
    let main = main_of(&reply.body);
    for (what, expected) in [
        (
            "the trail through Savings",
            format!(r#"<a href="/accounts/{savings}">Savings</a>"#),
        ),
        (
            "the way back to Savings",
            format!(r#"<a class="back" href="/accounts/{savings}">Back to Savings</a>"#),
        ),
        (
            "the form's address keeping inside",
            format!(r#"action="/accounts/new?under=assets&amp;inside={savings}""#),
        ),
    ] {
        assert!(
            main.contains(&expected),
            "the refused create opened inside Savings lacks {what}:\n{main}"
        );
    }
    assert_no_message(&reply, "the refused create opened inside Savings");

    let built_in = app.account("Expenses:Uncategorized").await;
    let address = format!("/accounts/new?under=expenses&inside={built_in}");
    let reply = sam.post(&address, &[("name", "Bills: Hydro")]).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused category sent with a built-in account as inside"
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            Refusal::AccountNotCreated {
                word: Word::Category
            }
            .to_string()
        )],
        "the line at the top of a refused category"
    );
    let main = main_of(&reply.body);
    assert!(
        !main.contains(&format!(r#"<a href="/accounts/{built_in}">"#))
            && !main.contains("Back to Uncategorized")
            && !main.contains(&format!("inside={built_in}")),
        "a refused create sent with a built-in account as inside is shown as opened inside it:\n{main}"
    );
    assert_no_message(&reply, "the refused category");

    let reply = sam.post("/accounts/new?under=assets", &[]).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of an empty form"
    );
    let empty = AccountError::Name {
        refused: new_account(),
        error: NameError::Empty,
    };
    assert!(
        reply.shows(&empty.to_string()),
        "the empty form is not refused for its empty name:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(r#"<option value="" selected>Joint</option>"#),
        "the empty form's owner is not Joint:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the empty form");

    let reply = sam
        .post(
            "/accounts/new?under=assets",
            &[
                ("name", "Fine name"),
                ("inside", &format!("0{savings}")),
                ("owner", &format!("+{sam_id}")),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a malformed place and owner"
    );
    for refusal in [
        AccountError::PlaceNotListed {
            refused: new_account(),
        },
        AccountError::OwnerNotListed {
            refused: new_account(),
        },
    ] {
        assert!(
            reply.shows(&refusal.to_string()),
            "the malformed form does not say {refusal}:\n{}",
            reply.body
        );
    }
    assert_eq!(
        (
            invalid(&reply, "name"),
            invalid(&reply, "inside"),
            invalid(&reply, "owner")
        ),
        (false, true, true),
        "the boxes marked on the malformed form"
    );
    assert_no_message(&reply, "the malformed form");
    assert_eq!(rows(&app).await, before, "the accounts after the refusals");
}

/// A create with an empty name inside an archived account answers 422 with both problems at
/// once, each under its own box, which alone are marked; the archived place stays in the list,
/// chosen and marked archived; nothing is written.
///
/// Proves: Decision 132 and INV-28 with P3: a create form refused under Name and Inside at once answers 422 with both problems, each under its box, and writes nothing.
#[tokio::test]
async fn a_form_refused_in_two_boxes_shows_both_and_writes_nothing() {
    let (app, mut sam) = household().await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;
    let before = rows(&app).await;
    let reply = sam
        .post(
            "/accounts/new?under=assets",
            &[("name", " "), ("inside", &old.to_string())],
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "status");
    for refusal in [
        AccountError::Name {
            refused: new_account(),
            error: NameError::Empty,
        },
        AccountError::ParentArchived {
            refused: new_account(),
            parent: String::from("Assets:Old savings"),
        },
    ] {
        assert!(
            reply.shows(&refusal.to_string()),
            "the form does not say {refusal}:\n{}",
            reply.body
        );
    }
    assert_eq!(
        (
            invalid(&reply, "name"),
            invalid(&reply, "inside"),
            invalid(&reply, "owner")
        ),
        (true, true, false),
        "the boxes marked"
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{old}" selected>Assets:Old savings (archived)</option>"#
        )),
        "the archived place chosen is not kept:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the refused form");
    assert_eq!(rows(&app).await, before, "the accounts after the refusal");
}

/// A crafted create under Assets inside Equity, inside a built-in account or inside a
/// category, or for a deactivated owner, is refused under its box with 422 and writes nothing,
/// the deactivated owner kept in the list, chosen and marked;
/// an owner sent with a category is not read, and the category is joint; a place sent with a
/// built-in account's change is not read, and the account keeps its place; an owner sent with
/// a category's change is not read, and nothing is written.
///
/// Proves: Web rule 12 with P3: inside set to Equity, to a built-in account or to an Expenses category under Assets, and a deactivated owner, answer 422 under their box; an owner sent to a category's form changes nothing.
#[tokio::test]
async fn crafted_fields_are_refused_or_not_read() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let equity = app.account("Equity").await;
    let expenses = app.account("Expenses").await;
    let opening = app.account("Equity:Opening Balances").await;
    let groceries = app.account("Expenses:Groceries").await;
    let sam_id = admin(&app).await;
    let robin = app.add_named_user("robin", "Robin").await;
    app.deactivate(robin.id).await;
    let before = rows(&app).await;
    for (what, inside) in [
        ("Equity", equity),
        ("a built-in account", opening),
        ("a category", groceries),
    ] {
        let reply = sam
            .post(
                "/accounts/new?under=assets",
                &[("name", "Crafted"), ("inside", &inside.to_string())],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of a create inside {what}"
        );
        assert!(
            reply.shows(
                &AccountError::PlaceNotListed {
                    refused: new_account()
                }
                .to_string()
            ) && invalid(&reply, "inside"),
            "a create inside {what} is not refused under Inside:\n{}",
            reply.body
        );
    }
    let reply = sam
        .post(
            "/accounts/new?under=assets",
            &[("name", "Crafted"), ("owner", &robin.id.to_string())],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a create for a deactivated owner"
    );
    assert!(
        reply.shows(
            &AccountError::OwnerNotListed {
                refused: new_account()
            }
            .to_string()
        ) && invalid(&reply, "owner"),
        "a create for a deactivated owner is not refused under Owner:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{}" selected>Robin (deactivated)</option>"#,
            robin.id
        )),
        "the deactivated owner chosen is not kept, chosen and marked:\n{}",
        reply.body
    );
    assert_eq!(rows(&app).await, before, "the accounts after the refusals");

    let reply = sam
        .post(
            "/accounts/new?under=expenses",
            &[("name", "Pets"), ("owner", &sam_id.to_string())],
        )
        .await;
    let pets = app.account("Expenses:Pets").await;
    assert_see_other(
        &reply,
        &format!("/accounts/{pets}"),
        "a category with an owner",
    );
    assert_eq!(
        row(&app, pets).await.owner_user_id,
        None,
        "the category's owner"
    );

    let reply = sam
        .post(
            &format!("/accounts/{opening}/edit"),
            &[
                ("name", "Start"),
                ("shown_name", "Opening Balances"),
                ("inside", &assets.to_string()),
                ("shown_inside", &equity.to_string()),
            ],
        )
        .await;
    assert_see_other(
        &reply,
        &format!("/accounts/{opening}"),
        "a built-in account's change with a place",
    );
    let renamed = row(&app, opening).await;
    assert_eq!(
        (renamed.name.as_str(), renamed.parent_id),
        ("Start", Some(equity)),
        "the built-in account after a change with a place"
    );

    let before = rows(&app).await;
    let reply = sam
        .post(
            &format!("/accounts/{groceries}/edit"),
            &[
                ("name", "Groceries"),
                ("shown_name", "Groceries"),
                ("inside", &expenses.to_string()),
                ("shown_inside", &expenses.to_string()),
                ("owner", &sam_id.to_string()),
                ("shown_owner", ""),
            ],
        )
        .await;
    assert_see_other(
        &reply,
        &format!("/accounts/{groceries}"),
        "a category's change with an owner",
    );
    assert_no_message(&reply, "a category's change with an owner");
    assert_eq!(rows(&app).await, before, "the accounts after it");
}

/// An account number no account has, a section's, one written with a sign or a leading zero,
/// one too large for 64 bits, a negative one, zero, one with letters, an account's own number
/// written with percent-escapes and an escape that is no UTF-8 get the page for an unknown
/// address at every account address, GET and POST, which never prints the number, the
/// menu's appearance form leading home, a section's change that changes nothing included; the
/// list opened with a number in its query neither prints it nor hands it to its appearance
/// form; an `inside` that names no place leads to the address without it; a create with no
/// section or with Equity gets the page for an unknown address. Nothing is written.
///
/// Proves: Decisions 118 and 128 with P6: an unknown, root, signed, zero-led, too large or percent-escaped number at every account address answers the 404 page, and no such number from a path or a query is printed.
#[tokio::test]
async fn an_unknown_or_malformed_account_number_gets_the_page_for_an_unknown_address() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let assets = app.account("Assets").await;
    let before = rows(&app).await;
    let numbers = [
        String::from("987654"),
        assets.to_string(),
        format!("+{checking}"),
        format!("0{checking}"),
        String::from("99999999999999999999"),
        String::from("-1"),
        String::from("0"),
        format!("{checking}abc"),
        checking
            .to_string()
            .bytes()
            .map(|digit| format!("%{digit:02X}"))
            .collect::<String>(),
        String::from("%FF"),
    ];
    for number in &numbers {
        for (method, end) in [
            ("GET", ""),
            ("GET", "/edit"),
            ("GET", "/archive"),
            ("GET", "/unarchive"),
            ("POST", "/edit"),
            ("POST", "/archive"),
            ("POST", "/unarchive"),
        ] {
            let uri = format!("/accounts/{number}{end}");
            let what = format!("{method} {uri}");
            let reply = if method == "GET" {
                sam.get(&uri).await
            } else {
                sam.post(&uri, &[("name", "Crafted"), ("shown_name", "Checking")])
                    .await
            };
            assert_unknown(&reply, &what);
            assert!(
                !text_of(main_of(&reply.body)).contains(number.as_str()),
                "{what} prints the number:\n{}",
                reply.body
            );
            for tag in common::start_tags(&reply.body, "<input") {
                if tag.contains(r#"name="back""#) {
                    assert_eq!(
                        attribute(tag, "value").as_deref(),
                        Some("/"),
                        "the appearance form of {what}"
                    );
                }
            }
        }
    }
    let uri = format!("/accounts/{assets}/edit");
    let reply = sam
        .post(&uri, &[("name", "Crafted"), ("shown_name", "Assets")])
        .await;
    assert_unknown(
        &reply,
        &format!("POST {uri} with the section's own name shown"),
    );
    let reply = sam
        .post(&uri, &[("name", "Assets"), ("shown_name", "Assets")])
        .await;
    assert_unknown(
        &reply,
        &format!("POST {uri} that changes nothing of the section"),
    );
    for (uri, back) in [
        ("/accounts?archived=987654", "/accounts"),
        (
            "/accounts?archived=shown&inside=987654",
            "/accounts?archived=shown",
        ),
    ] {
        let reply = sam.get(uri).await;
        assert_eq!(reply.status, StatusCode::OK, "status of {uri}");
        assert!(
            !text_of(&reply.body).contains("987654"),
            "{uri} prints the number:\n{}",
            reply.body
        );
        let backs: Vec<String> = common::start_tags(&reply.body, "<input")
            .into_iter()
            .filter(|tag| tag.contains(r#"name="back""#))
            .map(|tag| attribute(tag, "value").unwrap_or_default())
            .collect();
        assert!(
            !backs.is_empty() && backs.iter().all(|value| value == back),
            "the appearance form of {uri}: {backs:?}"
        );
    }
    let reply = sam.get("/accounts/new?under=assets&inside=987654").await;
    assert_see_other(&reply, "/accounts/new?under=assets", "an unknown inside");
    let reply = sam.get("/accounts/new?inside=0123").await;
    assert_see_other(&reply, "/accounts/new", "a malformed inside without under");
    for uri in ["/accounts/new", "/accounts/new?under=equity"] {
        let reply = sam.post(uri, &[("name", "Crafted")]).await;
        assert_unknown(&reply, &format!("POST {uri}"));
    }
    assert_eq!(rows(&app).await, before, "the accounts after the requests");
}

/// The change form shows what is saved: for Checking its name, Directly under Assets and
/// Joint, and sends them back as what it showed; a category has Name and Inside; a built-in
/// account and an archived one have the name alone, headed Change name; a section's address
/// gets the page for an unknown address.
///
/// Proves: Decision 134 with P4 and P12: GET edit shows three boxes for an account, two for a category, the name alone for a built-in and an archived account, and the 404 page for a section.
#[tokio::test]
async fn the_change_form_shows_what_is_saved() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let checking = app.account("Assets:Checking").await;
    let groceries = app.account("Expenses:Groceries").await;
    let opening = app.account("Equity:Opening Balances").await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;

    let reply = sam.get(&format!("/accounts/{checking}/edit")).await;
    assert_eq!(reply.status, StatusCode::OK, "status of the change form");
    assert_security_headers(&reply, "the change form");
    assert!(
        reply.shows("Change account"),
        "the heading:\n{}",
        reply.body
    );
    assert_eq!(value_of(&reply, "name"), "Checking", "the name box");
    for (field, expected) in [
        ("shown_name", String::from("Checking")),
        ("shown_inside", assets.to_string()),
        ("shown_owner", String::new()),
    ] {
        assert_eq!(value_of(&reply, field), expected, "the hidden {field}");
    }
    let main = main_of(&reply.body);
    assert!(
        main.contains(&format!(
            r#"<option value="{assets}" selected>Directly under Assets</option>"#
        )) && main.contains(r#"<option value="" selected>Joint</option>"#),
        "the change form does not show the place and the owner saved:\n{main}"
    );

    for (id, heading, boxes) in [
        (groceries, "Change category", ["name", "inside"].as_slice()),
        (opening, "Change name", ["name"].as_slice()),
        (old, "Change name", ["name"].as_slice()),
    ] {
        let reply = sam.get(&format!("/accounts/{id}/edit")).await;
        assert_eq!(reply.status, StatusCode::OK, "status of the form of {id}");
        assert!(reply.shows(heading), "the heading of {id}:\n{}", reply.body);
        let main = main_of(&reply.body);
        for name in ["name", "inside", "owner"] {
            assert_eq!(
                main.contains(&format!(r#"name="{name}""#)),
                boxes.contains(&name),
                "the box {name} on the form of {id}"
            );
        }
    }
    assert_unknown(
        &sam.get(&format!("/accounts/{assets}/edit")).await,
        "a section's change form",
    );
}

/// A change of the name alone, sent while another save gave the account to Alex, is one write
/// of the name, keeps Alex as its owner and leads to the account's page, which says once that
/// it was changed; a change that changes nothing writes nothing and leads to the page with no
/// message; a change of the owner alone then gives the account to Sam.
///
/// Proves: Decisions 134 and 137 with INV-28: a change of some boxes is one write and a 303 with its message; one that changes nothing writes nothing and sets no message.
#[tokio::test]
async fn a_change_saves_only_the_changed_boxes_and_says_so_once() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let checking = app.account("Assets:Checking").await;
    let alex = app.add_named_user("alex", "Alex").await;
    let edit = format!("/accounts/{checking}/edit");
    let page = format!("/accounts/{checking}");

    let mut sent = fields(&sam.get(&edit).await);
    give(&app, checking, Some(alex.id)).await;
    app.clock.advance(60);
    set(&mut sent, "name", "Everyday");
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_see_other(&reply, &page, "the change of the name");
    assert_eq!(
        reply.message_cookie().map(|(value, _)| value),
        Some(format!("account-changed.{checking}")),
        "the one-time message of the change"
    );
    let changed = row(&app, checking).await;
    assert_eq!(
        (
            changed.name.as_str(),
            changed.parent_id,
            changed.owner_user_id
        ),
        ("Everyday", Some(assets), Some(alex.id)),
        "the account after the change of its name"
    );
    assert_eq!(changed.updated_at, app.clock.now(), "when it was changed");
    let shown = sam.get(&page).await;
    assert_eq!(
        shown.notices(),
        [(
            String::from("status"),
            Notice::AccountChanged {
                word: Word::Account
            }
            .to_string()
        )],
        "the page after the change"
    );
    assert!(
        sam.get(&page).await.notices().is_empty(),
        "the page says it again"
    );

    let before = rows(&app).await;
    app.clock.advance(60);
    let sent = fields(&sam.get(&edit).await);
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_see_other(&reply, &page, "a change of nothing");
    assert_no_message(&reply, "a change of nothing");
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after a change of nothing"
    );

    let sam_id = admin(&app).await;
    let mut sent = fields(&sam.get(&edit).await);
    set(&mut sent, "owner", &sam_id.to_string());
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_see_other(&reply, &page, "the change of the owner");
    assert_eq!(
        reply.message_cookie().map(|(value, _)| value),
        Some(format!("account-changed.{checking}")),
        "the one-time message of the change of the owner"
    );
    let changed = row(&app, checking).await;
    assert_eq!(
        (changed.name.as_str(), changed.owner_user_id),
        ("Everyday", Some(sam_id)),
        "the account after the change of its owner"
    );
}

/// A rename sent after another save renamed the account answers 409 with the clash's
/// sentence, what is saved in the name alone, the person's name in its box and what is saved
/// sent back as what the form showed; nothing is written. Save mine then saves the person's
/// name.
///
/// Proves: Decision 134 and INV-28 with P2: a clash answers 409 with the clashing boxes only under Saved now, nothing written; Save mine then saves from what is saved.
#[tokio::test]
async fn a_clashing_change_answers_409_with_what_is_saved() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let edit = format!("/accounts/{checking}/edit");
    let mut sent = fields(&sam.get(&edit).await);
    rename(&app, checking, "Main").await;
    set(&mut sent, "name", "Daily");
    let before = rows(&app).await;
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the clash");
    assert_eq!(rows(&app).await, before, "the accounts after the clash");
    assert!(
        reply.shows(
            "Your change to Assets:Main was not saved because its name was changed while you were working, by another user or in another window."
        ),
        "the clash's sentence:\n{}",
        reply.body
    );
    let main = main_of(&reply.body);
    assert!(
        main.contains("<dt>Name</dt><dd>Main</dd>")
            && !main.contains("<dt>Inside</dt>")
            && !main.contains("<dt>Owner</dt>"),
        "Saved now does not hold the name alone:\n{main}"
    );
    assert_eq!(value_of(&reply, "name"), "Daily", "the person's name");
    assert_eq!(
        value_of(&reply, "shown_name"),
        "Main",
        "what is saved, sent back"
    );

    let mine = sam.post(&edit, &pairs(&fields(&reply))).await;
    assert_see_other(&mine, &format!("/accounts/{checking}"), "Save mine");
    assert_eq!(
        row(&app, checking).await.name,
        "Daily",
        "the name after Save mine"
    );
}

/// Another save renamed Checking, gave it to Alex and moved it inside Savings; this save
/// renamed it alone. The clash page holds the person's name, Alex as the owner and Savings as
/// the place, and Save mine writes the name alone: Alex stays the owner and Savings the place.
/// A clash on the owner after another rename holds the name saved now, and Save mine keeps it.
/// A clash on the owner of an account another save also archived names the owner saved now
/// under Saved now, under the heading Change account, its archived mark and its three boxes.
/// Another save renamed Coin, gave it to Alex, moved it inside Tin and archived both; this save
/// changed the name and the owner. The clash page keeps archived Tin chosen in Inside, and
/// once both are unarchived, Save mine leaves Coin where it was.
///
/// Proves: Decision 134 with P2: the other save changed name and owner, this save the name alone; after Save mine the owner is still the other save's.
#[tokio::test]
async fn save_mine_writes_only_the_persons_own_boxes() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let savings = app.account("Assets:Savings").await;
    let alex = app.add_named_user("alex", "Alex").await;
    let edit = format!("/accounts/{checking}/edit");
    let mut sent = fields(&sam.get(&edit).await);
    rename(&app, checking, "Main").await;
    give(&app, checking, Some(alex.id)).await;
    move_into(&app, checking, savings).await;
    set(&mut sent, "name", "Daily");
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{}" selected>Alex</option>"#,
            alex.id
        )),
        "the clash page does not hold the owner saved now:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{savings}" selected>Assets:Savings</option>"#
        )),
        "the clash page does not hold the place saved now:\n{}",
        reply.body
    );
    assert_eq!(
        value_of(&reply, "shown_owner"),
        alex.id.to_string(),
        "the owner saved now, sent back"
    );
    let mine = sam.post(&edit, &pairs(&fields(&reply))).await;
    assert_see_other(&mine, &format!("/accounts/{checking}"), "Save mine");
    let saved = row(&app, checking).await;
    assert_eq!(
        (saved.name.as_str(), saved.owner_user_id, saved.parent_id),
        ("Daily", Some(alex.id), Some(savings)),
        "the account after Save mine"
    );

    let sam_id = admin(&app).await;
    let mut sent = fields(&sam.get(&edit).await);
    rename(&app, checking, "Everyday").await;
    give(&app, checking, None).await;
    set(&mut sent, "owner", &sam_id.to_string());
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash on the owner:\n{}",
        reply.body
    );
    assert_eq!(
        value_of(&reply, "name"),
        "Everyday",
        "the clash page does not hold the name saved now"
    );
    let mine = sam.post(&edit, &pairs(&fields(&reply))).await;
    assert_see_other(
        &mine,
        &format!("/accounts/{checking}"),
        "Save mine of the owner",
    );
    let saved = row(&app, checking).await;
    assert_eq!(
        (saved.name.as_str(), saved.owner_user_id),
        ("Everyday", Some(sam_id)),
        "the account after Save mine of the owner"
    );

    let drawer = app.add_account("Assets", "Drawer", None).await;
    let drawer_edit = format!("/accounts/{drawer}/edit");
    let mut sent = fields(&sam.get(&drawer_edit).await);
    give(&app, drawer, Some(alex.id)).await;
    archive(&app, drawer).await;
    set(&mut sent, "owner", &sam_id.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&drawer_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash on the owner of an account archived meanwhile:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains("<dt>Owner</dt><dd>Alex</dd>"),
        "Saved now does not name the owner saved now:\n{}",
        reply.body
    );
    assert_eq!(rows(&app).await, before, "the accounts after the clash");
    let main = main_of(&reply.body);
    assert!(
        main.contains("<h1>Change account</h1>")
            && main.contains(r#"<small class="tag">archived</small>"#)
            && reply.shows("Saved now"),
        "the clash page of an account archived meanwhile lacks its heading, its archived mark or Saved now:\n{main}"
    );
    for name in ["name", "inside", "owner"] {
        assert!(
            !control(main, name).is_empty(),
            "the clash page of an account archived meanwhile lacks its {name} box"
        );
    }

    let tin = app.add_account("Assets", "Tin", None).await;
    app.add_account("Assets", "Pot", None).await;
    let coin = app.add_account("Assets:Pot", "Coin", None).await;
    let coin_edit = format!("/accounts/{coin}/edit");
    let mut sent = fields(&sam.get(&coin_edit).await);
    rename(&app, coin, "Coins").await;
    give(&app, coin, Some(alex.id)).await;
    move_into(&app, coin, tin).await;
    archive(&app, coin).await;
    archive(&app, tin).await;
    set(&mut sent, "name", "Coin jar");
    set(&mut sent, "owner", &sam_id.to_string());
    let reply = sam.post(&coin_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of the clash on the name and the owner of an account archived inside an archived one:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{tin}" selected>Assets:Tin (archived)</option>"#
        )),
        "the clash page does not keep the archived place that holds the account, chosen:\n{}",
        reply.body
    );
    unarchive(&app, tin).await;
    unarchive(&app, coin).await;
    let mine = sam.post(&coin_edit, &pairs(&fields(&reply))).await;
    assert_see_other(
        &mine,
        &format!("/accounts/{coin}"),
        "Save mine after both were unarchived",
    );
    let saved = row(&app, coin).await;
    assert_eq!(
        (saved.name.as_str(), saved.owner_user_id, saved.parent_id),
        ("Coin jar", Some(sam_id), Some(tin)),
        "the account after Save mine: it stays where it was"
    );
}

/// A change refused for a colon in the name and for an archived place answers 422 with the
/// line at the top and each sentence under its box, the two boxes alone marked, what was
/// typed kept, the archived place chosen and marked, and the values the form showed sent back
/// as they were sent; nothing is written. A move into a place that went inside the account
/// meanwhile comes back with that place kept and chosen, so the form sent again as it came
/// back is refused again and moves nothing.
///
/// Proves: Decisions 134 and 136 with INV-28 and P2: a refused change shows each problem under its box with 422, carries the shown values the request sent, and writes nothing.
#[tokio::test]
async fn a_refused_change_answers_422_with_the_form_as_typed() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let checking = app.account("Assets:Checking").await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;
    let edit = format!("/accounts/{checking}/edit");
    let mut sent = fields(&sam.get(&edit).await);
    set(&mut sent, "name", "Checking: joint");
    set(&mut sent, "inside", &old.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the refused change");
    assert_eq!(rows(&app).await, before, "the accounts after the refusal");
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            Refusal::AccountNotSaved {
                word: Word::Account
            }
            .to_string()
        )],
        "the line at the top"
    );
    for refusal in [
        AccountError::Name {
            refused: refused_on("Assets:Checking", Action::Renamed),
            error: NameError::Colon,
        },
        AccountError::ParentArchived {
            refused: refused_on("Assets:Checking", Action::Moved),
            parent: String::from("Assets:Old savings"),
        },
    ] {
        assert!(
            reply.shows(&refusal.to_string()),
            "the refused change does not say {refusal}:\n{}",
            reply.body
        );
    }
    assert_eq!(
        (
            invalid(&reply, "name"),
            invalid(&reply, "inside"),
            invalid(&reply, "owner")
        ),
        (true, true, false),
        "the boxes marked"
    );
    assert_eq!(value_of(&reply, "name"), "Checking: joint", "the name kept");
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{old}" selected>Assets:Old savings (archived)</option>"#
        )),
        "the archived place chosen is not kept:\n{}",
        reply.body
    );
    for (field, expected) in [
        ("shown_name", String::from("Checking")),
        ("shown_inside", assets.to_string()),
        ("shown_owner", String::new()),
    ] {
        assert_eq!(value_of(&reply, field), expected, "the hidden {field}");
    }

    let savings = app.account("Assets:Savings").await;
    let cash = app.account("Assets:Cash").await;
    let savings_edit = format!("/accounts/{savings}/edit");
    let mut sent = fields(&sam.get(&savings_edit).await);
    move_into(&app, cash, savings).await;
    set(&mut sent, "inside", &cash.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&savings_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a move into a place that went inside it:\n{}",
        reply.body
    );
    assert!(
        main_of(&reply.body).contains(&format!(
            r#"<option value="{cash}" selected>Assets:Savings:Cash</option>"#
        )),
        "the place chosen is not kept:\n{}",
        reply.body
    );
    let again = sam.post(&savings_edit, &pairs(&fields(&reply))).await;
    assert_eq!(
        again.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of the returned form sent again as it came back:\n{}",
        again.body
    );
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the form was sent again"
    );
}

/// A rename refused for its name after another save gave the account to Alex comes back with
/// the owner the person saw, Joint, and the owner the form showed; corrected and sent again,
/// it saves the name and Alex stays the owner, as the first send would have left it. A
/// name-only form shown while the account was archived, sent after another window unarchived
/// it and refused for its name, comes back with what is saved in the boxes it did not carry,
/// in the boxes and as what they showed; mended and sent again, it writes the name alone.
///
/// Proves: Decision 134 with P2: a save refused for its name after another save changed the owner, corrected and sent again, keeps the other save's owner and meets the clash rule as the first send would have.
#[tokio::test]
async fn a_refused_change_sent_again_does_not_undo_another_save() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let alex = app.add_named_user("alex", "Alex").await;
    let edit = format!("/accounts/{checking}/edit");
    let mut sent = fields(&sam.get(&edit).await);
    give(&app, checking, Some(alex.id)).await;
    set(&mut sent, "name", "Bad: name");
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of the refused rename:\n{}",
        reply.body
    );
    assert_eq!(
        value_of(&reply, "shown_owner"),
        "",
        "the owner the form showed, sent back"
    );
    let mut again = fields(&reply);
    set(&mut again, "name", "Daily");
    let reply = sam.post(&edit, &pairs(&again)).await;
    assert_see_other(
        &reply,
        &format!("/accounts/{checking}"),
        "the corrected rename",
    );
    let saved = row(&app, checking).await;
    assert_eq!(
        (saved.name.as_str(), saved.owner_user_id),
        ("Daily", Some(alex.id)),
        "the account after the corrected rename"
    );

    let savings = app.account("Assets:Savings").await;
    let spare = app
        .add_account("Assets:Savings", "Spare", Some(alex.id))
        .await;
    archive(&app, spare).await;
    let spare_edit = format!("/accounts/{spare}/edit");
    let mut sent = fields(&sam.get(&spare_edit).await);
    unarchive(&app, spare).await;
    set(&mut sent, "name", "Bad: spare");
    let reply = sam.post(&spare_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of the name-only form refused after the unarchive:\n{}",
        reply.body
    );
    let main = main_of(&reply.body);
    assert!(
        main.contains(&format!(
            r#"<option value="{savings}" selected>Assets:Savings</option>"#
        )) && main.contains(&format!(
            r#"<option value="{}" selected>Alex</option>"#,
            alex.id
        )),
        "the boxes the name-only form did not carry do not hold what is saved:\n{main}"
    );
    for (field, expected) in [
        ("shown_inside", savings.to_string()),
        ("shown_owner", alex.id.to_string()),
    ] {
        assert_eq!(
            value_of(&reply, field),
            expected,
            "the hidden {field} of a box the request did not carry"
        );
    }
    let mut again = fields(&reply);
    set(&mut again, "name", "Spare jar");
    let reply = sam.post(&spare_edit, &pairs(&again)).await;
    assert_see_other(
        &reply,
        &format!("/accounts/{spare}"),
        "the mended name-only form",
    );
    let saved = row(&app, spare).await;
    assert_eq!(
        (saved.name.as_str(), saved.parent_id, saved.owner_user_id),
        ("Spare jar", Some(savings), Some(alex.id)),
        "the account after the mended name-only form: the name alone written"
    );
}

/// A full change form of Old savings, saved with another place after another save archived
/// the account, answers 422 with the name-only form and the sentence at the top; nothing is
/// written. Its name alone is then saved from that form, and the account stays archived where
/// it was. A change of owner from a full form after the archive is refused the same way. A move
/// sent after another save moved and archived the account clashes: the clash page holds the
/// place chosen beside the place saved now, and Save mine is then refused at the top.
///
/// Proves: Decision 134 with P4: a full form saved after another save archived the account answers 422 with the name-only form and the message at the top, nothing written; its name alone is then saved.
#[tokio::test]
async fn an_archived_account_takes_a_new_name_alone() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let savings = app.account("Assets:Savings").await;
    let old = app.add_account("Assets", "Old savings", None).await;
    let edit = format!("/accounts/{old}/edit");
    let mut sent = fields(&sam.get(&edit).await);
    archive(&app, old).await;
    set(&mut sent, "inside", &savings.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status:\n{}",
        reply.body
    );
    assert_no_message(&reply, "the refused move");
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the refused move"
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            AccountError::ArchivedMeanwhile {
                account: String::from("Assets:Old savings"),
                kind: Kind::Asset,
            }
            .to_string()
        )],
        "the sentence at the top"
    );
    let main = main_of(&reply.body);
    assert!(
        main.contains(r#"name="name""#)
            && !main.contains(r#"name="inside""#)
            && !main.contains(r#"name="owner""#),
        "the form is not the name-only form:\n{main}"
    );

    let mut again = fields(&reply);
    set(&mut again, "name", "Older savings");
    let reply = sam.post(&edit, &pairs(&again)).await;
    assert_see_other(&reply, &format!("/accounts/{old}"), "the rename");
    let renamed = row(&app, old).await;
    assert_eq!(
        (renamed.name.as_str(), renamed.parent_id),
        ("Older savings", Some(assets)),
        "the archived account after its rename"
    );
    assert!(
        renamed.archived_at.is_some(),
        "the account is still archived"
    );

    let sam_id = admin(&app).await;
    let drawer = app.add_account("Assets", "Drawer", None).await;
    let drawer_edit = format!("/accounts/{drawer}/edit");
    let mut sent = fields(&sam.get(&drawer_edit).await);
    archive(&app, drawer).await;
    set(&mut sent, "owner", &sam_id.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&drawer_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a change of owner after the archive:\n{}",
        reply.body
    );
    assert_eq!(
        reply.notices(),
        [(
            String::from("alert"),
            AccountError::ArchivedMeanwhile {
                account: String::from("Assets:Drawer"),
                kind: Kind::Asset,
            }
            .to_string()
        )],
        "the sentence at the top of a change of owner after the archive"
    );
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the refused owner"
    );

    let checking = app.account("Assets:Checking").await;
    let shelf = app.add_account("Assets", "Shelf", None).await;
    let shelf_edit = format!("/accounts/{shelf}/edit");
    let mut sent = fields(&sam.get(&shelf_edit).await);
    move_into(&app, shelf, savings).await;
    archive(&app, shelf).await;
    set(&mut sent, "inside", &checking.to_string());
    let before = rows(&app).await;
    let reply = sam.post(&shelf_edit, &pairs(&sent)).await;
    assert_eq!(
        reply.status,
        StatusCode::CONFLICT,
        "status of a move after another move and the archive:\n{}",
        reply.body
    );
    let main = main_of(&reply.body);
    assert!(
        main.contains(&format!(
            r#"<option value="{checking}" selected>Assets:Checking</option>"#
        )) && main.contains("<dt>Inside</dt><dd>Assets:Savings</dd>"),
        "the clash page does not hold the place chosen beside the place saved now:\n{main}"
    );
    let mine = sam.post(&shelf_edit, &pairs(&fields(&reply))).await;
    assert_eq!(
        mine.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of Save mine of a move of an archived account:\n{}",
        mine.body
    );
    assert_eq!(
        mine.notices(),
        [(
            String::from("alert"),
            AccountError::ArchivedMeanwhile {
                account: String::from("Assets:Savings:Shelf"),
                kind: Kind::Asset,
            }
            .to_string()
        )],
        "the sentence at the top of Save mine"
    );
    assert_no_message(&mine, "Save mine of a move of an archived account");
    assert_eq!(rows(&app).await, before, "the accounts after Save mine");
}

/// GET of Archive shows the confirmation and changes nothing, and so does GET of Unarchive of
/// an archived account, each button posting to its own address; GET of Unarchive of an account that is not archived, and of Archive of
/// one that is, lead to its page. A section and a built-in account have neither address, GET
/// or POST: the page for an unknown address, nothing written.
///
/// Proves: Decision 135 with P6: GET of archive and unarchive shows the confirmation and changes nothing; one with nothing to do is a 303 to the page; a section or a built-in account gets the 404 page, GET and POST.
#[tokio::test]
async fn archive_and_unarchive_ask_first() {
    let (app, mut sam) = household().await;
    let assets = app.account("Assets").await;
    let opening = app.account("Equity:Opening Balances").await;
    let old = app.add_account("Assets", "Old savings", None).await;
    let page = format!("/accounts/{old}");
    let before = rows(&app).await;
    let reply = sam.get(&format!("{page}/archive")).await;
    assert_eq!(reply.status, StatusCode::OK, "status of the confirmation");
    assert!(
        reply.shows("Archive Old savings?")
            && main_of(&reply.body).contains(&format!(r#"action="{page}/archive""#)),
        "the confirmation of Archive:\n{}",
        reply.body
    );
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the confirmation"
    );
    assert_see_other(
        &sam.get(&format!("{page}/unarchive")).await,
        &page,
        "the confirmation of Unarchive of an account not archived",
    );

    archive(&app, old).await;
    assert_see_other(
        &sam.get(&format!("{page}/archive")).await,
        &page,
        "the confirmation of Archive of an archived account",
    );
    let reply = sam.get(&format!("{page}/unarchive")).await;
    assert_eq!(reply.status, StatusCode::OK, "status of the confirmation");
    assert!(
        reply.shows("Unarchive Old savings?")
            && main_of(&reply.body).contains(&format!(r#"action="{page}/unarchive""#))
            && !main_of(&reply.body).contains(&format!(r#"action="{page}/archive""#)),
        "the confirmation of Unarchive:\n{}",
        reply.body
    );

    let before = rows(&app).await;
    for id in [assets, opening] {
        for action in ["archive", "unarchive"] {
            let uri = format!("/accounts/{id}/{action}");
            assert_unknown(&sam.get(&uri).await, &format!("GET {uri}"));
            assert_unknown(&sam.post(&uri, &[]).await, &format!("POST {uri}"));
        }
    }
    assert_eq!(rows(&app).await, before, "the accounts after the requests");
}

/// POST of Archive archives Old savings in one write and leads to its page, which says once
/// that it was archived; a second Archive finds nothing to do and leads to the page with no
/// message and nothing written; Unarchive then does the same the other way.
///
/// Proves: Decisions 135 and 137 with INV-12, INV-28 and P6: POST archives or unarchives in one write with its message; a repeat finds nothing to do, a 303 to the page with no message cookie.
#[tokio::test]
async fn archive_and_unarchive_act_and_a_second_counts_as_done() {
    let (app, mut sam) = household().await;
    let old = app.add_account("Assets", "Old savings", None).await;
    let page = format!("/accounts/{old}");
    for (action, archived, notice) in [
        (
            "archive",
            true,
            Notice::AccountArchived {
                word: Word::Account,
            },
        ),
        (
            "unarchive",
            false,
            Notice::AccountUnarchived {
                word: Word::Account,
            },
        ),
    ] {
        app.clock.advance(60);
        let reply = sam.post(&format!("{page}/{action}"), &[]).await;
        assert_see_other(&reply, &page, action);
        assert_eq!(
            reply.message_cookie().map(|(value, _)| value),
            Some(format!("account-{action}d.{old}")),
            "the one-time message of {action}"
        );
        let saved = row(&app, old).await;
        assert_eq!(
            saved.archived_at.is_some(),
            archived,
            "archived after {action}"
        );
        assert_eq!(saved.updated_at, app.clock.now(), "when {action} was done");
        let shown = sam.get(&page).await;
        assert_eq!(
            shown.notices(),
            [(String::from("status"), notice.to_string())],
            "the page after {action}"
        );

        let before = rows(&app).await;
        app.clock.advance(60);
        let reply = sam.post(&format!("{page}/{action}"), &[]).await;
        assert_see_other(&reply, &page, &format!("a second {action}"));
        assert_no_message(&reply, &format!("a second {action}"));
        assert_eq!(
            rows(&app).await,
            before,
            "the accounts after a second {action}"
        );
    }
}

/// Archive of Checking, which holds 2,480.15, of Savings, which holds an account that is not
/// archived, and Unarchive of a Jar inside an archived account are each refused at the top of
/// the account's page with 422 in the core's words; nothing is written and no message set.
///
/// Proves: Decision 136 with INV-12 and INV-28: a refused archive or unarchive shows the refusal at the top of the account's page, 422, nothing written.
#[tokio::test]
async fn a_refused_archive_answers_422_on_the_accounts_page() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let savings = app.account("Assets:Savings").await;
    app.open_balance(checking, "2027-01-01", 248_015).await;
    app.add_account("Assets:Savings", "Emergency fund", None)
        .await;
    let old = app.add_account("Assets", "Old savings", None).await;
    let jar = app.add_account("Assets:Old savings", "Jar", None).await;
    archive(&app, jar).await;
    archive(&app, old).await;
    let before = rows(&app).await;
    for (uri, heading, refusal) in [
        (
            format!("/accounts/{checking}/archive"),
            "<h1>Checking</h1>",
            AccountError::BalanceNotZero {
                kind: Kind::Asset,
                account: String::from("Assets:Checking"),
                balance: Money::from_minor(248_015),
                digits: MinorDigits::TWO,
            },
        ),
        (
            format!("/accounts/{savings}/archive"),
            "<h1>Savings</h1>",
            AccountError::ChildrenNotArchived {
                account: String::from("Assets:Savings"),
                child: String::from("Assets:Savings:Emergency fund"),
            },
        ),
        (
            format!("/accounts/{jar}/unarchive"),
            "<h1>Jar</h1>",
            AccountError::ParentArchivedUnarchiveFirst {
                account: String::from("Assets:Old savings:Jar"),
                parent: String::from("Assets:Old savings"),
            },
        ),
    ] {
        let reply = sam.post(&uri, &[]).await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of POST {uri}:\n{}",
            reply.body
        );
        assert_eq!(
            reply.notices(),
            [(String::from("alert"), refusal.to_string())],
            "the refusal at the top of POST {uri}"
        );
        assert!(
            main_of(&reply.body).contains(heading),
            "POST {uri} is not the account's page:\n{}",
            reply.body
        );
        assert_no_message(&reply, &format!("POST {uri}"));
    }
    assert_eq!(rows(&app).await, before, "the accounts after the refusals");
}

/// Each account form, posted from another site, is refused with 403 and writes nothing; the
/// same requests from Nage's own pages are accepted.
///
/// Proves: Decision 110 and INV-28: every account POST from another origin is refused with 403 and leaves no row, beside a control the route accepts.
#[tokio::test]
async fn every_account_form_is_refused_from_another_site() {
    let (app, mut sam) = household().await;
    let checking = app.account("Assets:Checking").await;
    let spare = app.add_account("Assets", "Spare", None).await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;
    let posts: [(String, Vec<(&str, &str)>); 4] = [
        (
            String::from("/accounts/new?under=assets"),
            vec![("name", "Crafted")],
        ),
        (
            format!("/accounts/{checking}/edit"),
            vec![("name", "Renamed"), ("shown_name", "Checking")],
        ),
        (format!("/accounts/{spare}/archive"), Vec::new()),
        (format!("/accounts/{old}/unarchive"), Vec::new()),
    ];
    let before = rows(&app).await;
    for (uri, form) in &posts {
        let reply = sam
            .send(Req::form(uri, form).header("origin", "https://elsewhere.example"))
            .await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "status of POST {uri} from another site"
        );
        assert!(
            reply.shows(&Refusal::CrossSite.to_string()),
            "POST {uri} from another site is not the 403 page:\n{}",
            reply.body
        );
        assert_no_message(&reply, &format!("POST {uri} from another site"));
    }
    assert_eq!(rows(&app).await, before, "the accounts after the refusals");
    for (uri, form) in &posts {
        let reply = sam.post(uri, form).await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of POST {uri} from Nage's own page:\n{}",
            reply.body
        );
    }
    assert_ne!(
        rows(&app).await,
        before,
        "the accounts after the accepted posts"
    );
}

/// With the writer held and a short queue, each of the four account forms answers 503 with
/// `Retry-After: 5` and the busy page, sets no cookie and writes nothing.
///
/// Proves: INV-28 and SPEC 5.4 with P12: with the writer held, each of the four account POST routes answers 503, changes no row and sets no message cookie.
#[tokio::test]
async fn every_account_save_answers_busy_and_leaves_no_trace() {
    let app = App::start_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await;
    app.set_up_as(true).await;
    let mut sam = app.signed_in(ADMIN, PASSWORD).await;
    let checking = app.account("Assets:Checking").await;
    let spare = app.add_account("Assets", "Spare", None).await;
    let old = app.add_account("Assets", "Old savings", None).await;
    archive(&app, old).await;
    let posts: [(String, Vec<(&str, &str)>); 4] = [
        (
            String::from("/accounts/new?under=assets"),
            vec![("name", "Crafted")],
        ),
        (
            format!("/accounts/{checking}/edit"),
            vec![("name", "Renamed"), ("shown_name", "Checking")],
        ),
        (format!("/accounts/{spare}/archive"), Vec::new()),
        (format!("/accounts/{old}/unarchive"), Vec::new()),
    ];
    let before = rows(&app).await;
    let held = app.write().await;
    let mut replies = Vec::new();
    for (uri, form) in &posts {
        replies.push((uri, sam.post(uri, form).await));
    }
    drop(held);
    for (uri, reply) in &replies {
        assert_eq!(
            reply.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "status of POST {uri} while the writer is held:\n{}",
            reply.body
        );
        assert_eq!(
            reply.header("retry-after"),
            Some("5"),
            "Retry-After of {uri}"
        );
        assert!(
            reply.shows("Nage is busy"),
            "POST {uri} is not the busy page:\n{}",
            reply.body
        );
        assert!(
            reply.set_cookies().is_empty(),
            "POST {uri} sets a cookie: {:?}",
            reply.set_cookies()
        );
    }
    assert_eq!(
        rows(&app).await,
        before,
        "the accounts after the busy posts"
    );
}

/// A book with no account under Assets and Liabilities shows Add an account on the home page,
/// a form that asks the choice page with an empty query; once its only account there is
/// archived, the home page says so and links to the list with the archived accounts shown.
///
/// Proves: Decision 137 with P8 and P12: a book with no accounts shows Add an account, leading to the choice page; one whose accounts are all archived says so, its link leading to the list with them shown.
#[tokio::test]
async fn the_home_page_leads_to_the_accounts_from_its_empty_states() {
    let app = App::start().await;
    app.set_up_as(false).await;
    let mut sam = app.signed_in(ADMIN, PASSWORD).await;
    let home = sam.get("/").await;
    let main = main_of(&home.body);
    assert!(
        main.contains(r#"<form method="get" action="/accounts/new">"#)
            && main.contains(r#"<button type="submit">Add an account</button>"#),
        "the empty home page does not offer Add an account:\n{main}"
    );
    let what = sam.get("/accounts/new?").await;
    assert_eq!(what.status, StatusCode::OK, "status of the choice page");
    assert!(
        what.shows("What do you want to add?"),
        "Add an account does not lead to the choice page:\n{}",
        what.body
    );

    let cash = app.add_account("Assets", "Cash", None).await;
    archive(&app, cash).await;
    let home = sam.get("/").await;
    assert!(
        home.shows("Every account under Assets and Liabilities is archived")
            && main_of(&home.body)
                .contains(r#"<a href="/accounts?archived=shown">See the accounts</a>"#),
        "the home page does not lead to the archived accounts:\n{}",
        home.body
    );
    let list = sam.get("/accounts?archived=shown").await;
    assert_eq!(list.status, StatusCode::OK, "status of the list");
    assert!(
        main_of(&list.body).contains(">Cash<") && list.shows("Hide archived"),
        "the list does not show the archived account:\n{}",
        list.body
    );
}
