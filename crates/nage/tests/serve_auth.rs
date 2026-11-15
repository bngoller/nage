//! What `nage serve` hands nage-web, observed from outside the process (SPEC 5.2 and 5.3,
//! decisions 107, 108 and 113): a fresh install is sent to the set-up page, the passwords are
//! hashed with SPEC 5.3's parameters, the admin is stamped by the system clock, and
//! trust-proxy comes from `--trust-proxy` or `NAGE_TRUST_PROXY`.

mod common;

use nage_core::storage::{Storage, StorageOptions};
use nage_core::timestamp::Timestamp;
use nage_core::users;

use common::web::{ADMIN, header, session_cookie};
use common::{ServeProcess, assert_config_error, config_error_line, serve};

/// Runs `future` on a fresh current-thread runtime.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"))
        .block_on(future)
}

/// The system clock cut to the second, as the tests read it.
fn now() -> Timestamp {
    Timestamp::from_jiff(jiff::Timestamp::now())
        .unwrap_or_else(|error| panic!("the system clock is no instant of Nage: {error}"))
}

/// A fresh `nage serve` shows the set-up page at `/setup` and sends the start page there with
/// a 303; a set-up through the page answers 303 to `/` with the session cookie; the start
/// page then answers 200 with that cookie and 303 to the sign-in page without it, and
/// `/setup` is closed with 404.
///
/// Proves: SPEC W1 steps 1 to 3 through the real server, decision 113 (`serve` hands nage-web
/// the storage it opened, with a clock and a hashing that work), INV-27.
#[test]
fn serve_sends_a_fresh_install_to_set_up_and_everything_else_behind_the_sign_in() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let form = server.get(addr, "/setup", &[]);
    assert_eq!(
        form.status, 200,
        "GET /setup on a fresh server:\n{}",
        form.head
    );
    let start = server.get(addr, "/", &[]);
    assert_eq!(
        start.status, 303,
        "GET / on a fresh server:\n{}",
        start.head
    );
    assert_eq!(
        header(&start, "location"),
        Some("/setup"),
        "where a fresh server sends the start page:\n{}",
        start.head
    );

    let session = server.signed_in(addr);
    let home = server.get(addr, "/", &[("Cookie", &session)]);
    assert_eq!(
        home.status, 200,
        "GET / with the admin's session:\n{}",
        home.head
    );
    let anonymous = server.get(addr, "/", &[]);
    assert_eq!(
        anonymous.status, 303,
        "GET / without a session once set up:\n{}",
        anonymous.head
    );
    assert_eq!(
        header(&anonymous, "location"),
        Some("/login"),
        "where a set-up server sends the start page:\n{}",
        anonymous.head
    );
    let closed = server.get(addr, "/setup", &[]);
    assert_eq!(
        closed.status, 404,
        "GET /setup once set up:\n{}",
        closed.head
    );
}

/// The password a set-up through the page stores is hashed with SPEC 5.3's parameters,
/// argon2id at m=19456 KiB, t=2, p=1, never with a cheaper set; and the admin is stamped by
/// the system clock, between the instant before the set-up and the instant after it.
///
/// Proves: SPEC 5.3 (argon2id at m=19456 KiB, t=2, p=1), decision 107 (the binary hands these
/// parameters in, pinned by a test), decisions 88 and 113 (`serve` hands nage-web the clock).
#[test]
fn serve_hashes_with_the_parameters_of_spec_5_3_and_stamps_with_the_system_clock() {
    let mut server = ServeProcess::spawn();
    let database = server.dir().join("data").join("nage.sqlite");
    let addr = server.wait_listening();
    let before = now();
    let response = server.set_up(addr, &[]);
    let after = now();
    let (token, _secure) = session_cookie(&response);
    assert_eq!(token.len(), 64, "length of the session token");
    assert!(
        server.stop().success(),
        "nage serve did not stop cleanly after the set-up"
    );

    block_on(async {
        let storage = Storage::open(&database, &StorageOptions::default())
            .await
            .unwrap_or_else(|error| panic!("cannot open {}: {error}", database.display()));
        let mut read = storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
        let admin = users::user_by_username(&mut read, ADMIN)
            .await
            .unwrap_or_else(|error| panic!("cannot look the admin up: {error}"))
            .unwrap_or_else(|| panic!("the set-up created no admin"));
        assert!(
            admin
                .password_hash
                .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "the stored hash was not made with SPEC 5.3's parameters: {}",
            admin.password_hash
        );
        assert!(
            before <= admin.user.created_at && admin.user.created_at <= after,
            "the admin was created at {}, outside {before} to {after}",
            admin.user.created_at
        );
        let rows = nage_core::test_support::session_rows(&mut read).await;
        assert_eq!(rows, 1, "sessions after the set-up");
        drop(read);
        storage.close().await;
    });
}

