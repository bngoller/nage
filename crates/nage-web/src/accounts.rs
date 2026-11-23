//! The account pages (SPEC 6.2, W2, decisions 128 to 137 and the pause's rulings P1 to P14): the
//! chart of accounts, the ledger's accounts and categories, at `/accounts` and below, behind the
//! sign-in. Not to be confused with the module `account`, which is the signed-in user's own
//! password at `/account/password`.
//!
//! The addresses (decision 128): `GET /accounts` the list (`?archived=shown` puts the archived
//! accounts in their places); `GET /accounts/new` the page "What do you want to add?" (also
//! with an empty query, as the home page's Add an account sends it), and with
//! `?under=<section>` (`assets`, `liabilities`, `income` or `expenses`) and optionally
//! `&inside=<number>` the create form, which `POST /accounts/new?under=<section>` (keeping
//! `&inside=` when the form was opened with it) sends; `GET /accounts/<number>` an account's
//! page; `GET` and `POST /accounts/<number>/edit` the change form; `GET
//! /accounts/<number>/archive` and `/unarchive` the confirmations, a `POST` to the same
//! addresses acting. A number is well formed only as Nage prints it ([`well_formed_number`]),
//! judged on the path's segment as it was sent, before any percent-decoding ([`path_number`]),
//! so `/accounts/%39` is no second address of account 9 and `/accounts/%FF` no bare 400.
//! An unknown, malformed or root number, an unknown section, a section's or a built-in
//! account's archive and unarchive addresses, and a `POST /accounts/new` with a missing or
//! unknown section get the page for an unknown address (decision 118), which never prints
//! them; an `inside` without `under`, or one the form's list does not offer, answers 303 to the
//! same address without it, so no such number is ever printed, the menu's appearance form
//! included: each page's appearance form returns to the page's own address as Nage writes it,
//! never to the request's text.
//!
//! Every form is a plain post with no htmx attribute (decision 116): 303 after a save, 422 with
//! the form again and every problem under its box, what was typed kept, a refusal that belongs
//! to no box in the message at the top, 409 for a change that clashed with another save
//! (decision 134). A handler reads only the boxes its form has (the pause's P3): the name
//! always; Inside unless the account is built in; Owner only under Assets and Liabilities and
//! not for a built-in account; on an archived account's change, Inside and Owner are passed
//! on only so that the core refuses a move or a change of owner made from a full form shown
//! before the archive (P4), and are never saved. Other fields never reach the core. Each request
//! that writes runs in one write transaction, all or none, every refusal decided before the
//! first write, and a refused, clashing, busy or failed one changes no row and sets no one-time
//! message (INV-28); a save that wrote something sets one of the account messages with the
//! account's number (decision 137, P7), which that account's page shows once. Every page
//! carries the menu with Accounts marked; every other page a GET renders here clears a waiting
//! account message unseen, while a message of the three older codes shows on every account
//! page, the account's own included. The opening balance's pages under `/accounts/<number>`
//! live in `opening_balance` (decisions 140 to 146).

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use nage_core::accounts::{
    self, AccountBoxes, AccountError, AccountId, BoxName, ChangeError, Changed, Clash, Kind, Sent,
    SentBoxes, Word,
};
use nage_core::chart::{self, EditForm, Kept, TrailStep};
use nage_core::sessions::SignedIn;
use nage_core::storage;
use nage_core::users::UserId;
use serde::Deserialize;

use crate::flash::{self, Flash};
use crate::messages::Refusal;
use crate::pages::{
    AccountClashPage, AccountConfirmPage, AccountEditPage, AccountNewPage, AccountPage,
    AccountProblems, AccountTyped, AccountsPage, Entry, Layout, Menu, Message, NewWhatPage, render,
    render_with, shown_from_saved,
};
use crate::{AppError, Ctx, clock, cookie, forms, not_found};

