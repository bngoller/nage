//! The hashing the application is handed (SPEC 5.3, decision 107): the dummy hash made once
//! before any request, the production pair with the parameters handed in, and the limit on
//! how many hashes and verifications run at once.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use nage_core::password::{self, HashError, HashParams, Verification};
use nage_core::test_support::{self, CHEAP_HASH_PARAMS};
use nage_web::{AppState, Hashing, HealthProbe, MAX_CONCURRENT_HASHES};

use common::VERSION;
use common::app::{ADMIN, App, Gate, Options, PASSWORD, Work};

/// `prepare` makes the dummy hash with the hash function it was handed, once: a second call
/// makes no further hash. A sign-in for an unknown username is then verified against it
/// without another hash being made.
///
/// Proves: decision 107 (the dummy hash is made once, with the set handed in, before any
/// request).
#[tokio::test]
async fn prepare_makes_the_dummy_hash_once() {
    let work = Work::default();
    let hashing = work.hashing();
    assert_eq!(work.hashes(), 0, "hashes made by building the hashing");
    for call in 1..=2 {
        hashing
            .prepare()
            .await
            .unwrap_or_else(|error| panic!("prepare call {call} failed: {error}"));
        assert_eq!(work.hashes(), 1, "hashes made after prepare call {call}");
    }

    let app = App::with_book().await;
    let hashes_before = app.work.hashes();
    let reply = app
        .client()
        .post("/login", &[("username", "nobody"), ("password", PASSWORD)])
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of an unknown username's sign-in"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before,
        "hashes made by an unknown username's sign-in after prepare"
    );
    assert_eq!(
        app.work.verifications(),
        1,
        "verifications of an unknown username's sign-in"
    );
}

/// A hashing that was never prepared makes the dummy hash at the first verification that
/// needs it, and only then: the first sign-in for an unknown username makes one hash, the
/// second none.
///
/// Proves: decision 107 (the dummy hash is made once), for a caller that did not prepare.
#[tokio::test]
async fn a_hashing_never_prepared_makes_the_dummy_hash_at_its_first_need() {
    let (_dir, storage) = test_support::open_migrated().await;
    let work = Work::default();
    let router = nage_web::router(AppState {
        version: VERSION,
        health: HealthProbe::always(true),
        storage: storage.clone(),
        clock: common::app::TestClock::start().clock(),
        hashing: work.hashing(),
        trust_proxy: false,
    });
    test_support::set_up(
        &storage,
        common::app::stamp(common::app::START),
        ADMIN,
        PASSWORD,
    )
    .await;
    let hashes_before = work.hashes();
    let mut client = common::app::Client::over(router);
    for (attempt, hashes) in [(1, 1), (2, 1)] {
        let reply = client
            .post("/login", &[("username", "nobody"), ("password", PASSWORD)])
            .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of attempt {attempt}"
        );
        assert_eq!(
            work.hashes() - hashes_before,
            hashes,
            "dummy hashes made after attempt {attempt}"
        );
    }
}

/// A hasher that cannot hash fails `prepare` with its error instead of panicking or leaving
/// the failure to the first sign-in: argon2 refuses a memory of zero.
///
/// Proves: decision 107 (a parameter set argon2 refuses is an error, never a panic), at the
/// place `serve` meets it.
#[tokio::test]
async fn a_hasher_that_cannot_hash_fails_prepare() {
    let hashing = Hashing::argon2id(HashParams {
        memory_kib: 0,
        passes: 1,
        lanes: 1,
    });
    match hashing.prepare().await {
        Err(HashError::Hasher(_)) => {}
        other => panic!("prepare with a memory of zero answered {other:?}"),
    }
}