/// Trust-proxy is off unless asked for: the cookie of a set-up sent with
/// `X-Forwarded-Proto: https` carries `Secure` with `--trust-proxy`, with
/// `NAGE_TRUST_PROXY=true`, and with the switch whatever the variable says (`false`, or a
/// value that is neither); it does not with neither, with `NAGE_TRUST_PROXY=false` and with
/// the variable set but empty.
///
/// Proves: SPEC 5.2 (the trust-proxy flag), decision 113 (`--trust-proxy` turns it on
/// whatever `NAGE_TRUST_PROXY` says; without it the variable decides, `true` or `false`,
/// unset or empty meaning off), decision 108 (Secure only with trust-proxy on).
#[test]
fn serve_trusts_the_proxy_only_when_told_to() {
    let cases: [(bool, Option<&str>, bool); 7] = [
        (false, None, false),
        (false, Some("false"), false),
        (false, Some(""), false),
        (false, Some("true"), true),
        (true, None, true),
        (true, Some("false"), true),
        (true, Some("perhaps"), true),
    ];
    for (switch, variable, secure) in cases {
        let mut command = serve();
        if switch {
            command = command.args(["--trust-proxy"]);
        }
        if let Some(value) = variable {
            command = command.env("NAGE_TRUST_PROXY", value);
        }
        let what = format!("--trust-proxy {switch}, NAGE_TRUST_PROXY {variable:?}");
        let mut server = command.spawn();
        let addr = server.wait_listening();
        let response = server.set_up(addr, &[("X-Forwarded-Proto", "https")]);
        let (_token, has_secure) = session_cookie(&response);
        assert_eq!(
            has_secure, secure,
            "whether the cookie is Secure with {what}:\n{}",
            response.head
        );
    }
}

/// `NAGE_TRUST_PROXY` takes `true` or `false` and nothing else: `1`, `yes`, `on`, `TRUE`,
/// `True`, `0` and a `true` with a space before it each refuse to start, with exit code 2 and
/// one `nage: ` line that names the variable and quotes the value; nothing is created. The
/// test prints the line for `yes` (`cargo test -p nage --test serve_auth refuses -- --nocapture`
/// shows it).
///
/// Proves: decision 113 (any other value refuses to start with a message naming the variable
/// and the value, exit 2), decisions 42 and 45.
#[test]
fn serve_refuses_a_nage_trust_proxy_that_is_neither_true_nor_false() {
    for value in ["1", "yes", "on", "TRUE", "True", "0", " true"] {
        let command = serve().env("NAGE_TRUST_PROXY", value);
        let data_dir = command.dir().join("data");
        let out = command.run();
        assert_config_error(&out, "nage: NAGE_TRUST_PROXY: ", &format!("{value:?}"));
        let line = config_error_line(&out, "nage: NAGE_TRUST_PROXY: ");
        if value == "yes" {
            println!("[serve] NAGE_TRUST_PROXY=yes nage serve => {line}");
        }
        assert_eq!(
            line,
            format!("nage: NAGE_TRUST_PROXY: must be true or false {value:?}"),
            "the error line for NAGE_TRUST_PROXY={value:?}"
        );
        assert!(
            !data_dir.exists(),
            "a refused start created {}",
            data_dir.display()
        );
    }
}
