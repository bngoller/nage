//! The helpers of the tests of milestone 2 slices 5 and 6 (decisions 115 and 122): the real
//! router over a fresh migrated database, driven in process, with a clock the test sets, a
//! zone the test names, the cheap hashing set behind a double that counts, and a client that
//! carries both cookies by hand, the session's and the one-time message's, and finds each by
//! name. Scenario data is written through the core's operations
//! (`nage_core::test_support`, the account and opening-balance operations).

use std::fmt::Write as _;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use jiff::tz::TimeZone;
use nage_core::accounts::{self, AccountId, NewAccount};
use nage_core::currency::Currency;
use nage_core::date::Date;
use nage_core::money::Money;
use nage_core::password::{self, Verification};
use nage_core::sessions::{self, SignedIn, TokenHash};
use nage_core::setup::{self, Initialized, Setup};
use nage_core::storage::{ReadTransaction, Storage, StorageOptions, WriteTransaction};
use nage_core::test_support::{self, CHEAP_HASH_PARAMS, TestDir};
use nage_core::timestamp::Timestamp;
use nage_core::transactions::{self, NewOpeningBalance};
use nage_core::users::{self, Credentials, NewUser, User, UserId};
use nage_web::{AppState, Clock, Hashing, HealthProbe};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;

use super::VERSION;

/// The admin the tests set the book up with.
pub const ADMIN: &str = "sam";
/// The admin's password.
pub const PASSWORD: &str = "correct horse 42";
/// A second user's name and password.
pub const OTHER: &str = "alex";
pub const OTHER_PASSWORD: &str = "battery staple 7";
/// A password that is nobody's.
pub const WRONG_PASSWORD: &str = "not the password";
/// The names of the two cookies, written out here rather than taken from nage-web, so that a
/// renamed cookie fails the tests.
pub const SESSION_COOKIE: &str = "nage_session";
pub const MESSAGE_COOKIE: &str = "nage_message";
/// The host the client names in `Host`, and the origin its forms come from.
pub const HOST: &str = "nage.test";
pub const ORIGIN: &str = "http://nage.test";
/// The instant every test starts at: 2027-01-15T08:00:00Z.
pub const START: i64 = 1_800_000_000;
/// A day and an hour, in seconds.
pub const DAY: i64 = 86_400;
pub const HOUR: i64 = 3_600;

/// A clock the test sets. Cloning shares it.
#[derive(Clone)]
pub struct TestClock(Arc<Mutex<i64>>);

impl TestClock {
    fn at(second: i64) -> Self {
        Self(Arc::new(Mutex::new(second)))
    }

    /// A clock at [`START`].
    pub fn start() -> Self {
        Self::at(START)
    }

    fn second(&self) -> i64 {
        *self
            .0
            .lock()
            .unwrap_or_else(|error| panic!("the test clock is poisoned: {error}"))
    }

    /// The instant the clock shows.
    pub fn now(&self) -> Timestamp {
        stamp(self.second())
    }

    /// Moves the clock `seconds` on.
    pub fn advance(&self, seconds: i64) {
        let mut second = self
            .0
            .lock()
            .unwrap_or_else(|error| panic!("the test clock is poisoned: {error}"));
        *second += seconds;
    }

    /// Sets the clock to `seconds` after [`START`].
    pub fn set(&self, seconds: i64) {
        let mut second = self
            .0
            .lock()
            .unwrap_or_else(|error| panic!("the test clock is poisoned: {error}"));
        *second = START + seconds;
    }

    /// The clock as the application is handed it.
    pub fn clock(&self) -> Clock {
        let shared = self.clone();
        Clock::new(move || Timestamp::from_second(shared.second()))
    }
}

/// The instant `second` seconds after the epoch.
pub fn stamp(second: i64) -> Timestamp {
    Timestamp::from_second(second)
        .unwrap_or_else(|error| panic!("{second} is no instant of the tests: {error}"))
}

/// What the hashing double saw (decision 115: verifications are counted, never timed): how
/// many hashes were made, and against which stored hash each verification ran, in order.
#[derive(Clone, Default)]
pub struct Work {
    hashes: Arc<Mutex<usize>>,
    verified_against: Arc<Mutex<Vec<String>>>,
}

impl Work {
    /// How many hashes were made, the dummy hash included.
    pub fn hashes(&self) -> usize {
        *self
            .hashes
            .lock()
            .unwrap_or_else(|error| panic!("the hash count is poisoned: {error}"))
    }

