//! How the server hashes and verifies passwords (SPEC 5.3 and 5.4, decision 107): through a
//! pair of functions it is handed, always in a blocking thread, never inside a database
//! transaction, and only [`MAX_CONCURRENT_HASHES`] at once. `serve` hands argon2id with SPEC
//! 5.3's parameters ([`Hashing::argon2id`]); tests hand a double that counts, so that what a
//! sign-in costs is asserted by counting and never by timing (decision 115).

use std::sync::{Arc, OnceLock};

use nage_core::password::{self, HashError, HashParams, Verification};

use crate::AppError;

/// How many hashes and verifications run at once (decision 107): one takes about 19 MiB at
/// SPEC 5.3's parameters, so four take about 76 MiB however many attempts arrive together. A
/// further attempt waits for its turn; it is not refused.
pub const MAX_CONCURRENT_HASHES: usize = 4;

/// The password the dummy hash is made from. Its value never matters: a sign-in for an
/// unknown username is verified against the dummy hash and refused whatever the result.
const DUMMY_PASSWORD: &str = "nage dummy password";

type HashFn = dyn Fn(&str) -> Result<String, HashError> + Send + Sync;
type VerifyFn = dyn Fn(&str, &str) -> Verification + Send + Sync;

/// The hashing the application is handed (decisions 107 and 113), injectable like the health
/// probe. Cloning shares the functions, the dummy hash and the limit.
#[derive(Clone)]
pub struct Hashing {
    inner: Arc<Inner>,
}

struct Inner {
    hash: Box<HashFn>,
    verify: Box<VerifyFn>,
    /// The dummy hash, made once by the hash function: [`Hashing::prepare`] makes it before
    /// any request.
    dummy_hash: OnceLock<String>,
}

impl Hashing {
    /// The production hashing: `nage_core::password::hash` with `params` and
    /// `nage_core::password::verify`. `serve` hands `HashParams::SPEC`. Nothing is hashed
    /// here; [`Hashing::prepare`] makes the dummy hash.
    pub fn argon2id(params: HashParams) -> Self {
        Self::new(
            move |password| password::hash(params, password),
            password::verify,
        )
    }

    /// A hashing over any pair of functions: `hash` makes the stored hash of a new password
    /// (and of the dummy), `verify` takes the typed password and a stored hash, in that order,
    /// as `nage_core::password::verify` does. Tests wrap the real pair to count the calls.
    pub fn new<H, V>(hash: H, verify: V) -> Self
    where
        H: Fn(&str) -> Result<String, HashError> + Send + Sync + 'static,
        V: Fn(&str, &str) -> Verification + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(Inner {
                hash: Box::new(hash),
                verify: Box::new(verify),
                dummy_hash: OnceLock::new(),
            }),
        }
    }

    /// Makes the dummy hash, once, with the hash function (decision 107): `serve` calls this
    /// before it listens, so that no request pays for it and a hasher that cannot hash stops
    /// the start instead of a sign-in. A second call hashes nothing. A hashing that was never
    /// prepared makes the dummy hash at the first verification that needs it.
    pub async fn prepare(&self) -> Result<(), HashError> {
        if self.inner.dummy_hash.get().is_some() {
            return Ok(());
        }
        let inner = Arc::clone(&self.inner);
        blocking(move || inner.dummy_hash().map(|_| ()))
            .await
            .map_err(HashError::Hasher)?
    }

    /// The hash of a new `password`, made in a blocking thread under the limit. The rules are
    /// the hash function's: a password that breaks one is `Ok(Err(..))` with the rule's
    /// refusal, for the form to show; a hasher or a thread that fails is
    /// `Err(AppError::Internal)`.
    pub(crate) async fn hash(
        &self,
        password: String,
    ) -> Result<Result<String, password::PasswordError>, AppError> {
        let inner = Arc::clone(&self.inner);
        match blocking(move || (inner.hash)(&password))
            .await
            .map_err(AppError::Internal)?
        {
            Ok(hash) => Ok(Ok(hash)),
            Err(HashError::Password(refusal)) => Ok(Err(refusal)),
            Err(error @ HashError::Hasher(_)) => Err(AppError::Internal(error.to_string())),
        }
    }

    /// Whether `password` is the one `stored_hash` was made from, verified in a blocking
    /// thread under the limit (decisions 106 and 107). Without a stored hash (an unknown
    /// username), or with one that cannot be read, the password is verified against the dummy
    /// hash instead and the answer is `false`, so that every attempt costs one verification
    /// whatever its reason; a stored hash that cannot be read is logged at `error`, without
    /// the hash. A thread that fails is `Err(AppError::Internal)`.
    pub(crate) async fn verify(
        &self,
        password: String,
        stored_hash: Option<String>,
    ) -> Result<bool, AppError> {
        let inner = Arc::clone(&self.inner);
        blocking(move || inner.verify_or_dummy(&password, stored_hash.as_deref()))
            .await
            .map_err(AppError::Internal)?
            .map_err(|error| AppError::Internal(format!("cannot make the dummy hash: {error}")))
    }
}

use tokio::sync::Semaphore;

/// The permits behind [`MAX_CONCURRENT_HASHES`], one per hash or verification in progress.
/// They belong to the process, not to one [`Hashing`]: the limit bounds the memory the server
/// spends on hashing, and `serve` builds one hashing, so the two are the same there.
static PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_HASHES);

impl Inner {
    /// The dummy hash, made by the hash function at the first call and kept. Two first calls
    /// at once may both hash; the first to finish is kept.
    fn dummy_hash(&self) -> Result<&str, HashError> {
        if let Some(hash) = self.dummy_hash.get() {
            return Ok(hash);
        }
        let made = (self.hash)(DUMMY_PASSWORD)?;
        Ok(self.dummy_hash.get_or_init(|| made))
    }

    /// Whether `password` verifies against `stored_hash`; without one, or with one that
    /// cannot be read, `password` is verified against the dummy hash and the answer is false
    /// whatever that verification says, so that every attempt costs a verification.
    fn verify_or_dummy(
        &self,
        password: &str,
        stored_hash: Option<&str>,
    ) -> Result<bool, HashError> {
        if let Some(stored_hash) = stored_hash {
            match (self.verify)(password, stored_hash) {
                Verification::Verified => return Ok(true),
                Verification::WrongPassword => return Ok(false),
                Verification::UnreadableHash => {
                    tracing::error!(
                        "a stored password hash cannot be read; the attempt is verified against the dummy hash and refused"
                    );
                }
            }
        }
        let dummy_hash = self.dummy_hash()?;
        // No user holds the dummy hash, so what it answers changes nothing.
        let _ = (self.verify)(password, dummy_hash);
        Ok(false)
    }
}

/// Runs `work` in a blocking thread once a permit of [`PERMITS`] is free, waiting for its turn
/// meanwhile, and holds the permit until the work is done. A thread that panics or is
/// cancelled is the error, in words.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _permit = PERMITS
        .acquire()
        .await
        .map_err(|error| format!("the hashing limit is closed: {error}"))?;
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("the hashing thread failed: {error}"))
}
