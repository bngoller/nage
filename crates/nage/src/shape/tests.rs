//! The form of what the commands print (decisions 157, 158 and 159): the labelled lines, the
//! colours on the labels alone, a path on one line and the system's own words in one pair of
//! brackets.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use super::{BOLD, Finished, GREEN, Paint, RED, RESET, Refused, no_color, shown, system_words};

/// An error of Nage's own wrapping another, as a command's failure wraps the system's.
#[derive(Debug)]
struct Wrapping<E> {
    words: &'static str,
    inner: E,
}

impl<E: Error> fmt::Display for Wrapping<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.words, self.inner)
    }
}

impl<E: Error + 'static> Error for Wrapping<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.inner)
    }
}

/// What SQLite answers when a database is opened in a folder that does not exist: a real sqlx
/// error, wrapping words and all.
fn unable_to_open() -> sqlx::Error {
    use sqlx::ConnectOptions as _;

    let dir = tempfile_dir();
    let missing = dir.join("no such folder").join("nage.sqlite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"));
    let opened = runtime.block_on(async {
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&missing)
            .create_if_missing(false)
            .connect()
            .await
    });
    match opened {
        Ok(_) => panic!("{} was opened", missing.display()),
        Err(error) => error,
    }
}

/// A folder of this test's own that holds nothing.
fn tempfile_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("nage-shape-test-{}", std::process::id()))
}

/// Error, Why and What now, each at the left edge, each one line with a line break; a reason of
/// several lines stands under a bare `Why:`, each line indented by two spaces.
///
/// Proves: decision 157 (Error, Why and What now; a reason of several lines under a bare Why)
#[test]
fn a_refusal_is_three_labelled_lines_at_the_left_edge() {
    let refused = Refused::new(
        "The user was not created.",
        "Nobody has the username \"sma\", which --by named as the admin adding the user.",
        "Check the spelling, or name another admin with --by, then run the command again.",
    );
    assert_eq!(
        refused.render(Paint::PLAIN),
        "Error: The user was not created.\n\
         Why: Nobody has the username \"sma\", which --by named as the admin adding the user.\n\
         What now: Check the spelling, or name another admin with --by, then run the command again.\n"
    );

    let listing = Refused::listing(
        "The book did not pass every check: 2 problems were found.",
        vec![
            String::from("INV-13 account 14 has 2 live opening balances"),
            String::from("FK postings row 91 references a missing accounts row"),
        ],
        "Nothing was changed.",
    );
    assert_eq!(
        listing.render(Paint::PLAIN),
        "Error: The book did not pass every check: 2 problems were found.\n\
         Why:\n\
         \x20 INV-13 account 14 has 2 live opening balances\n\
         \x20 FK postings row 91 references a missing accounts row\n\
         What now: Nothing was changed.\n"
    );
}

/// `Success:` and its sentence, the facts indented by two spaces as `Label: value`, and
/// `What now:` only when there is a next step.
///
/// Proves: decision 157 (Success, the facts indented by two spaces, What now only when there is a next step)
#[test]
fn a_finished_command_is_a_sentence_its_facts_and_a_next_step() {
    let set_up = Finished::new("The book \"Household\" was set up.")
        .fact("Currency", "CAD")
        .fact("Starter accounts", "added")
        .fact("Admin", "sam (Sam)")
        .fact("Database", "/srv/nage/data/nage.sqlite")
        .then("Start the server; sam can then sign in.");
    assert_eq!(
        set_up.render(Paint::PLAIN),
        "Success: The book \"Household\" was set up.\n\
         \x20 Currency: CAD\n\
         \x20 Starter accounts: added\n\
         \x20 Admin: sam (Sam)\n\
         \x20 Database: /srv/nage/data/nage.sqlite\n\
         What now: Start the server; sam can then sign in.\n"
    );

    let up_to_date = Finished::new("The database is already up to date; nothing was changed.")
        .fact("Database version", "2");
    assert_eq!(
        up_to_date.render(Paint::PLAIN),
        "Success: The database is already up to date; nothing was changed.\n\
         \x20 Database version: 2\n"
    );

    assert_eq!(
        Finished::new("Nothing to check: no book is set up yet.").render(Paint::PLAIN),
        "Success: Nothing to check: no book is set up yet.\n"
    );
}

