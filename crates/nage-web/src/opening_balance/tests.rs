//! The opening-balance form as the core takes it, without a server (decision 141, the pause's
//! P3): the web hands the boxes on untouched and judges no rule of the core.

use nage_core::transactions::{OpeningForm, OpeningNamed, TransactionId};

use super::{OpeningBalanceForm, core_form};

/// The form as a request sends it.
fn sent(
    date: &str,
    amount: &str,
    below_zero: Option<&str>,
    entry: Option<&str>,
    version: Option<&str>,
) -> OpeningBalanceForm {
    OpeningBalanceForm {
        date: date.to_owned(),
        amount: amount.to_owned(),
        below_zero: below_zero.map(str::to_owned),
        entry: entry.map(str::to_owned),
        version: version.map(str::to_owned),
    }
}

/// The date box and the amount box reach the core exactly as typed, white space, a minus, a
/// dollar sign and a comma out of place untouched; the tick box is ticked whenever the request
/// carries it, whatever its value; a form without `entry` names nothing (`New`), and an entry or
/// a version is a number only when it is written as Nage prints one.
///
/// Proves: Decision 141 with the pause's P3: the date and the amount reach the core as typed, white space, a minus and a dollar sign untouched, the tick as sent; no entry is New, and an entry or a version not written as Nage prints a number is None.
#[test]
fn the_form_reaches_the_core_as_sent() {
    for (date, amount) in [
        ("2026-09-30", "2,480.15"),
        (" 2026-09-30 ", " 2,480.15 "),
        ("", ""),
        ("30/09/2026", "-35.20"),
        ("2026-02-30", "$1,204.80"),
        ("\t2026-09-30", "-$ 12"),
        ("<b>", "1.234,56"),
    ] {
        for (tick, below_zero) in [(None, false), (Some("1"), true), (Some(""), true)] {
            assert_eq!(
                core_form(sent(date, amount, tick, None, None)),
                OpeningForm {
                    date: date.to_owned(),
                    amount: amount.to_owned(),
                    below_zero,
                    named: OpeningNamed::New,
                },
                "web: the form reaches the core as sent: {date:?}, {amount:?}, tick {tick:?}"
            );
        }
    }
    for (entry, version, id, expected) in [
        (Some("12"), Some("3"), Some(TransactionId(12)), Some(3)),
        (Some("12"), None, Some(TransactionId(12)), None),
        (Some("012"), Some("03"), None, None),
        (Some("+12"), Some("-3"), None, None),
        (Some(""), Some(""), None, None),
        (Some("12 "), Some(" 3"), None, None),
        (Some("abc"), Some("1.0"), None, None),
        (
            Some("99999999999999999999"),
            Some("99999999999999999999"),
            None,
            None,
        ),
    ] {
        assert_eq!(
            core_form(sent("2026-09-30", "1", None, entry, version)).named,
            OpeningNamed::Entry {
                id,
                version: expected
            },
            "web: the form reaches the core as sent: entry {entry:?}, version {version:?}"
        );
    }
}
