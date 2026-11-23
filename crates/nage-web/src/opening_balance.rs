//! The opening balance's pages (SPEC 6.6, W2, decisions 140 to 146): the form that enters an
//! account's opening balance and changes it, the Remove confirmation and the three clash pages,
//! behind the sign-in, under the account's address.
//!
//! The addresses (decision 140): `GET /accounts/<number>/opening-balance` the form, which enters
//! one for an account without one and changes it for an account with one (its boxes hold what
//! is saved, its hidden fields `entry` and `version` name the entry and the version shown);
//! `POST` to the same address saves, judged against the account's live opening balance and never
//! against the entry a form names: without `entry` it enters one while the account has none;
//! with `entry` and `version` naming the live one at its version it changes that one; anything
//! else meets a clash. `GET /accounts/<number>/opening-balance/remove` the confirmation, a
//! `POST` to the same address the removal, naming the entry and the version it showed. The
//! number is the account's, judged as slice 7 judges it (`accounts::path_number`), and answered
//! BEFORE the body (leftover N1's order: axum's extractor has read and parsed the body by then,
//! but a body that is no form is answered only after the address): an unknown or malformed
//! number, a section, a built-in account and an account outside Assets and Liabilities get the
//! page for an unknown address, which never prints the number; an archived account's form and
//! confirmation answer
//! 303 to its page; a POST that meets an account archived meanwhile answers 422 with the
//! account's page and the core's sentence at its top, nothing written.
//!
//! Every form is a plain post with no htmx attribute (decision 116), inside the existing guard
//! (the sign-in rule, the Origin check, the body limit) with Cache-Control: no-store. One write
//! transaction per POST, all or none (INV-28): after a save or a removal a 303 to the account's
//! page with the one-time message `Flash::OpeningBalanceSaved` or `Flash::OpeningBalanceRemoved`
//! and the account's number (decision 146); a save that changes nothing, and a removal with
//! nothing to remove, a 303 with no message; a refused form 422 with what was typed kept, the tick
//! as sent and every problem under its box, slice 7's line at the top; a zero typed on an account
//! that has an opening balance, the Remove confirmation (200) with the line You typed 0, nothing
//! written (decision 143); a clash 409 with its page (decision 144); a busy, refused, clashing or
//! failed request leaves no row and no message cookie. Today is the server's day in the configured
//! zone (`clock::today`, A8); the date box's hints never decide: the forms carry novalidate, so a
//! date outside them is sent and answered in Nage's words (the pause's P4). The web hands the
//! core the boxes as typed and the tick as sent ([`core_form`], the pause's P3) and every rule
//! stays the core's (`nage_core::transactions::save_opening_balance_form`,
//! `remove_opening_balance_form`), which judges a save and a removal against the live opening
//! balance of the account in the address, never against the entry a form names (the pause's
//! P2). No number a request chose is ever printed. The form, the confirmation and the clash
//! pages read the account through the core's read for these pages
//! (`nage_core::chart::opening_balance_account`, the pause's P5) and never the roll-up of
//! balances (A7); the 422 of an account archived meanwhile shows the account's page, roll-up
//! included, as every other answer of that page does.

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use nage_core::accounts::AccountId;
use nage_core::chart::{self, OpeningBalanceAccount};
use nage_core::date::Date;
use nage_core::money::Money;
use nage_core::sessions::SignedIn;
use nage_core::storage::{self, WriteTransaction};
use nage_core::transactions::{
    self, OpeningBalance, OpeningBalanceError, OpeningForm, OpeningFormError, OpeningNamed,
    OpeningRemoved, OpeningSaved, TransactionId,
};
use serde::Deserialize;

use crate::accounts::{
    account_address, alert, layout, path_number, see_other, unknown, well_formed, with_message,
};
use crate::flash::{self, Flash};
use crate::messages::Refusal;
use crate::pages::{
    AccountPage, OpeningAccount, OpeningBalanceClashPage, OpeningBalancePage,
    OpeningBalanceRemovePage, OpeningProblems, OpeningTyped, render, render_with,
};
use crate::{AppError, Ctx, clock, cookie, forms};

