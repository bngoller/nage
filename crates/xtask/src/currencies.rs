//! `xtask currencies` (SPEC decision 87): reads ISO 4217 list one, the XML file SIX Group
//! publishes, and writes nage-core's currency table: every code of a currency entry whose minor
//! units are 0 to 3, sorted and each once, leaving out the entries whose minor units are N.A.
//! (precious metals, special drawing rights, bond-market and other units of account, the test and
//! no-currency codes) and the fund entries. It reads only the file it is given; the maintainer
//! downloads it (README, Currency table).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use quick_xml::XmlVersion;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

/// The table file's text before its rows; `{published}` stands for the list's publication date.
const HEADER: &str =
    "//! The ISO 4217 currencies with 0 to 3 minor digits, written by `cargo xtask currencies` from
//! ISO 4217 list one published {published}: every code of a currency entry, leaving out the
//! entries whose minor units are N.A. and the fund entries (SPEC decision 87). Do not edit by
//! hand: download the list again and rerun the command (README, Currency table).

use crate::money::MinorDigits;

use super::{Currency, row};

/// The publication date of the list the table was written from.
pub const PUBLISHED: &str = \"{published}\";

/// The table, sorted by code.
pub const CURRENCIES: &[Currency] = &[
";

/// Why the table could not be written; nothing was.
#[derive(Debug)]
pub(crate) enum CurrenciesError {
    /// The list could not be read.
    Read { path: PathBuf, source: io::Error },
    /// The list is not well-formed XML.
    Xml(quick_xml::Error),
    /// The root element has no `Pblshd` attribute.
    NoPublicationDate,
    /// The `Pblshd` attribute is not shaped `YYYY-MM-DD`.
    BadPublicationDate(String),
    /// A currency entry's code is not three capital letters.
    BadCode(String),
    /// A currency entry's minor units are neither a number nor N.A.
    BadMinorUnits { code: String, units: String },
    /// One code kept with two digit counts.
    Conflict { code: String, first: u8, second: u8 },
    /// The document ends before its root element closes (a truncated download).
    Unfinished,
    /// The list holds no currency with 0 to 3 minor digits.
    NoCurrency,
    /// The table could not be written.
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for CurrenciesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "reading {}: {source}", path.display()),
            Self::Xml(error) => write!(f, "not ISO 4217 list one: {error}"),
            Self::NoPublicationDate => f.write_str("the list has no publication date (Pblshd)"),
            Self::BadPublicationDate(date) => {
                write!(f, "the publication date {date:?} is not YYYY-MM-DD")
            }
            Self::BadCode(code) => write!(f, "the code {code:?} is not three capital letters"),
            Self::BadMinorUnits { code, units } => {
                write!(
                    f,
                    "{code}: the minor units {units:?} are neither a number nor N.A."
                )
            }
            Self::Conflict {
                code,
                first,
                second,
            } => {
                write!(f, "{code} has {first} and {second} minor digits")
            }
            Self::Unfinished => f.write_str("not ISO 4217 list one: it ends before </ISO_4217>"),
            Self::NoCurrency => f.write_str("the list holds no currency with 0 to 3 minor digits"),
            Self::Write { path, source } => write!(f, "writing {}: {source}", path.display()),
        }
    }
}

impl From<quick_xml::Error> for CurrenciesError {
    fn from(error: quick_xml::Error) -> Self {
        Self::Xml(error)
    }
}

impl From<AttrError> for CurrenciesError {
    fn from(error: AttrError) -> Self {
        Self::Xml(quick_xml::Error::from(error))
    }
}

/// Reads the list at `from`, writes the table beside `out` and renames it over `out`, and returns
/// the line to print; on any failure `out` is left as it was.
pub(crate) fn run(from: &Path, out: &Path) -> Result<String, CurrenciesError> {
    let xml = fs::read_to_string(from).map_err(|source| CurrenciesError::Read {
        path: from.to_path_buf(),
        source,
    })?;
    let list = read_list(&xml)?;
    let written = out.with_extension("rs.new");
    fs::write(&written, render(&list))
        .and_then(|()| fs::rename(&written, out))
        .map_err(|source| {
            let _ = fs::remove_file(&written);
            CurrenciesError::Write {
                path: out.to_path_buf(),
                source,
            }
        })?;
    Ok(format!(
        "{} currencies from ISO 4217 list one published {}, written to {}",
        list.currencies.len(),
        list.published,
        out.display()
    ))
}