    /// The stored hash each verification ran against, in order.
    pub fn verified_against(&self) -> Vec<String> {
        self.verified_against
            .lock()
            .unwrap_or_else(|error| panic!("the verification list is poisoned: {error}"))
            .clone()
    }

    /// How many verifications ran.
    pub fn verifications(&self) -> usize {
        self.verified_against().len()
    }

    /// The double: the core's hash with the cheap set and the core's verify, each counted
    /// before it runs.
    pub fn hashing(&self) -> Hashing {
        let hashes = Arc::clone(&self.hashes);
        let verified = Arc::clone(&self.verified_against);
        Hashing::new(
            move |password| {
                *hashes
                    .lock()
                    .unwrap_or_else(|error| panic!("the hash count is poisoned: {error}")) += 1;
                password::hash(CHEAP_HASH_PARAMS, password)
            },
            move |password, stored_hash| -> Verification {
                verified
                    .lock()
                    .unwrap_or_else(|error| panic!("the verification list is poisoned: {error}"))
                    .push(stored_hash.to_owned());
                password::verify(password, stored_hash)
            },
        )
    }
}

/// Holds work inside a hashing double until the test lets it go, so that a test can act while
/// a password is being hashed or verified. Every wait has a deadline, so a rule that is
/// broken fails the test instead of hanging it.
#[derive(Default)]
pub struct Gate {
    armed: Mutex<bool>,
    entered: Mutex<usize>,
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    /// A closed gate that holds every call from now on.
    pub fn new() -> Arc<Gate> {
        let gate = Gate::unarmed();
        gate.arm();
        gate
    }

    /// A gate that lets every call pass, uncounted, until [`Gate::arm`] is called.
    pub fn unarmed() -> Arc<Gate> {
        Arc::new(Gate::default())
    }