#[cfg(test)]
mod tests;

/// The opening-balance form as posted (decision 141). A box the request does not carry is
/// empty; the tick box counts as ticked when the request carries it, whatever its value; `entry`
/// and `version` are absent on the form that enters one.
#[derive(Deserialize)]
pub(crate) struct OpeningBalanceForm {
    #[serde(default)]
    pub(crate) date: String,
    #[serde(default)]
    pub(crate) amount: String,
    /// The tick box: The account was overdrawn, or The account was in credit.
    pub(crate) below_zero: Option<String>,
    /// The entry the change form names, as Nage printed its number.
    pub(crate) entry: Option<String>,
    /// The version the change form showed.
    pub(crate) version: Option<String>,
}

/// The removal as posted from the confirmation (decision 143): the entry and the version it
/// showed.
#[derive(Deserialize)]
pub(crate) struct RemoveForm {
    pub(crate) entry: Option<String>,
    pub(crate) version: Option<String>,
}

/// `GET /accounts/<number>/opening-balance`: the form (decisions 140 and 141), empty for an
/// account without an opening balance (no default date), holding what is saved for one with an
/// opening balance, an amount below zero without its minus and with the tick box ticked.
pub(crate) async fn get_form(
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
    let found = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::opening_balance_account(&mut read, id).await?
    };
    let Some(account) = taking(found) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    if account.archived {
        return Ok(see_other(&account_address(id)));
    }
    let (entry, typed) = match account.opening_balance {
        Some(saved) => (
            Some((saved.id, saved.version)),
            saved_typed(&saved, &account),
        ),
        None => (None, OpeningTyped::default()),
    };
    let pending = flash::pending(&method, &headers);
    let page = OpeningBalancePage::new(
        layout(&signed_in, form_address(id), pending.message(true)),
        page_account(&account),
        entry,
        typed,
        OpeningProblems::default(),
        today,
    );
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// `POST /accounts/<number>/opening-balance`: the address judged first, then the body; one
/// write transaction runs `save_opening_balance_form` and answers as the module says.
pub(crate) async fn post_form(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    form: Result<Form<OpeningBalanceForm>, FormRejection>,
) -> Result<Response, AppError> {
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let now = ctx.state.clock.now()?;
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut tx = ctx.state.storage.begin_write().await?;
    let found = chart::opening_balance_account(&mut tx, id).await?;
    let Some(account) = taking(found) else {
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
    let typed = OpeningTyped {
        date: form.date.clone(),
        amount: form.amount.clone(),
        below_zero: form.below_zero.is_some(),
    };
    let sent = core_form(form);
    let saved =
        transactions::save_opening_balance_form(&mut tx, signed_in.user.id, now, today, id, &sent)
            .await;
    match saved {
        Ok(OpeningSaved::Saved(_)) => {
            tx.commit().await.map_err(storage::Error::Sqlx)?;
            Ok(with_message(
                &account_address(id),
                Flash::OpeningBalanceSaved(id),
                secure,
            ))
        }
        Ok(OpeningSaved::Nothing) => {
            drop(tx);
            Ok(see_other(&account_address(id)))
        }
        Ok(OpeningSaved::AskRemove(saved)) => {
            drop(tx);
            let page = OpeningBalanceRemovePage::new(
                layout(&signed_in, remove_address(id), None),
                page_account(&account),
                saved,
                true,
            );
            Ok(render(&page))
        }
        Err(OpeningFormError::Clash(clash)) => {
            drop(tx);
            let page = OpeningBalanceClashPage::new(
                layout(&signed_in, form_address(id), alert(clash.to_string())),
                page_account(&account),
                *clash,
                typed,
                today,
            );
            Ok(render_with(StatusCode::CONFLICT, &page))
        }
        Err(OpeningFormError::Refused(refusals)) => {
            if let Some(top) = refusals.top {
                return archived_meanwhile(&ctx, &signed_in, tx, id, &top, method, headers).await;
            }
            drop(tx);
            let problems = OpeningProblems {
                date: refusals.date.as_ref().map(ToString::to_string),
                amount: refusals.amount.as_ref().map(ToString::to_string),
            };
            let page = OpeningBalancePage::new(
                layout(
                    &signed_in,
                    form_address(id),
                    alert(Refusal::OpeningBalanceNotSaved.to_string()),
                ),
                page_account(&account),
                account
                    .opening_balance
                    .map(|saved| (saved.id, saved.version)),
                typed,
                problems,
                today,
            );
            Ok(render_with(StatusCode::UNPROCESSABLE_ENTITY, &page))
        }
        Err(OpeningFormError::Failed(error)) => {
            drop(tx);
            failed(&ctx, &signed_in, error, method, headers).await
        }
    }
}

/// `GET /accounts/<number>/opening-balance/remove`: the confirmation (decision 143); for an
/// account without an opening balance, a 303 to its page.
pub(crate) async fn get_remove(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let found = {
        let mut read = ctx.state.storage.begin_read().await?;
        chart::opening_balance_account(&mut read, id).await?
    };
    let Some(account) = taking(found) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let saved = match account.opening_balance {
        Some(saved) if !account.archived => saved,
        _ => return Ok(see_other(&account_address(id))),
    };
    let pending = flash::pending(&method, &headers);
    let page = OpeningBalanceRemovePage::new(
        layout(&signed_in, remove_address(id), pending.message(true)),
        page_account(&account),
        saved,
        false,
    );
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

/// `POST /accounts/<number>/opening-balance/remove`: the address judged first, then the body;
/// one write transaction runs `remove_opening_balance_form`: removed, a 303 with its message;
/// nothing to remove, a 303 with none; a change met, 409 with the confirmation again and the
/// figure as it is now.
pub(crate) async fn post_remove(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    form: Result<Form<RemoveForm>, FormRejection>,
) -> Result<Response, AppError> {
    let Some(id) = path_number(&uri) else {
        return Ok(unknown(&ctx, &signed_in, method, headers).await);
    };
    let now = ctx.state.clock.now()?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut tx = ctx.state.storage.begin_write().await?;
    let found = chart::opening_balance_account(&mut tx, id).await?;
    let Some(account) = taking(found) else {
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
    let named = OpeningNamed::Entry {
        id: form
            .entry
            .as_deref()
            .and_then(well_formed)
            .map(TransactionId),
        version: form.version.as_deref().and_then(well_formed),
    };
    let removed =
        transactions::remove_opening_balance_form(&mut tx, signed_in.user.id, now, id, named).await;
    match removed {
        Ok(OpeningRemoved::Removed(_)) => {
            tx.commit().await.map_err(storage::Error::Sqlx)?;
            Ok(with_message(
                &account_address(id),
                Flash::OpeningBalanceRemoved(id),
                secure,
            ))
        }
        Ok(OpeningRemoved::Nothing) => {
            drop(tx);
            Ok(see_other(&account_address(id)))
        }
        Err(OpeningFormError::Clash(clash)) => {
            drop(tx);
            let Some(saved) = clash.saved().copied() else {
                return Err(AppError::Internal(String::from(
                    "a removal met a clash without the opening balance saved now",
                )));
            };
            let page = OpeningBalanceRemovePage::new(
                layout(&signed_in, remove_address(id), alert(clash.to_string())),
                page_account(&account),
                saved,
                false,
            );
            Ok(render_with(StatusCode::CONFLICT, &page))
        }
        Err(OpeningFormError::Refused(refusals)) => match refusals.top {
            Some(top) => archived_meanwhile(&ctx, &signed_in, tx, id, &top, method, headers).await,
            None => Err(AppError::Internal(String::from(
                "a removal was refused without a sentence at the top",
            ))),
        },
        Err(OpeningFormError::Failed(error)) => {
            drop(tx);
            failed(&ctx, &signed_in, error, method, headers).await
        }
    }
}

/// The form as the core takes it (decision 141, the pause's P3): the date box and the amount
/// box as typed and the tick box as sent, untouched; `entry` and `version` as what the form
/// names (`OpeningNamed::New` without `entry`, else each `None` when it is not a number as Nage
/// prints one). The sign, the two-signs refusal and which entry is the account's are the core's
/// to judge.
pub(crate) fn core_form(form: OpeningBalanceForm) -> OpeningForm {
    let named = match form.entry {
        None => OpeningNamed::New,
        Some(entry) => OpeningNamed::Entry {
            id: well_formed(&entry).map(TransactionId),
            version: form.version.as_deref().and_then(well_formed),
        },
    };
    OpeningForm {
        date: form.date,
        amount: form.amount,
        below_zero: form.below_zero.is_some(),
        named,
    }
}

/// `found`, the account as the opening-balance pages read it (`chart::opening_balance_account`),
/// when it takes an opening balance by the core's own rule; `None` for an unknown number, a
/// section, a built-in account and every account outside Assets and Liabilities.
fn taking(found: Option<OpeningBalanceAccount>) -> Option<OpeningBalanceAccount> {
    found.filter(|account| account.takes_opening_balance)
}

/// What the pages name of `account`.
fn page_account(account: &OpeningBalanceAccount) -> OpeningAccount {
    OpeningAccount {
        id: account.id,
        name: account.name.clone(),
        place: account.place.clone(),
        kind: account.kind,
        trail: account.trail.clone(),
        minor_digits: account.minor_digits,
    }
}

/// The boxes of the change form for `saved` (decision 141): its date as Nage writes it and its
/// amount as SPEC 6.10 writes it, an amount below zero without its minus and with the box ticked.
fn saved_typed(saved: &OpeningBalance, account: &OpeningBalanceAccount) -> OpeningTyped {
    let written = saved.amount.display(account.minor_digits).to_string();
    let below_zero = saved.amount < Money::ZERO;
    let amount = if below_zero {
        written.strip_prefix('-').unwrap_or(&written).to_owned()
    } else {
        written
    };
    OpeningTyped {
        date: date_text(saved.date),
        amount,
        below_zero,
    }
}

/// `date` as Nage writes dates.
fn date_text(date: Date) -> String {
    date.to_string()
}

/// The form's address of the account `id`.
fn form_address(id: AccountId) -> String {
    format!("/accounts/{id}/opening-balance")
}

/// The confirmation's address of the account `id`.
fn remove_address(id: AccountId) -> String {
    format!("/accounts/{id}/opening-balance/remove")
}

/// The answer to a save or a removal that met the account `id` archived meanwhile (decision
/// 140): 422 with the account's page, `top` at its top, nothing written.
async fn archived_meanwhile(
    ctx: &Ctx,
    signed_in: &SignedIn,
    mut tx: WriteTransaction,
    id: AccountId,
    top: &OpeningBalanceError,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let view = chart::account_view(&mut tx, today, id).await?;
    drop(tx);
    let Some(view) = view else {
        return Ok(unknown(ctx, signed_in, method, headers).await);
    };
    let page = AccountPage::new(
        layout(signed_in, account_address(id), alert(top.to_string())),
        view,
    );
    Ok(render_with(StatusCode::UNPROCESSABLE_ENTITY, &page))
}

/// The answer to an operation that failed rather than refused: an account that does not exist
/// or takes no opening balance gets the page for an unknown address; anything else is a failure.
async fn failed(
    ctx: &Ctx,
    signed_in: &SignedIn,
    error: OpeningBalanceError,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    match error {
        OpeningBalanceError::NoSuchAccount { .. }
        | OpeningBalanceError::NotAssetOrLiability { .. }
        | OpeningBalanceError::RootAccount { .. } => {
            Ok(unknown(ctx, signed_in, method, headers).await)
        }
        other => Err(other.into()),
    }
}