/// The production hashing, `Hashing::argon2id`, makes new hashes with the parameters it was
/// handed: a book set up through the page under the cheap set stores an argon2id hash that
/// names m=8, t=1, p=1 and verifies the password, and the admin signs in with it.
///
/// Proves: decision 107 (the set handed in makes new hashes, and they verify).
#[tokio::test]
async fn the_production_hashing_hashes_with_the_parameters_handed_in() {
    let app = App::start_with(Options {
        hashing: Some(Hashing::argon2id(CHEAP_HASH_PARAMS)),
        ..Options::default()
    })
    .await;
    let reply = app
        .client()
        .post(
            "/setup",
            &[
                ("book_name", "Household"),
                ("currency", "CAD"),
                ("username", ADMIN),
                ("display_name", "Sam"),
                ("password", PASSWORD),
                ("password_again", PASSWORD),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "POST /setup status:\n{}",
        reply.body
    );
    let stored = app.credentials(ADMIN).await.password_hash;
    assert!(
        stored.starts_with("$argon2id$v=19$m=8,t=1,p=1$"),
        "the stored hash does not name the set handed in: {stored}"
    );
    assert_eq!(
        password::verify(PASSWORD, &stored),
        Verification::Verified,
        "the stored hash against the password"
    );
    app.signed_in(ADMIN, PASSWORD).await;
}

/// Only `MAX_CONCURRENT_HASHES` verifications run at once: of six sign-ins sent together,
/// each held inside its verification by the hashing double, four are inside and two wait for
/// their turn; once the four are let go all six are verified and answered. The limit covers
/// the hash of a new password too: of six set-ups sent together to a server without a user,
/// each held inside the hash of its password, four are inside and two wait; once the four
/// are let go all six are hashed and one set-up is accepted.
///
/// Proves: decision 107 (hashing and verification run at most four at once; a further one
/// waits for its turn).
#[tokio::test]
async fn only_four_verifications_run_at_once() {
    assert_eq!(MAX_CONCURRENT_HASHES, 4, "the limit on hashes at once");
    let gate = Gate::new();
    let held = Arc::clone(&gate);
    let hashing = Hashing::new(
        |password| password::hash(CHEAP_HASH_PARAMS, password),
        move |password, stored_hash| {
            held.pass();
            password::verify(password, stored_hash)
        },
    );
    let app = App::start_with(Options {
        hashing: Some(hashing),
        ..Options::default()
    })
    .await;
    app.set_up().await;
    let attempt = |username: &'static str| {
        let mut client = app.client();
        async move {
            client
                .post("/login", &[("username", username), ("password", PASSWORD)])
                .await
                .status
        }
    };
    let (a, b, c, d, e, f, inside) = tokio::join!(
        attempt("one"),
        attempt("two"),
        attempt("three"),
        attempt("four"),
        attempt("five"),
        attempt("six"),
        async {
            gate.wait_entered(MAX_CONCURRENT_HASHES).await;
            // Long enough for a fifth to arrive if nothing held it back.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let inside = gate.entered();
            gate.open();
            inside
        }
    );
    assert_eq!(
        inside, MAX_CONCURRENT_HASHES,
        "verifications inside the hashing while the first four were held"
    );
    for status in [a, b, c, d, e, f] {
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of a sign-in for an unknown username"
        );
    }
    assert_eq!(gate.entered(), 6, "verifications once the gate was open");

    // The dummy hash, made before any request, passes the hash gate unheld: it is armed after.
    let hash_gate = Gate::unarmed();
    let held = Arc::clone(&hash_gate);
    let hashing = Hashing::new(
        move |password| {
            held.pass();
            password::hash(CHEAP_HASH_PARAMS, password)
        },
        password::verify,
    );
    let empty = App::start_with(Options {
        hashing: Some(hashing),
        ..Options::default()
    })
    .await;
    hash_gate.arm();
    let set_up = |username: &'static str| {
        let mut client = empty.client();
        async move {
            client
                .post(
                    "/setup",
                    &[
                        ("book_name", "Household"),
                        ("currency", "CAD"),
                        ("username", username),
                        ("display_name", "Sam"),
                        ("password", PASSWORD),
                        ("password_again", PASSWORD),
                    ],
                )
                .await
                .status
        }
    };
    let (a, b, c, d, e, f, inside) = tokio::join!(
        set_up("one"),
        set_up("two"),
        set_up("three"),
        set_up("four"),
        set_up("five"),
        set_up("six"),
        async {
            hash_gate.wait_entered(MAX_CONCURRENT_HASHES).await;
            // Long enough for a fifth to arrive if nothing held it back.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let inside = hash_gate.entered();
            hash_gate.open();
            inside
        }
    );
    assert_eq!(
        inside, MAX_CONCURRENT_HASHES,
        "hashes inside the hashing while the first four were held"
    );
    let accepted = [a, b, c, d, e, f]
        .iter()
        .filter(|status| **status == StatusCode::SEE_OTHER)
        .count();
    assert_eq!(accepted, 1, "set-ups accepted of six sent together");
    assert_eq!(hash_gate.entered(), 6, "hashes once the gate was open");
}