/// The number `text` names, when it is written exactly as Nage prints one: ASCII digits only,
/// no sign, no leading zero, within 64 bits.
pub(crate) fn well_formed(text: &str) -> Option<i64> {
    let well_formed = text.bytes().all(|byte| byte.is_ascii_digit()) && !text.starts_with('0');
    if !well_formed {
        return None;
    }
    text.parse::<i64>().ok()
}

/// The account number `text` names, when it is written exactly as Nage prints one (decision
/// 128, the pause's P6): ASCII digits only, no sign, no leading zero, within 64 bits. So
/// `/accounts/+12`, `/accounts/012` and a number too large get the page for an unknown address
/// instead of a second address of account 12.
pub(crate) fn well_formed_number(text: &str) -> Option<AccountId> {
    well_formed(text).map(AccountId)
}

/// The account number of an account address (`/accounts/<number>` and the addresses below it),
/// judged on the path's segment exactly as the request sent it, before any percent-decoding
/// (decision 128): `/accounts/%39` and `/accounts/%FF` name no account, and get the page for an
/// unknown address like any other malformed number.
pub(crate) fn path_number(uri: &Uri) -> Option<AccountId> {
    uri.path().split('/').nth(2).and_then(well_formed_number)
}

/// The create form as posted (the pause's P6). A field the request does not carry is empty: an
/// empty `inside` means directly under the section, an empty `owner` Joint; one that is not a
/// well-formed number is refused under its box as not in the list.
#[derive(Deserialize)]
pub(crate) struct NewAccountForm {
    #[serde(default)]
    pub(crate) name: String,
    /// The number of the account it goes inside, or of its section.
    #[serde(default)]
    pub(crate) inside: String,
    /// The owner's number; empty for Joint. Read only under Assets and Liabilities.
    #[serde(default)]
    pub(crate) owner: String,
}

/// The change form as posted: the boxes as sent, and what the form showed when it was opened
/// (decision 134, the pause's P2). A box the request does not carry is `None` and counts as
/// unchanged; a shown value that is missing or malformed equals nothing saved.
#[derive(Deserialize)]
pub(crate) struct EditAccountForm {
    pub(crate) name: Option<String>,
    pub(crate) inside: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) shown_name: Option<String>,
    pub(crate) shown_inside: Option<String>,
    pub(crate) shown_owner: Option<String>,
}

/// `GET /accounts`: the list (decision 131) as of today in the configured zone, read in one
/// read transaction (`nage_core::chart::chart`); `?archived=shown` shows the archived accounts
/// in their places, any other query hides them. The page clears an account message unseen.
pub(crate) async fn get_list(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let archived_shown = query_value(&uri, "archived") == Some("shown");
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let chart = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::chart(&mut read, today, archived_shown).await?
    };
    let pending = flash::pending(&method, &headers);
    let back = if archived_shown {
        "/accounts?archived=shown"
    } else {
        "/accounts"
    };
    let page = AccountsPage::new(
        layout(&signed_in, back.to_owned(), pending.message(true)),
        chart,
    );
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// `GET /accounts/new` (decision 132, the pause's P6 and P10): with no query or an empty one,
/// the page "What do you want to add?"; with `?under=<section>` the create form of that
/// section, empty, Joint selected; with `&inside=<number>` of a place the form's list offers,
/// that place selected and the trail through its path; an unknown section is the 404 page;
/// `inside` without `under`, or one the list does not offer (unknown, malformed, archived,
/// built in, another section, too deep), a 303 to the same address without it.
pub(crate) async fn get_new(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let inside = query_value(&uri, "inside");
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let Some(under) = query_value(&uri, "under") else {
        if inside.is_some() {
            return Ok(see_other(NEW_PATH));
        }
        let pending = flash::pending(&method, &headers);
        let page = NewWhatPage::new(layout(
            &signed_in,
            NEW_PATH.to_owned(),
            pending.message(true),
        ));
        return Ok(pending.settle(render(&page), secure));
    };
    let Some(kind) = section(under) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let here = new_address(kind);
    let mut read = ctx.state.storage.begin_read().await?;
    let form = chart::new_form(&mut read, kind, Kept::default()).await?;
    let mut chosen = form.section.id;
    let mut path = Vec::new();
    if let Some(inside) = inside {
        let offered = well_formed_number(inside)
            .filter(|id| *id == form.section.id || form.places.iter().any(|place| place.id == *id));
        let Some(id) = offered else {
            return Ok(see_other(&here));
        };
        chosen = id;
        if id != form.section.id {
            let place = chart::edit_form(&mut read, id, Kept::default()).await?;
            path = place_path(place, kind);
        }
    }
    drop(read);
    let back = if path.is_empty() {
        here
    } else {
        format!("{here}&inside={chosen}")
    };
    let typed = AccountTyped {
        inside: chosen.to_string(),
        ..AccountTyped::default()
    };
    let pending = flash::pending(&method, &headers);
    let page = AccountNewPage::new(
        layout(&signed_in, back, pending.message(true)),
        form,
        typed,
        AccountProblems::default(),
        path,
    );
    Ok(pending.settle(render(&page), secure))
}