/// On a terminal without `NO_COLOR` the labels alone are coloured, `Error:` red, `Success:`
/// green, `Why:` and `What now:` bold, each followed by a reset; anywhere else nothing is. An
/// empty `NO_COLOR` counts as unset; any other value, whatever its bytes, turns colour off.
///
/// Proves: decision 158 (red, green and bold labels on a terminal without NO_COLOR; plain otherwise)
#[test]
fn only_the_labels_are_coloured_and_only_when_painted() {
    let painted = Paint::new(true, false);
    assert!(painted.colours());
    let refused = Refused::new("Nage did not start.", "Because.", "Start it again.");
    assert_eq!(
        refused.render(painted),
        format!(
            "{RED}Error:{RESET} Nage did not start.\n\
             {BOLD}Why:{RESET} Because.\n\
             {BOLD}What now:{RESET} Start it again.\n"
        )
    );
    let listing = Refused::listing("Not checked.", vec![String::from("INV-1 one")], "Keep it.");
    assert_eq!(
        listing.render(painted),
        format!(
            "{RED}Error:{RESET} Not checked.\n{BOLD}Why:{RESET}\n  INV-1 one\n{BOLD}What now:{RESET} Keep it.\n"
        )
    );
    let finished = Finished::new("Done.")
        .fact("Database", "/srv/nage/data/nage.sqlite")
        .then("Start the server.");
    assert_eq!(
        finished.render(painted),
        format!(
            "{GREEN}Success:{RESET} Done.\n  Database: /srv/nage/data/nage.sqlite\n{BOLD}What now:{RESET} Start the server.\n"
        )
    );

    for plain in [
        Paint::new(true, true),
        Paint::new(false, false),
        Paint::new(false, true),
    ] {
        assert!(!plain.colours());
        assert_eq!(
            refused.render(plain),
            "Error: Nage did not start.\nWhy: Because.\nWhat now: Start it again.\n"
        );
        assert_eq!(
            finished.render(plain),
            "Success: Done.\n  Database: /srv/nage/data/nage.sqlite\nWhat now: Start the server.\n"
        );
        assert!(!listing.render(plain).contains('\u{1b}'));
    }

    // NO_COLOR as the environment holds it: unset or empty leaves the colour on, any other value,
    // one that is not UTF-8 text included, turns it off.
    assert!(!no_color(None));
    assert!(!no_color(Some(OsStr::new(""))));
    assert!(no_color(Some(OsStr::new("1"))));
    assert!(no_color(Some(OsStr::new("false"))));
    assert!(no_color(Some(OsStr::from_bytes(b"\xFF"))));
}

/// A path is shown without quotation marks, its control characters escaped as Rust escapes
/// them, so a folder whose name holds a line break stays on one line; everything else, an
/// apostrophe or a space included, stands as it is.
///
/// Proves: decision 157, ruling P10 (a line break in a folder's name escaped, no quotation marks)
#[test]
fn a_path_is_shown_on_one_line_without_quotation_marks() {
    assert_eq!(
        shown(Path::new("/srv/nage/data/nage.sqlite")),
        "/srv/nage/data/nage.sqlite"
    );
    assert_eq!(
        shown(Path::new("/srv/nage/two\nlines")),
        "/srv/nage/two\\nlines"
    );
    assert_eq!(
        shown(Path::new("/srv/nage/tab\there\rand\u{1b}escape")),
        "/srv/nage/tab\\there\\rand\\u{1b}escape"
    );
    assert_eq!(
        shown(Path::new("/srv/sam's data/with space")),
        "/srv/sam's data/with space"
    );
    let line = format!(
        "Why: There is no database at {}.",
        shown(Path::new("/srv/nage/two\nlines/nage.sqlite"))
    );
    assert_eq!(line.lines().count(), 1, "{line}");
    assert!(!line.contains('"'), "{line}");
}

/// The system's own words are the innermost message alone, rewritten so that no bracket stands
/// inside the one pair the reason puts around them: an operating system's error as `Address in
/// use, os error 98`, SQLite's as `unable to open database file, code 14`, without the words
/// that wrap them.
///
/// Proves: decision 159, ruling P9 (the innermost message: Address in use, os error 98; unable to open database file, code 14; no wrapping words)
#[test]
fn the_system_words_stand_alone_in_one_pair_of_brackets() {
    // The C library names the error: musl, in the image, "Address in use"; glibc "Address
    // already in use".
    let in_use = io::Error::from_raw_os_error(98);
    let words = system_words(&in_use);
    assert!(
        [
            "Address in use, os error 98",
            "Address already in use, os error 98"
        ]
        .contains(&words.as_str()),
        "{words:?}"
    );

    let denied = Wrapping {
        words: "cannot create the data folder",
        inner: io::Error::from_raw_os_error(13),
    };
    assert_eq!(system_words(&denied), "Permission denied, os error 13");

    let sqlite = unable_to_open();
    assert!(
        sqlite.to_string().contains("code: 14"),
        "SQLite did not answer code 14: {sqlite}"
    );
    assert_eq!(
        system_words(&sqlite),
        "unable to open database file, code 14"
    );
    let wrapped = Wrapping {
        words: "the database could not be opened",
        inner: sqlite,
    };
    let words = system_words(&wrapped);
    assert_eq!(words, "unable to open database file, code 14");
    assert!(!words.contains('(') && !words.contains(')'), "{words}");

    let own = io::Error::other("the instant is before 1970-01-01T00:00:00Z");
    assert_eq!(
        system_words(&own),
        "the instant is before 1970-01-01T00:00:00Z"
    );
}