    /// Makes the gate hold every call from now on.
    pub fn arm(&self) {
        *self
            .armed
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}")) = true;
    }

    /// Called inside the double: once the gate is armed, counts the entry, then waits until
    /// the gate is open.
    pub fn pass(&self) {
        if !*self
            .armed
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}"))
        {
            return;
        }
        *self
            .entered
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}")) += 1;
        let open = self
            .open
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}"));
        let (open, timeout) = self
            .changed
            .wait_timeout_while(open, GATE_DEADLINE, |open| !*open)
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}"));
        assert!(
            *open && !timeout.timed_out(),
            "the gate was not opened within {GATE_DEADLINE:?}"
        );
    }

    /// How many calls have entered the gate so far.
    pub fn entered(&self) -> usize {
        *self
            .entered
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}"))
    }

    /// Waits until `count` calls have entered the gate.
    pub async fn wait_entered(&self, count: usize) {
        let deadline = std::time::Instant::now() + GATE_DEADLINE;
        while self.entered() < count {
            assert!(
                std::time::Instant::now() < deadline,
                "only {} of {count} calls reached the gate within {GATE_DEADLINE:?}",
                self.entered()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Opens the gate for every call, waiting and later.
    pub fn open(&self) {
        *self
            .open
            .lock()
            .unwrap_or_else(|error| panic!("the gate is poisoned: {error}")) = true;
        self.changed.notify_all();
    }
}

/// How long a [`Gate`] waits before it fails the test.
pub const GATE_DEADLINE: Duration = Duration::from_secs(10);

/// How a test's application differs from the default.
#[derive(Default)]
pub struct Options {
    /// Trust-proxy on.
    pub trust_proxy: bool,
    /// Storage waits other than the defaults.
    pub storage: Option<StorageOptions>,
    /// A hashing of the test's own in place of the counting double.
    pub hashing: Option<Hashing>,
    /// The configured time zone by its name, such as `America/Toronto`, in place of UTC.
    pub zone: Option<&'static str>,
}

/// The application of one test: the real router over a fresh migrated database that holds no
/// book and no user, a clock at [`START`], trust-proxy off unless asked for, UTC as the
/// configured zone unless another is named, and the hashing double, prepared as `serve`
/// prepares its hashing before it listens.
pub struct App {
    pub router: Router,
    pub storage: Storage,
    pub clock: TestClock,
    pub work: Work,
    /// The zone the application was handed.
    pub zone: TimeZone,
    dir: TestDir,
}

impl App {
    /// The default application.
    pub async fn start() -> App {
        App::start_with(Options::default()).await
    }

    /// An application with `options`.
    pub async fn start_with(options: Options) -> App {
        let storage_options = options.storage.unwrap_or_default();
        let (dir, storage) = test_support::open_with(&storage_options).await;
        test_support::migrate(&storage).await;
        let clock = TestClock::start();
        let work = Work::default();
        let hashing = options.hashing.unwrap_or_else(|| work.hashing());
        hashing
            .prepare()
            .await
            .unwrap_or_else(|error| panic!("cannot prepare the hashing: {error}"));
        let zone = match options.zone {
            Some(name) => TimeZone::get(name)
                .unwrap_or_else(|error| panic!("{name} is no time zone: {error}")),
            None => TimeZone::UTC,
        };
        let router = nage_web::router(AppState {
            version: VERSION,
            health: HealthProbe::always(true),
            storage: storage.clone(),
            clock: clock.clock(),
            hashing,
            trust_proxy: options.trust_proxy,
            zone: zone.clone(),
        });
        App {
            router,
            storage,
            clock,
            work,
            zone,
            dir,
        }
    }

    /// The default application over a book set up through the core, with the admin [`ADMIN`]
    /// whose password is [`PASSWORD`].
    pub async fn with_book() -> App {
        let app = App::start().await;
        app.set_up().await;
        app
    }

    /// Sets the book up through the core at the clock's instant: the admin [`ADMIN`] with
    /// [`PASSWORD`], hashed with the cheap set.
    pub async fn set_up(&self) -> Initialized {
        test_support::set_up(&self.storage, self.clock.now(), ADMIN, PASSWORD).await
    }

    /// Sets a book up through the core at the clock's instant as [`App::set_up`] does, with
    /// the starter accounts or without them, and with the admin's display name `Sam`.
    pub async fn set_up_as(&self, starter_chart: bool) -> Initialized {
        self.set_up_in("CAD", starter_chart).await
    }

    /// Sets a book in the currency `code` up through the core at the clock's instant, as
    /// [`App::set_up_as`] does.
    pub async fn set_up_in(&self, code: &str, starter_chart: bool) -> Initialized {
        let setup = Setup {
            book_name: String::from(test_support::TEST_BOOK_NAME),
            currency: Currency::from_code(code)
                .unwrap_or_else(|| panic!("{code} is not in the currency table")),
            admin: NewUser {
                username: String::from(ADMIN),
                display_name: String::from("Sam"),
                password_hash: test_support::cheap_hash(PASSWORD),
                is_admin: true,
            },
            starter_chart,
        };
        let mut tx = self.write().await;
        let initialized = setup::initialize(&mut tx, self.clock.now(), &setup)
            .await
            .unwrap_or_else(|error| panic!("cannot set the book up: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the set-up: {error}"));
        initialized
    }

    /// Adds the user `username` with the display name `display_name` and [`OTHER_PASSWORD`],
    /// not an admin, through the core, created by the admin.
    pub async fn add_named_user(&self, username: &str, display_name: &str) -> User {
        let admin = self.credentials(ADMIN).await.user.id;
        let new = NewUser {
            username: username.to_owned(),
            display_name: display_name.to_owned(),
            password_hash: test_support::cheap_hash(OTHER_PASSWORD),
            is_admin: false,
        };
        let mut tx = self.write().await;
        let user = users::create_user(&mut tx, Some(admin), self.clock.now(), &new)
            .await
            .unwrap_or_else(|error| panic!("cannot add {username}: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit adding {username}: {error}"));
        user
    }

    /// Today as the tests reckon it: the date in the application's zone at the clock's
    /// instant, through the core's `Date::today`.
    pub fn today(&self) -> Date {
        Date::today(&self.zone, self.clock.now().to_jiff())
            .unwrap_or_else(|error| panic!("the clock's day is no date of Nage: {error}"))
    }

    /// The account whose full name is `full_name`, which must exist.
    pub async fn account(&self, full_name: &str) -> AccountId {
        let mut read = self.read().await;
        accounts::resolve(&mut read, full_name)
            .await
            .unwrap_or_else(|error| panic!("cannot resolve {full_name}: {error}"))
            .unwrap_or_else(|| panic!("no account {full_name}"))
            .id
    }

    /// Creates the account `name` under the account whose full name is `parent`, for `owner`
    /// (`None` is joint), through the core, by the admin at the clock's instant.
    pub async fn add_account(&self, parent: &str, name: &str, owner: Option<UserId>) -> AccountId {
        let admin = self.credentials(ADMIN).await.user.id;
        let parent = self.account(parent).await;
        let new = NewAccount {
            name: name.to_owned(),
            parent,
            owner,
        };
        let mut tx = self.write().await;
        let account = accounts::create_account(&mut tx, admin, self.clock.now(), &new)
            .await
            .unwrap_or_else(|error| panic!("cannot create the account {name}: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the account {name}: {error}"));
        account.id
    }

    /// Gives `account` the opening balance of `natural` minor units in natural sign, dated
    /// `on`, through the core's opening-balance operation, by the admin at the clock's
    /// instant with [`App::today`] as today.
    pub async fn open_balance(&self, account: AccountId, on: &str, natural: i64) {
        let admin = self.credentials(ADMIN).await.user.id;
        let new = NewOpeningBalance {
            account,
            date: on
                .parse()
                .unwrap_or_else(|error| panic!("{on:?} is no date: {error}")),
            amount: Money::from_minor(natural),
        };
        let mut tx = self.write().await;
        transactions::create_opening_balance(&mut tx, admin, self.clock.now(), self.today(), &new)
            .await
            .unwrap_or_else(|error| panic!("cannot create the opening balance of {on}: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the opening balance of {on}: {error}"));
    }

    /// Adds the user `username` with `password`, not an admin, through the core, created by
    /// the admin.
    pub async fn add_user(&self, username: &str, password: &str) -> User {
        let admin = self.credentials(ADMIN).await.user.id;
        test_support::add_user(
            &self.storage,
            admin,
            self.clock.now(),
            username,
            password,
            false,
        )
        .await
    }

    /// A client without a session.
    pub fn client(&self) -> Client {
        Client::over(self.router.clone())
    }

    /// A client signed in as `username` through the sign-in page.
    pub async fn signed_in(&self, username: &str, password: &str) -> Client {
        let mut client = self.client();
        let reply = client
            .post("/login", &[("username", username), ("password", password)])
            .await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the sign-in of {username}:\n{}",
            reply.body
        );
        assert!(
            client.cookie.is_some(),
            "the sign-in of {username} set no session cookie: {:?}",
            reply.set_cookies()
        );
        client
    }

    /// A read transaction on the test's database.
    pub async fn read(&self) -> ReadTransaction {
        self.storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"))
    }

    /// A write transaction on the test's database; the test commits or drops it.
    pub async fn write(&self) -> WriteTransaction {
        self.storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a write: {error}"))
    }

    /// The user typed as `username` with the stored hash, who must exist.
    pub async fn credentials(&self, username: &str) -> Credentials {
        let mut read = self.read().await;
        users::user_by_username(&mut read, username)
            .await
            .unwrap_or_else(|error| panic!("cannot look {username:?} up: {error}"))
            .unwrap_or_else(|| panic!("{username:?} finds nobody"))
    }

    /// How many users exist.
    pub async fn users(&self) -> i64 {
        let mut read = self.read().await;
        users::count_users(&mut read)
            .await
            .unwrap_or_else(|error| panic!("cannot count the users: {error}"))
    }

    /// How many rows `sessions` holds.
    pub async fn session_rows(&self) -> i64 {
        let mut read = self.read().await;
        test_support::session_rows(&mut read).await
    }

    /// The valid session the cookie value `token` names at the clock's instant, if any.
    pub async fn session_of(&self, token: &str) -> Option<SignedIn> {
        let mut read = self.read().await;
        sessions::valid_session(&mut read, &token_hash(token), self.clock.now())
            .await
            .unwrap_or_else(|error| panic!("cannot look the session up: {error}"))
    }

    /// Deactivates `user` as the admin would, at the clock's instant: the deactivation
    /// operation is milestone 8 slice 1's, so the pair is planted with the unchecked update
    /// of `test_support` (decisions 98 and 115).
    pub async fn deactivate(&self, user: UserId) {
        let admin = self.credentials(ADMIN).await.user.id;
        let mut tx = self.write().await;
        test_support::deactivate_unchecked(&mut tx, user, admin, self.clock.now()).await;
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the deactivation: {error}"));
    }

    /// The directory of the test's database.
    pub fn dir(&self) -> &TestDir {
        &self.dir
    }
}

/// The hash the core stores for the cookie value `token`: the SHA-256 of the 32 bytes that
/// `token` writes in hexadecimal (decision 108). Panics unless `token` is 64 hexadecimal
/// characters.
pub fn token_hash(token: &str) -> TokenHash {
    assert_eq!(token.len(), 64, "length of the session token {token:?}");
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&token[index * 2..index * 2 + 2], 16)
            .unwrap_or_else(|error| panic!("the session token {token:?} is not hex: {error}"));
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    TokenHash::from_digest(digest)
}