/// `POST /accounts/new?under=<section>` (decision 132, the pause's P3): a missing or unknown
/// section is the 404 page; one write transaction runs
/// `nage_core::accounts::create_from_form`, which gathers every refusal of the form; refused,
/// the form comes back with every problem under its box, what was typed kept (a place or an
/// owner the list no longer offers as one extra choice, marked), 422, nothing written; it keeps
/// the trail, the way back and `&inside=` of the place it was opened inside only when that
/// place is in the form's list, as the GET of the same address would. Success
/// is a 303 to the new account's page with the one-time message `Flash::AccountCreated` and
/// its number.
pub(crate) async fn post_new(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    form: Result<Form<NewAccountForm>, FormRejection>,
) -> Result<Response, AppError> {
    // The section is judged before the body is answered (decision 148, leftover N1's order): the
    // extractor has read and parsed the body already, but a body that is no form is answered
    // only after the section.
    let Some(kind) = query_value(&uri, "under").and_then(section) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => return Ok(forms::unreadable(rejection)),
    };
    let reads_owner = kind.allows_owner();
    let typed = AccountTyped {
        name: form.name.clone(),
        inside: form.inside.clone(),
        owner: if reads_owner {
            form.owner.clone()
        } else {
            String::new()
        },
    };
    let sent = SentBoxes {
        name: Sent::Value(form.name),
        inside: if form.inside.is_empty() {
            Sent::Absent
        } else {
            place_sent(&form.inside)
        },
        owner: if reads_owner && !form.owner.is_empty() {
            owner_sent(&form.owner)
        } else {
            Sent::Absent
        },
    };
    let now = ctx.state.clock.now()?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut tx = ctx.state.storage.begin_write().await?;
    match accounts::create_from_form(&mut tx, signed_in.user.id, now, kind, &sent).await {
        Ok(account) => {
            tx.commit().await.map_err(storage::Error::Sqlx)?;
            Ok(with_message(
                &account_address(account.id),
                Flash::AccountCreated(account.id),
                secure,
            ))
        }
        Err(AccountError::Form(refusals)) => {
            let new_form = chart::new_form(&mut tx, kind, kept(&sent)).await?;
            let opened = query_value(&uri, "inside").and_then(well_formed_number);
            let opened = opened.filter(|id| new_form.places.iter().any(|place| place.id == *id));
            let path = match opened {
                Some(id) => place_path(chart::edit_form(&mut tx, id, Kept::default()).await?, kind),
                None => Vec::new(),
            };
            drop(tx);
            let here = new_address(kind);
            let back = match path.last() {
                Some(step) => format!("{here}&inside={}", step.id),
                None => here,
            };
            let message = refusals.top.as_ref().map_or_else(
                || {
                    Refusal::AccountNotCreated {
                        word: Word::of(kind),
                    }
                    .to_string()
                },
                ToString::to_string,
            );
            let problems = AccountProblems {
                name: refusals.name.as_ref().map(ToString::to_string),
                inside: refusals.inside.as_ref().map(ToString::to_string),
                owner: refusals.owner.as_ref().map(ToString::to_string),
            };
            let page = AccountNewPage::new(
                layout(&signed_in, back, alert(message)),
                new_form,
                typed,
                problems,
                path,
            );
            Ok(render_with(StatusCode::UNPROCESSABLE_ENTITY, &page))
        }
        Err(error) => Err(error.into()),
    }
}

