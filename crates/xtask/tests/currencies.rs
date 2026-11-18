//! `cargo xtask currencies` (decision 87): the table it writes from a list shaped like ISO 4217
//! list one, and the lists it refuses without touching the table.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use nage_core::test_support::TestDir;

/// The table `currencies_writes_the_sorted_table` expects, in the generator's layout.
const EXPECTED_TABLE: &str =
    "//! The ISO 4217 currencies with 0 to 3 minor digits, written by `cargo xtask currencies` from
//! ISO 4217 list one published 2031-02-03: every code of a currency entry, leaving out the
//! entries whose minor units are N.A. and the fund entries (SPEC decision 87). Do not edit by
//! hand: download the list again and rerun the command (README, Currency table).

use crate::money::MinorDigits;

use super::{Currency, row};

/// The publication date of the list the table was written from.
pub const PUBLISHED: &str = \"2031-02-03\";

/// The table, sorted by code.
pub const CURRENCIES: &[Currency] = &[
    row(\"EUR\", MinorDigits::TWO),
    row(\"JPY\", MinorDigits::ZERO),
    row(\"KWD\", MinorDigits::THREE),
    row(\"QQO\", MinorDigits::ONE),
    row(\"ZZD\", MinorDigits::TWO),
];
";

/// An entry without a currency, as list one has for Antarctica.
const NO_CURRENCY: &str = "\t\t<CcyNtry>\r\n\t\t\t<CtryNm>KAPPA</CtryNm>\r\n\t\t\t<CcyNm>No universal currency</CcyNm>\r\n\t\t</CcyNtry>\r\n";

/// One `CcyNtry` in list one's layout: CRLF line ends, tab indentation, the fund flag as an
/// attribute of `CcyNm`.
fn entry(country: &str, name: &str, code: &str, units: &str, fund: bool) -> String {
    let flag = if fund { " IsFund=\"true\"" } else { "" };
    format!(
        "\t\t<CcyNtry>\r\n\t\t\t<CtryNm>{country}</CtryNm>\r\n\t\t\t<CcyNm{flag}>{name}</CcyNm>\r\n\t\t\t<Ccy>{code}</Ccy>\r\n\t\t\t<CcyNbr>999</CcyNbr>\r\n\t\t\t<CcyMnrUnts>{units}</CcyMnrUnts>\r\n\t\t</CcyNtry>\r\n"
    )
}

/// A list with the publication date `published` (no `Pblshd` attribute when `None`) holding
/// `entries`, in list one's layout.
fn list(published: Option<&str>, entries: &[String]) -> String {
    let root = match published {
        Some(date) => format!("<ISO_4217 Pblshd=\"{date}\">"),
        None => String::from("<ISO_4217>"),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n{root}\r\n\t<CcyTbl>\r\n{}\t</CcyTbl>\r\n</ISO_4217>",
        entries.concat()
    )
}

/// Runs `xtask currencies --from <from> --out <out>`.
fn currencies(from: &Path, out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("currencies")
        .arg("--from")
        .arg(from)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap_or_else(|error| panic!("cannot run xtask: {error}"))
}

/// Runs the generator on `xml`, written as `<dir>/list-one.xml`, into `<dir>/table.rs`.
fn run(dir: &TestDir, xml: &str) -> Output {
    let from = dir.path().join("list-one.xml");
    fs::write(&from, xml).unwrap_or_else(|error| panic!("cannot write the list: {error}"));
    currencies(&from, &dir.path().join("table.rs"))
}

/// The table holds every code of a currency entry with 0 to 3 minor digits, sorted and each once,
/// in the generator's layout with the list's publication date, and the summary counts them: a
/// code two countries name (EUR) appears once, one digit maps to `ONE`; a fund entry (skipped
/// unchecked, whatever its code or units), an entry with N.A. minor units, an entry without a
/// currency and a unit with four digits are left out, and an entity in a country's name is read
/// through.
///
/// Proves: decision 87.
#[test]
fn currencies_writes_the_sorted_table() {
    let dir = TestDir::new();
    let xml = list(
        Some("2031-02-03"),
        &[
            entry("ZETA", "Zeta dollar", "ZZD", "2", false),
            entry("ALPHA", "Euro", "EUR", "2", false),
            entry("BETA &amp; GAMMA", "Euro", "EUR", "2", false),
            entry("ALPHA", "Alpha fund", "AFU", "2", true),
            entry("IOTA", "Iota fund", "IOTAF", "two", true),
            entry("ZZ08_Gold", "Gold", "XAU", "N.A.", false),
            NO_CURRENCY.to_owned(),
            entry("DELTA", "Delta unit", "DLU", "4", false),
            entry("EPSILON", "Yen", "JPY", "0", false),
            entry("ETA", "Dinar", "KWD", "3", false),
            entry("THETA", "Theta unit", "QQO", "1", false),
        ],
    );
    let output = run(&dir, &xml);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}; stderr: {stderr}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("5 currencies from ISO 4217 list one published 2031-02-03, written to "),
        "stdout: {stdout}"
    );
    let table = fs::read_to_string(dir.path().join("table.rs"))
        .unwrap_or_else(|error| panic!("cannot read the table: {error}"));
    assert_eq!(table, EXPECTED_TABLE);
}

/// A list the generator cannot read is refused with exit 1 and a message naming the problem, and
/// the table file is left as it was: no publication date, a date not shaped `YYYY-MM-DD`, a code
/// that is not three capital letters, minor units that are neither a number nor N.A., one code
/// with two digit counts, no currency with 0 to 3 digits, a malformed document, a document cut
/// before its root element closes; a missing list is refused and writes no table.
///
/// Proves: decision 87.
#[test]
fn currencies_refuses_a_list_it_cannot_read() {
    let euro = || entry("ALPHA", "Euro", "EUR", "2", false);
    for (xml, expected) in [
        (list(None, &[euro()]), "no publication date"),
        (list(Some("3 Feb 2031"), &[euro()]), "is not YYYY-MM-DD"),
        (list(Some("2031/02/03"), &[euro()]), "is not YYYY-MM-DD"),
        (list(Some("2031-02-033"), &[euro()]), "is not YYYY-MM-DD"),
        (list(Some("YYYY-MM-DD"), &[euro()]), "is not YYYY-MM-DD"),
        (list(Some("2031-02-3"), &[euro()]), "is not YYYY-MM-DD"),
        (list(Some("2031--2-03"), &[euro()]), "is not YYYY-MM-DD"),
        (
            list(
                Some("2031-02-03"),
                &[entry("ALPHA", "Euro", "EURO", "2", false)],
            ),
            "is not three capital letters",
        ),
        (
            list(
                Some("2031-02-03"),
                &[entry("ALPHA", "Euro", "EU", "2", false)],
            ),
            "is not three capital letters",
        ),
        (
            list(
                Some("2031-02-03"),
                &[entry("ALPHA", "Euro", "EU1", "2", false)],
            ),
            "is not three capital letters",
        ),
        (
            list(
                Some("2031-02-03"),
                &[entry("ALPHA", "Euro", "Eur", "2", false)],
            ),
            "is not three capital letters",
        ),
        (
            list(
                Some("2031-02-03"),
                &[entry("ALPHA", "Euro", "EUR", "two", false)],
            ),
            "neither a number nor N.A.",
        ),
        (
            list(
                Some("2031-02-03"),
                &[euro(), entry("BETA", "Euro", "EUR", "3", false)],
            ),
            "EUR has 2 and 3 minor digits",
        ),
        (
            list(
                Some("2031-02-03"),
                &[entry("ZZ08_Gold", "Gold", "XAU", "N.A.", false)],
            ),
            "holds no currency",
        ),
        (
            String::from("<ISO_4217 Pblshd=\"2031-02-03\"><CcyTbl><CcyNtry></CcyTbl></ISO_4217>"),
            "not ISO 4217 list one: ill-formed document",
        ),
        (
            String::from("<ISO_4217 Pblshd=\"2031-02-03\"><CcyTbl>")
                + &entry("ALPHA", "Euro", "EUR", "2", false),
            "not ISO 4217 list one: it ends before </ISO_4217>",
        ),
        (
            list(Some("2031-02-03"), &[euro()]).replace("</ISO_4217>", ""),
            "not ISO 4217 list one: it ends before </ISO_4217>",
        ),
    ] {
        let dir = TestDir::new();
        let out = dir.path().join("table.rs");
        fs::write(&out, "untouched\n").unwrap_or_else(|error| panic!("cannot write: {error}"));
        let output = run(&dir, &xml);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{expected:?}; stderr: {stderr}"
        );
        assert!(
            stderr.starts_with("xtask currencies: ") && stderr.contains(expected),
            "{expected:?}; stderr: {stderr}"
        );
        assert_eq!(
            fs::read_to_string(&out).unwrap_or_default(),
            "untouched\n",
            "{expected:?}"
        );
    }
    let dir = TestDir::new();
    let out = dir.path().join("table.rs");
    let output = currencies(&dir.path().join("missing.xml"), &out);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("reading "), "stderr: {stderr}");
    assert!(!out.exists(), "a table was written");
}