/// A request a test sends: by default it names [`HOST`] in `Host`, and a form names
/// [`ORIGIN`] in `Origin`, as a browser on one of Nage's own pages does.
pub struct Req {
    method: &'static str,
    uri: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    /// `GET uri`.
    pub fn get(uri: &str) -> Req {
        Req::new("GET", uri)
    }

    /// `method uri` with no body and no `Origin`.
    pub fn new(method: &'static str, uri: &str) -> Req {
        Req {
            method,
            uri: uri.to_owned(),
            headers: vec![(String::from("host"), String::from(HOST))],
            body: Vec::new(),
        }
    }

    /// `POST uri` with `fields` as a URL-encoded form from [`ORIGIN`].
    pub fn form(uri: &str, fields: &[(&str, &str)]) -> Req {
        let mut request = Req::new("POST", uri)
            .header("origin", ORIGIN)
            .header("content-type", "application/x-www-form-urlencoded");
        request.body = form_body(fields).into_bytes();
        request
    }

    /// Sets header `name` to `value`, replacing any value it had.
    pub fn header(mut self, name: &str, value: &str) -> Req {
        self.headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// Removes header `name`.
    pub fn without(mut self, name: &str) -> Req {
        self.headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        self
    }

    /// Replaces the body, as raw bytes.
    pub fn body(mut self, body: Vec<u8>) -> Req {
        self.body = body;
        self
    }

    fn has(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
    }
}

/// `fields` as `application/x-www-form-urlencoded` text.
pub fn form_body(fields: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (name, value) in fields {
        if !body.is_empty() {
            body.push('&');
        }
        let _ = write!(body, "{}={}", percent(name), percent(value));
    }
    body
}

/// `text` percent-encoded: every byte but ASCII letters, digits, `-`, `.`, `_` and `~`.
pub fn percent(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// An answer, read whole.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Reply {
    /// The first value of header `name`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| {
            value
                .to_str()
                .unwrap_or_else(|error| panic!("header {name} is not visible ASCII: {error}"))
        })
    }

    /// The `Location` header.
    pub fn location(&self) -> Option<&str> {
        self.header("location")
    }

    /// Every `Set-Cookie` value.
    pub fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .get_all("set-cookie")
            .iter()
            .map(|value| {
                value
                    .to_str()
                    .unwrap_or_else(|error| panic!("a Set-Cookie is not visible ASCII: {error}"))
            })
            .collect()
    }

    /// Every `Set-Cookie` of the cookie called `name`, each as its value and the sorted list
    /// of its attributes. A cookie whose name only begins with `name` is another cookie.
    pub fn cookies_named(&self, name: &str) -> Vec<(String, Vec<String>)> {
        self.set_cookies()
            .into_iter()
            .filter_map(|cookie| {
                let mut parts = cookie.split("; ");
                let value = parts
                    .next()
                    .and_then(|first| first.strip_prefix(name))
                    .and_then(|rest| rest.strip_prefix('='))?;
                let mut attributes: Vec<String> = parts.map(str::to_owned).collect();
                attributes.sort();
                Some((value.to_owned(), attributes))
            })
            .collect()
    }

    /// The one `Set-Cookie` of the session cookie, found by its name, as its value and the
    /// sorted list of its attributes; panics unless the answer has exactly one `Set-Cookie`
    /// for `nage_session`. A `Set-Cookie` for the one-time message may stand beside it.
    pub fn session_cookie(&self) -> (String, Vec<String>) {
        let mut cookies = self.cookies_named(SESSION_COOKIE);
        assert_eq!(
            cookies.len(),
            1,
            "Set-Cookie headers for {SESSION_COOKIE} among {:?}",
            self.set_cookies()
        );
        cookies.remove(0)
    }

    /// Whether the answer clears the session cookie: exactly one `Set-Cookie` for
    /// `nage_session`, with an empty value and `Max-Age=0`.
    pub fn clears_the_cookie(&self) -> bool {
        let cookies = self.cookies_named(SESSION_COOKIE);
        matches!(cookies.as_slice(), [(value, attributes)]
            if value.is_empty() && attributes.iter().any(|attribute| attribute == "Max-Age=0"))
    }

    /// The one `Set-Cookie` of the one-time message's cookie, found by its name, as its
    /// value and the sorted list of its attributes; `None` when the answer has none. Panics
    /// when the answer has more than one.
    pub fn message_cookie(&self) -> Option<(String, Vec<String>)> {
        let mut cookies = self.cookies_named(MESSAGE_COOKIE);
        assert!(
            cookies.len() <= 1,
            "Set-Cookie headers for {MESSAGE_COOKIE} among {:?}",
            self.set_cookies()
        );
        cookies.pop()
    }

    /// Whether the answer clears the one-time message's cookie: one `Set-Cookie` for
    /// `nage_message`, with an empty value and `Max-Age=0`.
    pub fn clears_the_message(&self) -> bool {
        self.message_cookie().is_some_and(|(value, attributes)| {
            value.is_empty() && attributes.iter().any(|attribute| attribute == "Max-Age=0")
        })
    }

    /// The notices of the page: for each element of class `notice`, its `role` (`status` for
    /// a confirmation, `alert` for something that was not done) and its text. A one-time
    /// message is one of them, above the page's heading.
    pub fn notices(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut rest = self.body.as_str();
        while let Some(at) = rest.find(r#"<p class="notice""#) {
            let tag_end = rest[at..].find('>').map_or(rest.len(), |index| index + at);
            let role = attribute(&rest[at..tag_end], "role").unwrap_or_default();
            let close = rest[tag_end..]
                .find("</p>")
                .map_or(rest.len(), |index| index + tag_end);
            let text = unescape(rest.get(tag_end + 1..close).unwrap_or_default());
            found.push((role, text));
            rest = &rest[close..];
        }
        found
    }

    /// The home page's figures, in order, one text each: `## <section> <total>` for a
    /// section's heading, `[<heading>]` for a group's, `<level> <name> <amount>` for an
    /// account's line and for an Other line (whose name is `Other`), with ` <owner tag>` after
    /// the name when the line names its owner, the tag as the page writes it, parentheses
    /// included, and `<name> <amount>` for the Net worth line; every amount as the page writes
    /// it.
    pub fn figures(&self) -> Vec<String> {
        let main = self.body.split("<main>").nth(1).unwrap_or_default();
        let mut out = Vec::new();
        let mut kind = "";
        let (mut name, mut owner, mut amount) = (String::new(), None::<String>, String::new());
        for chunk in main.split('<').skip(1) {
            let (tag, text) = chunk.split_once('>').unwrap_or((chunk, ""));
            let text = unescape(text.trim());
            let (element, attributes) = tag.split_once(' ').unwrap_or((tag, ""));
            match element {
                "h2" | "li" | "p" => {
                    kind = if element == "p" && !attributes.contains("net-worth") {
                        ""
                    } else {
                        tag
                    };
                    (name, owner, amount) = (String::new(), None, String::new());
                }
                "h3" => out.push(format!("[{text}]")),
                "span" if attributes.contains("amount") => amount = text,
                "span" => name = text,
                "small" if attributes.contains("owner") => owner = Some(text),
                "/h2" if !kind.is_empty() => out.push(format!("## {name} {amount}")),
                "/p" if !kind.is_empty() => out.push(format!("{name} {amount}")),
                "/li" if !kind.is_empty() => {
                    let level = kind
                        .split("level-")
                        .nth(1)
                        .map(|rest| rest.trim_end_matches('"'))
                        .unwrap_or("?");
                    let owner = owner
                        .as_ref()
                        .map(|owner| format!(" {owner}"))
                        .unwrap_or_default();
                    out.push(format!("{level} {name}{owner} {amount}"));
                }
                _ => {}
            }
            if matches!(element, "/h2" | "/p" | "/li") {
                kind = "";
            }
        }
        out
    }

    /// The page as text: the body without its tags, with the character references askama
    /// writes turned back into their characters.
    pub fn text(&self) -> String {
        let mut text = String::new();
        let mut in_tag = false;
        for character in self.body.chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        unescape(&text)
    }

    /// Whether the page shows `message`, a text of `nage_web::messages` or of the core.
    pub fn shows(&self, message: &str) -> bool {
        self.text().contains(message)
    }
}