/// `GET /accounts/<number>`: the account's page (decision 133) as of today, showing and
/// clearing an account message only when the message's number is its own, and a message of
/// the three older codes as every other page does (`Pending::account_message`, the pause's
/// P7, decision 137); an unknown, malformed or root number is the page for an unknown address.
pub(crate) async fn get_account(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let view = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::account_view(&mut read, today, id).await?
    };
    let Some(view) = view else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let pending = flash::pending(&method, &headers);
    let message = pending.account_message(id, view.word);
    let page = AccountPage::new(layout(&signed_in, account_address(id), message), view);
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// `GET /accounts/<number>/edit`: the change form with what is saved (decision 134): Name,
/// Inside and Owner for an account, Name and Inside for a category, the name alone for a
/// built-in and an archived account (the pause's P4); the 404 page for a section.
pub(crate) async fn get_edit(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let form = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::edit_form(&mut read, id, Kept::default()).await?
    };
    let Some(form) = form else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let typed = AccountTyped {
        name: form.saved.name.clone(),
        inside: form
            .saved
            .parent
            .map(|parent| parent.to_string())
            .unwrap_or_default(),
        owner: form
            .saved
            .owner
            .map(|owner| owner.to_string())
            .unwrap_or_default(),
    };
    let shown = shown_from_saved(&form.saved);
    let pending = flash::pending(&method, &headers);
    let page = AccountEditPage::new(
        layout(&signed_in, edit_address(id), pending.message(true)),
        form,
        typed,
        AccountProblems::default(),
        shown,
    );
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// `POST /accounts/<number>/edit` (decision 134, the pause's P2 to P4): one write transaction
/// runs `nage_core::accounts::change_account` with what the form showed and the boxes its form
/// has. A clash is 409 with the clash page, nothing written, its boxes those of
/// `nage_core::chart::clash_form` (an archived account's Inside and Owner kept when the save
/// changed them, so Save mine then meets the refusal at the top); refusals are 422 with the
/// form as typed, every problem under its box, and the shown values the request sent, parsed,
/// never refreshed from what is saved; a change that changes nothing writes nothing. A box the
/// request does not carry (a name-only form shown while the account was archived, sent after
/// another window unarchived it) is unchanged, and a returned form (422 or 409) shows what is
/// saved now in it, with what is saved now as its shown value, never an empty box a browser
/// would send back as Directly under the section or Joint (the pause's P2). The place each
/// returned form's Inside holds stays in its list, selected, whatever the reason the list
/// leaves it out (the pause's P5). Success, or nothing to save, is a 303 to the account's page,
/// with the one-time message `Flash::AccountChanged` and its number when something was saved.
pub(crate) async fn post_edit(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    form: Result<Form<EditAccountForm>, FormRejection>,
) -> Result<Response, AppError> {
    // The address is judged before the body is answered (decision 148, leftover N1): the
    // extractor has read and parsed the body already, but a body that is no form is answered
    // only after the address, a section's number included.
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let now = ctx.state.clock.now()?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut tx = ctx.state.storage.begin_write().await?;
    let found = accounts::account(&mut tx, id).await?;
    let Some(account) = found.filter(|account| !account.is_root()) else {
        drop(tx);
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => {
            drop(tx);
            return Ok(forms::unreadable(rejection));
        }
    };
    let reads_inside = !account.is_system();
    let reads_owner = account.kind.allows_owner() && !account.is_system();
    let EditAccountForm {
        name,
        inside,
        owner,
        shown_name,
        shown_inside,
        shown_owner,
    } = form;
    let inside = inside.filter(|_| reads_inside);
    let owner = owner.filter(|_| reads_owner);
    let typed = AccountTyped {
        name: name.clone().unwrap_or_default(),
        inside: inside.clone().unwrap_or_default(),
        owner: owner.clone().unwrap_or_default(),
    };
    let wanted = SentBoxes {
        name: name.map_or(Sent::Absent, Sent::Value),
        inside: inside.as_deref().map_or(Sent::Absent, place_sent),
        owner: owner.as_deref().map_or(Sent::Absent, owner_sent),
    };
    let shown = SentBoxes {
        name: shown_name.map_or(Sent::Absent, Sent::Value),
        inside: shown_inside.as_deref().map_or(Sent::Absent, place_sent),
        owner: shown_owner.as_deref().map_or(Sent::Absent, owner_sent),
    };
    let changed =
        accounts::change_account(&mut tx, signed_in.user.id, now, id, &shown, &wanted).await;
    match changed {
        Ok(Changed::Nothing) => {
            drop(tx);
            Ok(see_other(&account_address(id)))
        }
        Ok(Changed::Saved(_)) => {
            tx.commit().await.map_err(storage::Error::Sqlx)?;
            Ok(with_message(
                &account_address(id),
                Flash::AccountChanged(id),
                secure,
            ))
        }
        Err(ChangeError::Clash(clash)) => {
            let held = clash_held(&clash, &wanted);
            let form = chart::clash_form(&mut tx, id, kept(&held), &clash).await?;
            drop(tx);
            let Some(form) = form else {
                return Ok(unknown(&ctx, &signed_in, method, headers).await);
            };
            let message = alert(clash.to_string());
            let page = AccountClashPage::new(
                layout(&signed_in, edit_address(id), message),
                form,
                *clash,
                typed,
            );
            Ok(render_with(StatusCode::CONFLICT, &page))
        }
        Err(ChangeError::Refused(refusals)) => {
            let saved = AccountBoxes {
                name: account.name.clone(),
                parent: account.parent_id,
                owner: account.owner_user_id,
            };
            let held = unsent_as_saved(&wanted, &wanted, &saved);
            let form = chart::edit_form(&mut tx, id, kept(&held)).await?;
            drop(tx);
            let Some(form) = form else {
                return Ok(unknown(&ctx, &signed_in, method, headers).await);
            };
            let typed = typed_or_saved(typed, &wanted, &saved);
            let shown = unsent_as_saved(&shown, &wanted, &saved);
            let message = refusals.top.as_ref().map_or_else(
                || {
                    Refusal::AccountNotSaved {
                        word: Word::of(account.kind),
                    }
                    .to_string()
                },
                ToString::to_string,
            );
            let problems = AccountProblems {
                name: refusals.name.as_ref().map(ToString::to_string),
                inside: refusals.inside.as_ref().map(ToString::to_string),
                owner: refusals.owner.as_ref().map(ToString::to_string),
            };
            let page = AccountEditPage::new(
                layout(&signed_in, edit_address(id), alert(message)),
                form,
                typed,
                problems,
                shown,
            );
            Ok(render_with(StatusCode::UNPROCESSABLE_ENTITY, &page))
        }
        Err(ChangeError::Failed(AccountError::NoSuchAccount)) => {
            drop(tx);
            Ok(unknown(&ctx, &signed_in, method, headers).await)
        }
        Err(ChangeError::Failed(error)) => Err(error.into()),
    }
}