/// A list's publication date and its currencies: code to minor digits, sorted by code.
struct List {
    published: String,
    currencies: BTreeMap<String, u8>,
}

/// A currency entry (`CcyNtry`) while it is read.
#[derive(Default)]
struct Entry {
    code: Option<String>,
    minor_units: Option<String>,
    fund: bool,
}

/// The element whose text is being read.
#[derive(Clone, Copy)]
enum Field {
    Other,
    Code,
    MinorUnits,
}

/// Reads the root's `Pblshd`, then each `CcyNtry`'s `Ccy`, its `CcyMnrUnts` and the fund flag on
/// its `CcyNm` (`IsFund="true"`).
fn read_list(xml: &str) -> Result<List, CurrenciesError> {
    let mut reader = Reader::from_str(xml);
    let mut published = None;
    let mut currencies = BTreeMap::new();
    let mut entry = Entry::default();
    let mut field = Field::Other;
    let mut closed = false;
    loop {
        match reader.read_event()? {
            Event::Start(start) => {
                field = Field::Other;
                match start.name().as_ref() {
                    "ISO_4217" => published = attribute(&start, "Pblshd")?,
                    "CcyNtry" => entry = Entry::default(),
                    "CcyNm" => entry.fund = attribute(&start, "IsFund")?.as_deref() == Some("true"),
                    "Ccy" => field = Field::Code,
                    "CcyMnrUnts" => field = Field::MinorUnits,
                    _ => {}
                }
            }
            Event::Text(text) => match field {
                Field::Code => entry.code.get_or_insert_default().push_str(&text),
                Field::MinorUnits => entry.minor_units.get_or_insert_default().push_str(&text),
                Field::Other => {}
            },
            Event::End(end) => {
                field = Field::Other;
                if end.name().as_ref() == "CcyNtry" {
                    add(&mut currencies, std::mem::take(&mut entry))?;
                } else if end.name().as_ref() == "ISO_4217" {
                    closed = true;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !closed {
        return Err(CurrenciesError::Unfinished);
    }
    let published = published.ok_or(CurrenciesError::NoPublicationDate)?;
    if !is_iso_date(&published) {
        return Err(CurrenciesError::BadPublicationDate(published));
    }
    if currencies.is_empty() {
        return Err(CurrenciesError::NoCurrency);
    }
    Ok(List {
        published,
        currencies,
    })
}

/// The value of the attribute `name` on `start`, if it has one.
fn attribute(start: &BytesStart<'_>, name: &str) -> Result<Option<String>, CurrenciesError> {
    match start.try_get_attribute(name)? {
        Some(value) => Ok(Some(
            value
                .normalized_value(XmlVersion::Implicit1_0)?
                .into_owned(),
        )),
        None => Ok(None),
    }
}

/// Adds a finished entry: an entry without a code, a fund entry and an entry whose minor units are
/// N.A. are left out; any other must have a code of three capital letters and a number of minor
/// units that fits a `u8`, is left out when that number is more than three, and a code kept before
/// must have the same digits.
fn add(currencies: &mut BTreeMap<String, u8>, entry: Entry) -> Result<(), CurrenciesError> {
    let Some(code) = entry.code else {
        return Ok(());
    };
    let code = code.trim().to_owned();
    let units = entry.minor_units.unwrap_or_default();
    let units = units.trim();
    if entry.fund || units == "N.A." {
        return Ok(());
    }
    if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(CurrenciesError::BadCode(code));
    }
    let digits: u8 = units.parse().map_err(|_| CurrenciesError::BadMinorUnits {
        code: code.clone(),
        units: units.to_owned(),
    })?;
    if digits > 3 {
        return Ok(());
    }
    match currencies.insert(code.clone(), digits) {
        Some(first) if first != digits => Err(CurrenciesError::Conflict {
            code,
            first,
            second: digits,
        }),
        _ => Ok(()),
    }
}

/// Whether `text` is shaped `YYYY-MM-DD`.
fn is_iso_date(text: &str) -> bool {
    text.len() == 10
        && text.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// The table's Rust text, in rustfmt's layout so `cargo fmt --all --check` passes.
fn render(list: &List) -> String {
    let mut text = HEADER.replace("{published}", &list.published);
    for (code, digits) in &list.currencies {
        let name = match digits {
            0 => "ZERO",
            1 => "ONE",
            2 => "TWO",
            _ => "THREE",
        };
        text.push_str(&format!("    row(\"{code}\", MinorDigits::{name}),\n"));
    }
    text.push_str("];\n");
    text
}