/// `text` with the character references askama writes (`&#60;`, `&#62;`, `&#34;`, `&#39;`
/// and `&#38;`) turned back into their characters.
pub fn unescape(text: &str) -> String {
    text.replace("&#60;", "<")
        .replace("&#62;", ">")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#38;", "&")
}

/// The start tag of the form control called `name` in `html` (an `input` or a `select`).
pub fn control<'a>(html: &'a str, name: &str) -> &'a str {
    let needle = format!(r#"name="{name}""#);
    let found: Vec<&str> = super::start_tags(html, "<input")
        .into_iter()
        .chain(super::start_tags(html, "<select"))
        .filter(|tag| tag.contains(&needle))
        .collect();
    assert_eq!(found.len(), 1, "controls called {name} in:\n{html}");
    found[0]
}

/// The value of attribute `name` in the start tag `tag`, unescaped.
pub fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!(r#" {name}=""#);
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(unescape(&tag[start..end]))
}

/// Asserts the four response headers of SPEC 5.3 on `reply`, each exactly once with its
/// exact value: the three of decision 40 and `Cache-Control: no-store` (decision 119), which
/// every answer carries but a served asset, which keeps its own and is checked elsewhere.
/// `what` names the answer in the failure message.
pub fn assert_security_headers(reply: &Reply, what: &str) {
    let expected = [
        ("content-security-policy", super::CSP),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
        ("cache-control", "no-store"),
    ];
    for (name, value) in expected {
        assert_eq!(reply.header(name), Some(value), "{name} on {what}");
        assert_eq!(
            reply.headers.get_all(name).iter().count(),
            1,
            "number of {name} headers on {what}"
        );
    }
}