/// `GET /accounts/<number>/archive`: the confirmation (decision 135); an archived account's
/// address answers 303 to its page; a section and a built-in account get the page for an
/// unknown address (the pause's P6).
pub(crate) async fn get_archive(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    confirm(&ctx, &signed_in, &uri, method, headers, true).await
}

/// `GET /accounts/<number>/unarchive`: the confirmation (decision 135); the address of an
/// account that is not archived answers 303 to its page; a section and a built-in account get
/// the page for an unknown address (the pause's P6).
pub(crate) async fn get_unarchive(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    confirm(&ctx, &signed_in, &uri, method, headers, false).await
}

/// `POST /accounts/<number>/archive` (decision 135): one write transaction archives the
/// account; a refusal is the account's page with the refusal at the top, 422, nothing
/// written; an account archived already, as by a second Archive from another window, is
/// nothing to do: a 303 to its page with no message cookie (the pause's P6). Done is a 303 to
/// the account's page with the one-time message `Flash::AccountArchived` and its number. A
/// section and a built-in account get the page for an unknown address.
pub(crate) async fn post_archive(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    act(&ctx, &signed_in, &uri, method, headers, true).await
}

/// `POST /accounts/<number>/unarchive` (decision 135), as [`post_archive`] with
/// `Flash::AccountUnarchived`; an account not archived is nothing to do: a 303 to its page with
/// no message cookie.
pub(crate) async fn post_unarchive(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    act(&ctx, &signed_in, &uri, method, headers, false).await
}

/// The confirmation of Archive (`archive`) or Unarchive of the account the path of `uri` names,
/// as [`get_archive`] and [`get_unarchive`] describe it.
async fn confirm(
    ctx: &Ctx,
    signed_in: &SignedIn,
    uri: &Uri,
    method: Method,
    headers: HeaderMap,
    archive: bool,
) -> Result<Response, AppError> {
    let Some(id) = path_number(uri) else {
        return Ok(unknown(ctx, signed_in, method, headers).await);
    };
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let view = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::account_view(&mut read, today, id).await?
    };
    let Some(view) = view.filter(|view| view.built_in.is_none()) else {
        return Ok(unknown(ctx, signed_in, method, headers).await);
    };
    if view.archived == archive {
        return Ok(see_other(&account_address(id)));
    }
    let pending = flash::pending(&method, &headers);
    let page = AccountConfirmPage::new(
        layout(
            signed_in,
            action_address(id, archive),
            pending.message(true),
        ),
        view,
        archive,
    );
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// Archives (`archive`) or unarchives the account the path of `uri` names, as [`post_archive`]
/// and [`post_unarchive`] describe it.
async fn act(
    ctx: &Ctx,
    signed_in: &SignedIn,
    uri: &Uri,
    method: Method,
    headers: HeaderMap,
    archive: bool,
) -> Result<Response, AppError> {
    let Some(id) = path_number(uri) else {
        return Ok(unknown(ctx, signed_in, method, headers).await);
    };
    let now = ctx.state.clock.now()?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut tx = ctx.state.storage.begin_write().await?;
    let found = accounts::account(&mut tx, id).await?;
    if found.is_none_or(|account| account.is_system()) {
        drop(tx);
        return Ok(unknown(ctx, signed_in, method, headers).await);
    }
    let by = signed_in.user.id;
    let result = if archive {
        accounts::archive_account(&mut tx, by, now, id).await
    } else {
        accounts::unarchive_account(&mut tx, by, now, id).await
    };
    match result {
        Ok(_) => {
            tx.commit().await.map_err(storage::Error::Sqlx)?;
            let flash = if archive {
                Flash::AccountArchived(id)
            } else {
                Flash::AccountUnarchived(id)
            };
            Ok(with_message(&account_address(id), flash, secure))
        }
        Err(AccountError::AlreadyArchived { .. } | AccountError::NotArchived { .. }) => {
            drop(tx);
            Ok(see_other(&account_address(id)))
        }
        Err(AccountError::NoSuchAccount) => {
            drop(tx);
            Ok(unknown(ctx, signed_in, method, headers).await)
        }
        Err(error @ (AccountError::Database(_) | AccountError::MissingSystemAccount(_))) => {
            Err(error.into())
        }
        Err(refusal) => {
            let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
            let view = chart::account_view(&mut tx, today, id).await?;
            drop(tx);
            let Some(view) = view else {
                return Ok(unknown(ctx, signed_in, method, headers).await);
            };
            let page = AccountPage::new(
                layout(signed_in, account_address(id), alert(refusal.to_string())),
                view,
            );
            Ok(render_with(StatusCode::UNPROCESSABLE_ENTITY, &page))
        }
    }
}