/// A browser of the tests: it sends its requests through the router and carries two cookies
/// by hand, the session's and the one-time message's, each found by its name: it keeps the
/// value the last answer set for a cookie and drops the cookie when an answer clears it. It
/// keeps a cookie whatever its `Max-Age`, since its clock is the test's.
pub struct Client {
    router: Router,
    /// The value of the `nage_session` cookie the client holds.
    pub cookie: Option<String>,
    /// The value of the `nage_message` cookie the client holds.
    pub message: Option<String>,
}

impl Client {
    /// A client without a session and without a message over `router`.
    pub fn over(router: Router) -> Client {
        Client {
            router,
            cookie: None,
            message: None,
        }
    }

    /// Sends `request`, with the client's cookies in one `Cookie` header unless the request
    /// names one itself.
    pub async fn send(&mut self, request: Req) -> Reply {
        let mut builder = Request::builder().method(request.method).uri(&request.uri);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let held: Vec<String> = [
            (SESSION_COOKIE, &self.cookie),
            (MESSAGE_COOKIE, &self.message),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.as_ref().map(|value| format!("{name}={value}")))
        .collect();
        if !request.has("cookie") && !held.is_empty() {
            builder = builder.header("cookie", held.join("; "));
        }
        if !request.body.is_empty() && !request.has("transfer-encoding") {
            builder = builder.header("content-length", request.body.len());
        }
        let what = format!("{} {}", request.method, request.uri);
        let request = builder
            .body(Body::from(request.body))
            .unwrap_or_else(|error| panic!("cannot build {what}: {error}"));
        let response = match self.router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| panic!("cannot read the answer to {what}: {error}"));
        let body = String::from_utf8(bytes.to_vec())
            .unwrap_or_else(|error| panic!("the answer to {what} is not UTF-8: {error}"));
        let reply = Reply {
            status,
            headers,
            body,
        };
        for (name, held) in [
            (SESSION_COOKIE, &mut self.cookie),
            (MESSAGE_COOKIE, &mut self.message),
        ] {
            for (value, attributes) in reply.cookies_named(name) {
                let cleared = attributes.iter().any(|attribute| attribute == "Max-Age=0");
                *held = if value.is_empty() || cleared {
                    None
                } else {
                    Some(value)
                };
            }
        }
        reply
    }

    /// `HEAD uri`.
    pub async fn head(&mut self, uri: &str) -> Reply {
        self.send(Req::new("HEAD", uri)).await
    }

    /// `GET uri`.
    pub async fn get(&mut self, uri: &str) -> Reply {
        self.send(Req::get(uri)).await
    }

    /// `POST uri` with `fields` as a form from Nage's own page.
    pub async fn post(&mut self, uri: &str, fields: &[(&str, &str)]) -> Reply {
        self.send(Req::form(uri, fields)).await
    }
}