/// The address of the page "What do you want to add?".
const NEW_PATH: &str = "/accounts/new";

/// The section `under` names in a query: `assets`, `liabilities`, `income` or `expenses`,
/// exactly; `None` for any other text, Equity's included, which has no create form on the
/// pages (decision 131).
fn section(under: &str) -> Option<Kind> {
    [Kind::Asset, Kind::Liability, Kind::Income, Kind::Expense]
        .into_iter()
        .find(|kind| kind.root_key().as_str() == under)
}

/// The first value of the field `name` in the query of `uri`, as sent.
fn query_value<'a>(uri: &'a Uri, name: &str) -> Option<&'a str> {
    uri.query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == name).then_some(value)
    })
}

/// The create form's address for the section of `kind`.
fn new_address(kind: Kind) -> String {
    format!("{NEW_PATH}?under={}", kind.root_key())
}

/// The page of the account `id`.
pub(crate) fn account_address(id: AccountId) -> String {
    format!("/accounts/{id}")
}

/// The change form of the account `id`.
fn edit_address(id: AccountId) -> String {
    format!("/accounts/{id}/edit")
}

/// The confirmation of Archive (`archive`) or Unarchive of the account `id`.
fn action_address(id: AccountId, archive: bool) -> String {
    let action = if archive { "archive" } else { "unarchive" };
    format!("/accounts/{id}/{action}")
}

/// The layout of an account page for `signed_in`, its appearance form returning to `back`,
/// with `message` at the top; the page's constructor marks Accounts in the menu.
pub(crate) fn layout(signed_in: &SignedIn, back: String, message: Option<Message>) -> Layout {
    Layout {
        menu: Some(Menu {
            display_name: signed_in.user.display_name.clone(),
            current: Some(Entry::Accounts),
            back,
        }),
        message,
        ..Layout::embedded()
    }
}

/// `text` as a message that says something was not done.
pub(crate) fn alert(text: String) -> Option<Message> {
    Some(Message { text, alert: true })
}

/// The page for an unknown address (decision 118), as the router's fallback answers it: 404,
/// the number never printed, a waiting message shown or cleared as that page does.
pub(crate) async fn unknown(
    ctx: &Ctx,
    signed_in: &SignedIn,
    method: Method,
    headers: HeaderMap,
) -> Response {
    not_found::not_found(
        State(ctx.clone()),
        Some(Extension(signed_in.clone())),
        method,
        headers,
    )
    .await
}

/// A 303 to `location` that sets no cookie.
pub(crate) fn see_other(location: &str) -> Response {
    Redirect::to(location).into_response()
}

/// A 303 to `location` that sets the one-time message `flash` and no other cookie.
pub(crate) fn with_message(location: &str, flash: Flash, secure: bool) -> Response {
    let mut response = Redirect::to(location).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, flash::set(flash, secure));
    response
}

/// A place box as sent: an account's number, or malformed.
fn place_sent(text: &str) -> Sent<AccountId> {
    well_formed_number(text).map_or(Sent::Malformed, Sent::Value)
}

/// An owner box as sent: empty for Joint, a user's number, or malformed.
fn owner_sent(text: &str) -> Sent<Option<UserId>> {
    if text.is_empty() {
        return Sent::Value(None);
    }
    well_formed(text).map_or(Sent::Malformed, |id| Sent::Value(Some(UserId(id))))
}

/// What a returned form keeps in its lists (the pause's P5): the place and the owner `sent`
/// names.
fn kept(sent: &SentBoxes) -> Kept {
    Kept {
        place: match sent.inside {
            Sent::Value(id) => Some(id),
            Sent::Absent | Sent::Malformed => None,
        },
        owner: match sent.owner {
            Sent::Value(owner) => owner,
            Sent::Absent | Sent::Malformed => None,
        },
    }
}

/// The boxes of the clash page (the pause's P2 and P5): what `wanted` sent in each box the
/// person changed (`clash.changed`), what is saved now in every other one, so that the place the
/// Inside box holds stays in its list ([`kept`]) whatever the reason the list leaves it out.
fn clash_held(clash: &Clash, wanted: &SentBoxes) -> SentBoxes {
    let changed = |name: BoxName| clash.changed.contains(&name);
    let saved = &clash.saved;
    SentBoxes {
        name: if changed(BoxName::Name) {
            wanted.name.clone()
        } else {
            Sent::Value(saved.name.clone())
        },
        inside: if changed(BoxName::Inside) {
            wanted.inside.clone()
        } else {
            saved.parent.map_or(Sent::Absent, Sent::Value)
        },
        owner: if changed(BoxName::Owner) {
            wanted.owner.clone()
        } else {
            Sent::Value(saved.owner)
        },
    }
}

/// `sent` with what is `saved` now in each box `wanted` does not carry (the pause's P2): a
/// returned change form never invents a value for a box its request did not carry, neither in
/// the box nor in its hidden shown value.
fn unsent_as_saved(sent: &SentBoxes, wanted: &SentBoxes, saved: &AccountBoxes) -> SentBoxes {
    SentBoxes {
        name: match wanted.name {
            Sent::Absent => Sent::Value(saved.name.clone()),
            Sent::Value(_) | Sent::Malformed => sent.name.clone(),
        },
        inside: match wanted.inside {
            Sent::Absent => saved.parent.map_or(Sent::Absent, Sent::Value),
            Sent::Value(_) | Sent::Malformed => sent.inside.clone(),
        },
        owner: match wanted.owner {
            Sent::Absent => Sent::Value(saved.owner),
            Sent::Value(_) | Sent::Malformed => sent.owner.clone(),
        },
    }
}

/// `typed` with what is `saved` now in each box `wanted` does not carry, as the lists write it
/// (an account's number, an empty owner for Joint).
fn typed_or_saved(typed: AccountTyped, wanted: &SentBoxes, saved: &AccountBoxes) -> AccountTyped {
    AccountTyped {
        name: match wanted.name {
            Sent::Absent => saved.name.clone(),
            Sent::Value(_) | Sent::Malformed => typed.name,
        },
        inside: match wanted.inside {
            Sent::Absent => saved
                .parent
                .map(|parent| parent.to_string())
                .unwrap_or_default(),
            Sent::Value(_) | Sent::Malformed => typed.inside,
        },
        owner: match wanted.owner {
            Sent::Absent => saved
                .owner
                .map(|owner| owner.to_string())
                .unwrap_or_default(),
            Sent::Value(_) | Sent::Malformed => typed.owner,
        },
    }
}

/// The whole path of the place whose change form is `form` under the section of `kind`, from
/// the top-level account down to the place itself, for the trail of a create form opened
/// inside it (the pause's P10); empty when it is no account of that section.
fn place_path(form: Option<EditForm>, kind: Kind) -> Vec<TrailStep> {
    match form {
        Some(form) if form.kind == kind => {
            let mut path = form.trail;
            path.push(TrailStep {
                id: form.id,
                name: form.name,
            });
            path
        }
        _ => Vec::new(),
    }
}
