//! `docs/deploy.md`, the deployment guide (PLAN M2 slice 9, SPEC decisions 179 to 187 and 195),
//! read as text and held to the program it describes. The guide has two kinds of fenced block. A
//! block whose info word is `text` is an answer: its first line is the command after `$ `, and the
//! lines under it are consecutive printed lines of one committed snapshot of this crate (under
//! `tests/snapshots` or a `snapshots` folder under `src`), never a snapshot's own marker lines (its
//! header, its `$ ` line, `[stdout]`, `[stderr]`, `[exit] N`, `[stderr, up to the listening
//! line]`). A block whose info word is `log` holds single log lines, each a log line of some
//! snapshot. Lines are compared once the varying parts are made equal: `/data` for
//! `/srv/nage/data`, a version for `[version]`, a stamp for `[TIME]` and `[STAMP]`, an address and
//! port for `[BIND]`, a port for `[PORT]`, and a four-digit change number for a four-digit change
//! number. Every text of the program's sources that sends a reader to the deployment guide, to
//! whoever looks after Nage or to an admin has the guide's heading that answers it; the report
//! form, the warning to strike out names and the three proxies are there; the image the guide names
//! is this version's; `nage --help` names this file; and the guide names no address but
//! `127.0.0.1`, `0.0.0.0` and `nage.example`, and no zone but `Etc/UTC`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The workspace version, which the guide's image and addresses must carry.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The guide, relative to the repository's root.
const GUIDE: &str = "docs/deploy.md";
/// The example Compose file, relative to the repository's root.
const EXAMPLE: &str = "deploy/compose.example.yaml";
/// The report form's address, as the guide and the help give it (without the version).
const REPORT_FORM: &str = "https://github.com/bngoller/nage/issues/new?template=problem.yml";

/// Every text of the sources that sends a reader to the guide, to whoever looks after Nage or to
/// an admin, by a part of its words, and the guide's heading that answers it.
const PROMISES: &[(&str, &str)] = &[
    (
        "The deployment guide for this version:",
        "Nage deployment guide",
    ),
    (
        "the deployment guide says where to report them",
        "Report a problem",
    ),
    (
        "as the deployment guide shows",
        "Go back to the version before",
    ),
    ("whoever looks after Nage", "A check reports a broken rule"),
    ("ask an admin to reset it", "A forgotten password"),
];

/// The repository's root, two levels above this crate.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn guide() -> String {
    read(GUIDE)
}

/// A fenced block of the guide: its info word, the line of its opening fence (1-based) and its
/// lines.
struct Block {
    info: String,
    line: usize,
    lines: Vec<String>,
}

/// A heading of the guide outside every fenced block: its level, its text and its line (0-based).
struct Heading {
    level: usize,
    text: String,
    line: usize,
}

/// The guide's fenced blocks and headings, a fence being a line that begins with three backticks.
fn outline(text: &str) -> (Vec<Block>, Vec<Heading>) {
    let mut blocks = Vec::new();
    let mut headings = Vec::new();
    let mut open: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(fence) = line.strip_prefix("```") {
            match open.take() {
                Some(block) => blocks.push(block),
                None => {
                    open = Some(Block {
                        info: fence.trim().to_owned(),
                        line: index + 1,
                        lines: Vec::new(),
                    });
                }
            }
            continue;
        }
        if let Some(block) = open.as_mut() {
            block.lines.push(line.to_owned());
            continue;
        }
        let level = line.chars().take_while(|&c| c == '#').count();
        if level > 0
            && let Some(rest) = line[level..].strip_prefix(' ')
        {
            headings.push(Heading {
                level,
                text: rest.trim().to_owned(),
                line: index,
            });
        }
    }
    assert!(
        open.is_none(),
        "{GUIDE} has a fenced block that is never closed"
    );
    (blocks, headings)
}

/// The lines under the heading `name` up to the next heading of its level or above.
fn section(text: &str, name: &str) -> String {
    let (_, headings) = outline(text);
    let Some(position) = headings.iter().position(|h| h.text == name) else {
        panic!("{GUIDE} has no heading {name:?}");
    };
    let heading = &headings[position];
    let end = headings[position + 1..]
        .iter()
        .find(|h| h.level <= heading.level)
        .map_or(usize::MAX, |h| h.line);
    text.lines()
        .enumerate()
        .filter(|&(index, _)| index > heading.line && index < end)
        .map(|(_, line)| format!("{line}\n"))
        .collect()
}

/// Every `.snap` file of this crate: `tests/snapshots` and every `snapshots` folder under `src`.
fn snapshot_files() -> Vec<PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_snapshots(&crate_dir.join("tests/snapshots"), &mut files);
    collect_snapshots(&crate_dir.join("src"), &mut files);
    files.sort();
    files
}

fn collect_snapshots(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect_snapshots(&path, files);
        } else if path.extension().is_some_and(|e| e == "snap")
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "snapshots")
        {
            files.push(path);
        }
    }
}

/// True for a snapshot's own marker lines, which are never printed lines.
fn is_marker(line: &str) -> bool {
    line.starts_with("$ ")
        || line == "[stdout]"
        || line == "[stderr]"
        || (line.starts_with("[stderr,") && line.ends_with(']'))
        || line.starts_with("[exit] ")
}

/// A snapshot's printed lines in order: its header and its marker lines left out.
fn printed_lines(snapshot: &str) -> Vec<String> {
    let mut lines = snapshot.lines();
    let mut body: Vec<&str> = Vec::new();
    let mut first = lines.next();
    if first == Some("---") {
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
        }
        first = lines.next();
    }
    body.extend(first);
    body.extend(lines);
    body.into_iter()
        .filter(|line| !is_marker(line))
        .map(str::to_owned)
        .collect()
}

/// Every snapshot as its file name and its printed lines made comparable by [`canon`].
fn snapshots() -> Vec<(String, Vec<String>)> {
    let files = snapshot_files();
    assert!(!files.is_empty(), "this crate has no committed snapshot");
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                name,
                printed_lines(&text).iter().map(|l| canon(l)).collect(),
            )
        })
        .collect()
}

/// Every command of every snapshot with the printed lines under it, made comparable by
/// [`canon`]: the words after `nage` on its `$ ` line, the variables before `nage` left out.
fn snapshot_commands() -> Vec<(Vec<String>, Vec<String>)> {
    let mut commands = Vec::new();
    for path in snapshot_files() {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let mut lines = text.lines().peekable();
        if lines.peek() == Some(&"---") {
            lines.next();
            for line in lines.by_ref() {
                if line == "---" {
                    break;
                }
            }
        }
        let mut current: Option<(Vec<String>, Vec<String>)> = None;
        for line in lines {
            if let Some(command) = line.strip_prefix("$ ") {
                commands.extend(current.take());
                let words: Vec<String> = command
                    .split_whitespace()
                    .skip_while(|word| word.contains('='))
                    .skip(1)
                    .map(str::to_owned)
                    .collect();
                current = Some((words, Vec::new()));
            } else if !is_marker(line)
                && let Some((_, printed)) = current.as_mut()
            {
                printed.push(canon(line));
            }
        }
        commands.extend(current);
    }
    commands
}

/// The words of an answer's command after `docker compose run --rm nage`, `docker compose exec
/// nage nage` or `docker compose exec -T nage nage`: the command Nage runs.
fn nage_words(command: &str) -> Option<Vec<String>> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let prefixes: [&[&str]; 3] = [
        &["docker", "compose", "run", "--rm", "nage"],
        &["docker", "compose", "exec", "nage", "nage"],
        &["docker", "compose", "exec", "-T", "nage", "nage"],
    ];
    prefixes
        .iter()
        .find(|prefix| words.starts_with(prefix))
        .map(|prefix| {
            words[prefix.len()..]
                .iter()
                .map(|w| (*w).to_owned())
                .collect()
        })
}

/// True when the guide's command and a snapshot's are the same command: the same subcommand
/// words and the same flags with the same values (the word after a flag, unless it is a flag
/// itself), each value made comparable by [`canon`] as the answers are. The snapshot's
/// `--data-dir`, `--bind` and `--password-stdin` are left out with their values: a test gives them
/// and a container does not need them, and the values of the first two are a test's temporary
/// folder and address, which the guide's command cannot have.
fn same_command(guide: &[String], snapshot: &[String]) -> bool {
    let subcommand = |words: &[String]| -> Vec<String> {
        words
            .iter()
            .take_while(|word| !word.starts_with('-'))
            .cloned()
            .collect()
    };
    let flags = |words: &[String]| -> Vec<(String, Option<String>)> {
        let mut flags: Vec<(String, Option<String>)> = words
            .iter()
            .enumerate()
            .filter(|(_, word)| word.starts_with("--"))
            .filter(|(_, word)| {
                !["--data-dir", "--bind", "--password-stdin"].contains(&word.as_str())
            })
            .map(|(at, flag)| {
                let value = words
                    .get(at + 1)
                    .filter(|next| !next.starts_with('-'))
                    .map(|next| canon(next));
                (flag.clone(), value)
            })
            .collect();
        flags.sort();
        flags
    };
    subcommand(guide) == subcommand(snapshot) && flags(guide) == flags(snapshot)
}

fn digit(chars: &[char], index: usize) -> bool {
    chars.get(index).is_some_and(char::is_ascii_digit)
}

fn digits_at(chars: &[char], index: usize) -> usize {
    chars[index.min(chars.len())..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count()
}

/// The length of a stamp `YYYY-MM-DDTHH<sep>MM<sep>SSZ` at `index`, if there is one.
fn stamp_at(chars: &[char], index: usize, sep: char) -> Option<usize> {
    let shape = [
        'd', 'd', 'd', 'd', '-', 'd', 'd', '-', 'd', 'd', 'T', 'd', 'd', 's', 'd', 'd', 's', 'd',
        'd', 'Z',
    ];
    let fits = shape.iter().enumerate().all(|(offset, &want)| {
        let Some(&got) = chars.get(index + offset) else {
            return false;
        };
        match want {
            'd' => got.is_ascii_digit(),
            's' => got == sep,
            other => got == other,
        }
    });
    fits.then_some(shape.len())
}

/// The length of an IPv4 address with its port (digits or `[PORT]`) at `index`, if there is one.
fn bind_at(chars: &[char], index: usize) -> Option<usize> {
    let mut at = index;
    for part in 0..4 {
        let run = digits_at(chars, at);
        if run == 0 || run > 3 {
            return None;
        }
        at += run;
        let want = if part == 3 { ':' } else { '.' };
        if chars.get(at) != Some(&want) {
            return None;
        }
        at += 1;
    }
    let port: Vec<char> = "[PORT]".chars().collect();
    if chars.get(at..at + port.len()) == Some(&port[..]) {
        return Some(at + port.len() - index);
    }
    let run = digits_at(chars, at);
    (run > 0 && run <= 5).then_some(at + run - index)
}

/// The length of a version `N.N.N` at `index`, if there is one that is not part of a longer
/// dotted number.
fn version_at(chars: &[char], index: usize) -> Option<usize> {
    if index > 0 && chars[index - 1] == '.' {
        return None;
    }
    let mut at = index;
    for part in 0..3 {
        let run = digits_at(chars, at);
        if run == 0 {
            return None;
        }
        at += run;
        if part < 2 {
            if chars.get(at) != Some(&'.') {
                return None;
            }
            at += 1;
        }
    }
    if chars.get(at) == Some(&'.') && digit(chars, at + 1) {
        return None;
    }
    Some(at - index)
}

/// A line with its varying parts made equal: `/srv/nage/data` becomes `/data`; a log stamp
/// `[TIME]`, a backup stamp `[STAMP]`, an IPv4 address with its port `[BIND]`, this version
/// `[version]` (so that a snapshot's `[version]` meets this version alone), any other version
/// `[another version]`, a port after a colon `[PORT]` and a four-digit change number `[CHANGE]`.
/// Applied to both sides, so that a snapshot's placeholders and the guide's values meet.
fn canon(line: &str) -> String {
    let line = line.replace("/srv/nage/data", "/data");
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some(length) = stamp_at(&chars, index, ':') {
            out.push_str("[TIME]");
            index += length;
        } else if let Some(length) = stamp_at(&chars, index, '-') {
            out.push_str("[STAMP]");
            index += length;
        } else if chars[index].is_ascii_digit() {
            if let Some(length) = bind_at(&chars, index) {
                out.push_str("[BIND]");
                index += length;
            } else if let Some(length) = version_at(&chars, index) {
                let found: String = chars[index..index + length].iter().collect();
                out.push_str(if found == VERSION {
                    "[version]"
                } else {
                    "[another version]"
                });
                index += length;
            } else {
                let run = digits_at(&chars, index);
                if index > 0 && chars[index - 1] == ':' && run <= 5 {
                    out.push_str("[PORT]");
                } else if run == 4 {
                    out.push_str("[CHANGE]");
                } else {
                    out.extend(&chars[index..index + run]);
                }
                index += run;
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// True when `window` stands in `lines` as consecutive lines.
fn holds_run(lines: &[String], window: &[String]) -> bool {
    !window.is_empty() && lines.windows(window.len()).any(|w| w == window)
}

/// Every string literal of a Rust source, comments left out; a line continuation (a backslash
/// at the end of a line) joins the lines as the compiler does.
fn string_literals(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        if c == '/' && next == Some('/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0usize;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    index += 2;
                } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    index += 1;
                }
            }
        } else if c == 'r'
            && (index == 0 || !(chars[index - 1].is_alphanumeric() || chars[index - 1] == '_'))
            && matches!(next, Some('#' | '"'))
        {
            let mut at = index + 1;
            let mut hashes = 0;
            while chars.get(at) == Some(&'#') {
                hashes += 1;
                at += 1;
            }
            if chars.get(at) != Some(&'"') {
                index += 1;
                continue;
            }
            at += 1;
            let start = at;
            loop {
                if at >= chars.len() {
                    break;
                }
                if chars[at] == '"'
                    && (1..=hashes).all(|offset| chars.get(at + offset) == Some(&'#'))
                {
                    break;
                }
                at += 1;
            }
            literals.push(chars[start..at.min(chars.len())].iter().collect());
            index = at + 1 + hashes;
        } else if c == '"' {
            let mut literal = String::new();
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if chars[index] == '\\' {
                    match chars.get(index + 1) {
                        Some('\n') => {
                            index += 2;
                            while chars.get(index).is_some_and(|c| c.is_whitespace()) {
                                index += 1;
                            }
                            continue;
                        }
                        Some('n') => literal.push('\n'),
                        Some(&other) => literal.push(other),
                        None => {}
                    }
                    index += 2;
                } else {
                    literal.push(chars[index]);
                    index += 1;
                }
            }
            literals.push(literal);
            index += 1;
        } else if c == '\'' {
            if next == Some('\\') {
                let mut at = index + 3;
                while at < chars.len() && chars[at] != '\'' {
                    at += 1;
                }
                index = at + 1;
            } else if chars.get(index + 2) == Some(&'\'') {
                index += 3;
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    literals
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

/// Every file under `dir`, at any depth.
fn all_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            all_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// Every text of a page template: each run between its angle brackets, the words of the page and
/// the insides of its tags alike (an attribute can carry a sentence), whitespace collapsed to one
/// space, so that a sentence wrapped over several lines is one text.
fn template_texts(source: &str) -> Vec<String> {
    source
        .split(['<', '>'])
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .collect()
}

/// True when the text sends a reader to the guide, to whoever looks after Nage or to an admin.
fn is_promise(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "deployment guide",
        "whoever looks after nage",
        "ask an admin to reset it",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Every IPv4 address of a text, as written.
fn ipv4_addresses(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() && (index == 0 || !chars[index - 1].is_ascii_digit()) {
            let mut at = index;
            let mut parts = 0;
            loop {
                let run = digits_at(&chars, at);
                if run == 0 || run > 3 {
                    break;
                }
                at += run;
                parts += 1;
                if parts == 4 || chars.get(at) != Some(&'.') || !digit(&chars, at + 1) {
                    break;
                }
                at += 1;
            }
            if parts == 4 && !digit(&chars, at) {
                found.push(chars[index..at].iter().collect());
            }
            index = at.max(index + 1);
        } else {
            index += 1;
        }
    }
    found
}

/// Every web address of a text (`http://` or `https://` up to a space, a backtick, a quote, a
/// parenthesis or an angle bracket).
fn web_addresses(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for scheme in ["http://", "https://"] {
        let mut rest = text;
        while let Some(at) = rest.find(scheme) {
            let tail = &rest[at..];
            let end = tail
                .find(|c: char| c.is_whitespace() || "`\"'()<>".contains(c))
                .unwrap_or(tail.len());
            found.push(tail[..end].to_owned());
            rest = &tail[scheme.len()..];
        }
    }
    found
}

/// Every name of the shape `Area/City` in a text: a capital letter and letters, a slash, a capital
/// letter and letters or underscores, not inside a longer word.
fn zone_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (slash, &c) in chars.iter().enumerate() {
        if c != '/' || slash == 0 {
            continue;
        }
        let mut start = slash;
        while start > 0 && chars[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        let mut end = slash + 1;
        while end < chars.len() && (chars[end].is_ascii_alphabetic() || chars[end] == '_') {
            end += 1;
        }
        let area_ok = start < slash && chars[start].is_ascii_uppercase();
        let city_ok = end > slash + 2 && chars[slash + 1].is_ascii_uppercase();
        if area_ok && city_ok {
            found.push(chars[start..end].iter().collect());
        }
    }
    found
}

/// The time zones of the tz database whose names are not of the shape Area/City (UTC aside, which
/// the guide uses to say how times are written). No list of abbreviations stands beside it: such a
/// list would name the zone of the machine that wrote it, or give it away by leaving it out. A
/// value given to a zone setting is held to `Etc/UTC` by [`zone_values`] instead.
const OTHER_ZONES: &[&str] = &[
    "CST6CDT",
    "Cuba",
    "EST5EDT",
    "Egypt",
    "Eire",
    "Factory",
    "GB",
    "GB-Eire",
    "GMT+0",
    "GMT-0",
    "GMT0",
    "Greenwich",
    "Hongkong",
    "Iceland",
    "Iran",
    "Israel",
    "Jamaica",
    "Japan",
    "Kwajalein",
    "Libya",
    "MST7MDT",
    "NZ",
    "NZ-CHAT",
    "Navajo",
    "PRC",
    "PST8PDT",
    "Poland",
    "Portugal",
    "ROC",
    "ROK",
    "Singapore",
    "Turkey",
    "Universal",
    "W-SU",
    "Zulu",
];

/// Every value a text gives a zone: the word after `TZ:`, `TZ=`, `NAGE_TZ:`, `NAGE_TZ=` or the
/// log's `tz=`, without its quotes and a full stop after it. `tz_source=` and a setting named in a
/// sentence, such as the settings table's "not set" or "UTC" beside `TZ`, give no value.
fn zone_values(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (at, &sign) in chars.iter().enumerate() {
        if sign != ':' && sign != '=' {
            continue;
        }
        let mut start = at;
        while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        let key: String = chars[start..at].iter().collect();
        if !(key == "TZ" || key == "NAGE_TZ" || (key == "tz" && sign == '=')) {
            continue;
        }
        let value: String = chars[at + 1..]
            .iter()
            .skip_while(|c| **c == ' ' || **c == '\t')
            .take_while(|c| !c.is_whitespace() && !"`,;()".contains(**c))
            .collect();
        found.push(
            value
                .trim_end_matches('.')
                .trim_matches(['"', '\''])
                .to_owned(),
        );
    }
    found
}

/// The first cells of a look-up table's rows, header and rule left out, backticks removed.
fn first_cells(section: &str) -> Vec<String> {
    section
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .filter_map(|line| line.split('|').nth(1))
        .map(|cell| cell.replace('`', "").trim().to_owned())
        .collect()
}

/// The guide's first line is the heading `# Nage deployment guide`, and the guide is one
/// Markdown file at `docs/deploy.md`.
///
/// Proves: SPEC decision 195: docs/deploy.md is one Markdown file whose first line is the heading
/// Nage deployment guide
#[test]
fn the_guide_is_titled_nage_deployment_guide() {
    let text = guide();
    assert_eq!(
        text.lines().next(),
        Some("# Nage deployment guide"),
        "the first line of {GUIDE}"
    );
    let (_, headings) = outline(&text);
    let titles: Vec<&Heading> = headings.iter().filter(|h| h.level == 1).collect();
    assert_eq!(titles.len(), 1, "{GUIDE} must have exactly one title");
}

/// Every `text` block of the guide is a command after `$ ` and then consecutive printed lines of
/// one committed snapshot.
///
/// Proves: SPEC decision 180: every block of the guide whose info word is text begins with its
/// command after a dollar sign and a space, and the lines under it are consecutive printed lines of
/// one committed snapshot of the nage crate, never its marker lines, once /data, a version, a
/// stamp, an address and port, a port and a four-digit change number stand for /srv/nage/data,
/// [version], [TIME] or [STAMP], [BIND], [PORT] and a four-digit change number; the guide holds at
/// least one such block
#[test]
fn every_answer_the_guide_shows_is_a_run_of_lines_of_one_snapshot() {
    let (blocks, _) = outline(&guide());
    let snapshots = snapshots();
    let commands = snapshot_commands();
    let answers: Vec<&Block> = blocks.iter().filter(|b| b.info == "text").collect();
    assert!(
        !answers.is_empty(),
        "{GUIDE} shows no answer (a text block)"
    );
    let mut problems = Vec::new();
    for block in answers {
        let Some(command) = block.lines.first() else {
            problems.push(format!("line {}: an empty answer", block.line));
            continue;
        };
        if !command.starts_with("$ ") || command.len() <= 2 {
            problems.push(format!(
                "line {}: the answer does not begin with its command after \"$ \": {command:?}",
                block.line
            ));
            continue;
        }
        let shown: Vec<String> = block.lines[1..].iter().map(|l| canon(l)).collect();
        if shown.is_empty() {
            problems.push(format!("line {}: {command:?} shows no line", block.line));
            continue;
        }
        if !snapshots.iter().any(|(_, lines)| holds_run(lines, &shown)) {
            problems.push(format!(
                "line {}: the lines under {command:?} are no run of printed lines of one snapshot:\n    {}",
                block.line,
                shown.join("\n    ")
            ));
            continue;
        }
        // A command Nage runs is the command of the snapshot that printed the lines; a log is
        // read with docker compose logs, under whatever command started the server.
        let run = &command[2..];
        if run.starts_with("docker compose logs ") {
            continue;
        }
        match nage_words(run) {
            Some(words) => {
                if !commands
                    .iter()
                    .any(|(ran, lines)| same_command(&words, ran) && holds_run(lines, &shown))
                {
                    problems.push(format!(
                        "line {}: no snapshot of the command {command:?} printed the lines under it",
                        block.line
                    ));
                }
            }
            None => problems.push(format!(
                "line {}: {command:?} is neither docker compose logs nor a Nage command run through docker compose",
                block.line
            )),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // A flag's value counts: the set-up's answer under its own command, and never under the same
    // command with another currency than the one it printed.
    let answered = |command: &str, shown: &[String]| {
        let words = nage_words(command).expect("a Nage command run through docker compose");
        commands
            .iter()
            .any(|(ran, lines)| same_command(&words, ran) && holds_run(lines, shown))
    };
    let shown: Vec<String> = [
        "Success: The book \"Household\" was set up.",
        "  Currency: CAD",
    ]
    .iter()
    .map(|line| canon(line))
    .collect();
    let set_up = "docker compose run --rm nage init --book-name Household --currency CAD --username sam --display-name Sam";
    assert!(
        answered(set_up, &shown),
        "no snapshot of {set_up:?} printed {shown:?}"
    );
    let wrong = set_up.replace("--currency CAD", "--currency EUR");
    assert!(
        !answered(&wrong, &shown),
        "{wrong:?} is taken for the command that printed {shown:?}"
    );
}

/// Every line of a `log` block of the guide is a log line of some committed snapshot.
///
/// Proves: SPEC decision 180: every line of every block of the guide whose info word is log equals
/// a log line of some committed snapshot of the nage crate by the same rule, and the guide holds at
/// least one such block
#[test]
fn every_log_line_the_guide_shows_is_a_line_nage_logs() {
    let (blocks, _) = outline(&guide());
    let logged: Vec<String> = snapshots()
        .into_iter()
        .flat_map(|(_, lines)| lines)
        .filter(|line| line.starts_with("[TIME] "))
        .collect();
    let logs: Vec<&Block> = blocks.iter().filter(|b| b.info == "log").collect();
    assert!(!logs.is_empty(), "{GUIDE} has no log block");
    let mut problems = Vec::new();
    for block in logs {
        if block.lines.is_empty() {
            problems.push(format!("line {}: an empty log block", block.line));
        }
        for (offset, line) in block.lines.iter().enumerate() {
            let shown = canon(line);
            if !shown.starts_with("[TIME] ") || !logged.contains(&shown) {
                problems.push(format!(
                    "line {}: no snapshot logs this line: {line}",
                    block.line + 1 + offset
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every text of the three crates' sources, and of nage-web's page templates, that sends a reader
/// to the guide, to whoever looks after Nage or to an admin has a line in [`PROMISES`], and the
/// guide has every heading it names.
///
/// Proves: SPEC decision 180: every string literal in the src folders of nage, nage-core and
/// nage-web that names the deployment guide, or that says whoever looks after Nage or ask an admin
/// to reset it, has a line in the test's table naming the guide's heading that answers it, and the
/// guide has every heading the table names; a text with no line, or a heading the guide lacks,
/// fails
#[test]
fn every_promise_of_the_programs_messages_has_its_heading() {
    let crates = root().join("crates");
    let mut files = Vec::new();
    for name in ["nage", "nage-core", "nage-web"] {
        rust_files(&crates.join(name).join("src"), &mut files);
    }
    // The page templates too, by the same rule: a page that sends its reader to the guide.
    let templates = crates.join("nage-web/templates");
    let mut pages = Vec::new();
    all_files(&templates, &mut pages);
    assert!(
        !pages.is_empty(),
        "{} holds no template",
        templates.display()
    );
    files.sort();
    pages.sort();
    let mut problems = Vec::new();
    let mut used = vec![false; PROMISES.len()];
    let mut found = 0;
    let texts = files
        .iter()
        .map(|path| (path, true))
        .chain(pages.iter().map(|path| (path, false)));
    for (path, rust) in texts {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let texts = if rust {
            string_literals(&source)
        } else {
            template_texts(&source)
        };
        for literal in texts.iter().filter(|l| is_promise(l)) {
            found += 1;
            let lower = literal.to_lowercase();
            match PROMISES
                .iter()
                .position(|(words, _)| lower.contains(&words.to_lowercase()))
            {
                Some(at) => used[at] = true,
                None => problems.push(format!(
                    "{}: no line of the table answers {literal:?}",
                    path.display()
                )),
            }
        }
    }
    assert!(found > 0, "no text of the sources sends a reader anywhere");
    let (_, headings) = outline(&guide());
    for ((words, heading), used) in PROMISES.iter().zip(used) {
        if !headings.iter().any(|h| h.text == *heading) {
            problems.push(format!(
                "{GUIDE} has no heading {heading:?}, which answers {words:?}"
            ));
        }
        if !used {
            problems.push(format!("no text of the sources says {words:?} any more"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Report a problem gives the report form and the warning to strike out names first.
///
/// Proves: SPEC decision 184: the section Report a problem names the report form's address and
/// tells the reader to strike out names first, since what is written there is public
#[test]
fn the_report_section_gives_the_form_and_the_warning_to_strike_out_names() {
    let report = section(&guide(), "Report a problem");
    assert!(
        report.contains(REPORT_FORM),
        "Report a problem does not name the form {REPORT_FORM}:\n{report}"
    );
    let lower = report.to_lowercase();
    assert!(
        lower.contains("strike out names first"),
        "Report a problem does not say to strike out names first:\n{report}"
    );
    assert!(
        lower.contains("public"),
        "Report a problem does not say that the form is public:\n{report}"
    );
}

/// Report a problem sends a question or an idea to the Discussions board and a security problem
/// to the private report form.
///
/// Proves: SPEC decisions 184 and 185: the section Report a problem sends a question or an idea to
/// the Discussions board and a security problem to Report a vulnerability on the Security page
#[test]
fn questions_and_security_problems_are_sent_to_their_own_places() {
    let report = section(&guide(), "Report a problem");
    let question = report
        .lines()
        .find(|line| line.contains("A question or an idea"))
        .unwrap_or_else(|| panic!("Report a problem has no line for a question:\n{report}"));
    assert!(
        question.contains("Discussions")
            && question.contains("https://github.com/bngoller/nage/discussions"),
        "a question is not sent to the Discussions board: {question}"
    );
    let at = report.find("A security problem").unwrap_or_else(|| {
        panic!("Report a problem has no line for a security problem:\n{report}")
    });
    let security = &report[at..];
    assert!(
        security.contains("\"Report a vulnerability\"")
            && security.contains("Security page")
            && security.contains("https://github.com/bngoller/nage/security/advisories/new"),
        "a security problem is not sent to Report a vulnerability:\n{security}"
    );
}

/// Tailscale Serve, Caddy and nginx each have a heading with the lines they need.
///
/// Proves: SPEC decision 182: the guide has a heading each for Tailscale Serve, Caddy and nginx,
/// with the tailscale serve command, Caddy's reverse_proxy, and nginx's proxy_set_header lines for
/// Host, X-Forwarded-Host and X-Forwarded-Proto
#[test]
fn the_three_proxies_are_given_with_the_lines_they_need() {
    let text = guide();
    let tailscale = section(&text, "Tailscale Serve");
    assert!(
        tailscale
            .lines()
            .any(|l| l.contains("tailscale serve --bg http://127.0.0.1:8787")),
        "Tailscale Serve has no tailscale serve command:\n{tailscale}"
    );
    let caddy = section(&text, "Caddy");
    assert!(
        caddy
            .lines()
            .any(|l| l.trim() == "reverse_proxy 127.0.0.1:8787"),
        "Caddy has no reverse_proxy line:\n{caddy}"
    );
    let nginx = section(&text, "nginx");
    for header in ["Host", "X-Forwarded-Host", "X-Forwarded-Proto"] {
        let wanted = format!("proxy_set_header {header} ");
        assert!(
            nginx.lines().any(|l| l.trim().starts_with(&wanted)),
            "nginx has no proxy_set_header line for {header}:\n{nginx}"
        );
    }
    // Each line as nginx needs it: the port Nage listens on, the browser's own host, and the
    // scheme the visitor used.
    for wanted in [
        "proxy_pass http://127.0.0.1:8787;",
        "proxy_set_header Host $http_host;",
        "proxy_set_header X-Forwarded-Host $http_host;",
        "proxy_set_header X-Forwarded-Proto $scheme;",
    ] {
        assert!(
            nginx.lines().any(|l| l.trim() == wanted),
            "nginx has no line {wanted:?}:\n{nginx}"
        );
    }
}

/// Every image and every repository address at a tag the guide names is this version's.
///
/// Proves: SPEC decisions 165 and 190: every ghcr.io/bngoller/nage image the guide names carries
/// this version, and every repository address naming a tag names v and this version
#[test]
fn the_image_and_the_addresses_the_guide_names_are_this_versions() {
    let text = guide();
    let image = "ghcr.io/bngoller/nage";
    let mut images = 0;
    let mut problems = Vec::new();
    for (at, _) in text.match_indices(image) {
        images += 1;
        let tail = &text[at + image.len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || ":.-_".contains(*c))
            .collect();
        let name = name.trim_end_matches('.');
        if name != format!(":{VERSION}") {
            problems.push(format!("the image {image}{name} is not {image}:{VERSION}"));
        }
    }
    assert!(images > 0, "{GUIDE} names no image");
    let mut tagged = 0;
    for address in web_addresses(&text) {
        let Some(path) = address.strip_prefix("https://github.com/bngoller/nage/") else {
            continue;
        };
        let parts: Vec<&str> = path.split(['/', '?', '#']).collect();
        let tag = match parts.as_slice() {
            ["blob" | "raw" | "tree", tag, ..] => Some(*tag),
            ["releases", "tag", tag, ..] => Some(*tag),
            _ => None,
        };
        if let Some(tag) = tag {
            tagged += 1;
            if tag != format!("v{VERSION}") {
                problems.push(format!("{address} names {tag}, not v{VERSION}"));
            }
        }
    }
    assert!(tagged > 0, "{GUIDE} names no repository address at a tag");
    // A file downloaded from the repository is the example of this version, never another file.
    let mut downloads = 0;
    for address in web_addresses(&text) {
        if address.starts_with("https://github.com/bngoller/nage/raw/") {
            downloads += 1;
            let wanted = format!("https://github.com/bngoller/nage/raw/v{VERSION}/{EXAMPLE}");
            if address != wanted {
                problems.push(format!("{address} is not {wanted}"));
            }
        }
    }
    assert!(downloads > 0, "{GUIDE} downloads no example file");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `nage --help` gives this guide's address at this version's tag.
///
/// Proves: SPEC decision 183: the guide's address in nage --help ends with the path of this file
/// in the repository, docs/deploy.md, at v and this version
#[test]
fn the_help_names_this_guide() {
    let out = Command::new(env!("CARGO_BIN_EXE_nage"))
        .arg("--help")
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .expect("nage --help runs");
    assert!(out.status.success(), "nage --help exited {}", out.status);
    let help = String::from_utf8(out.stdout).expect("the help is UTF-8");
    let mut lines = help.lines();
    lines
        .by_ref()
        .find(|line| *line == "The deployment guide for this version:")
        .unwrap_or_else(|| panic!("nage --help names no deployment guide:\n{help}"));
    let address = lines.next().map(str::trim).unwrap_or_default();
    let wanted = format!("/blob/v{VERSION}/{GUIDE}");
    assert!(
        address.starts_with("https://github.com/bngoller/nage/") && address.ends_with(&wanted),
        "nage --help gives the guide as {address:?}, not at {wanted:?}"
    );
    assert!(root().join(GUIDE).is_file(), "{GUIDE} is not a file");
}

/// The guide names no IPv4 address but 127.0.0.1 and 0.0.0.0, no web host but github.com,
/// nage.example and those two, and no tailnet.
///
/// Proves: SPEC decision 195: every IPv4 address the guide holds is 127.0.0.1 or 0.0.0.0, every web
/// address is on github.com, nage.example or those two, and no ts.net name appears
#[test]
fn the_guide_names_no_address_but_loopback_any_and_nage_example() {
    let text = guide();
    let mut problems = Vec::new();
    for address in ipv4_addresses(&text) {
        if address != "127.0.0.1" && address != "0.0.0.0" {
            problems.push(format!("the IPv4 address {address}"));
        }
    }
    let addresses = web_addresses(&text);
    assert!(!addresses.is_empty(), "{GUIDE} names no web address");
    for address in addresses {
        let rest = address.split_once("://").map_or("", |(_, rest)| rest);
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default();
        if !["github.com", "nage.example", "127.0.0.1", "0.0.0.0"].contains(&host) {
            problems.push(format!("the web address {address}"));
        }
    }
    if text.to_lowercase().contains("ts.net") {
        problems.push("a ts.net name".to_owned());
    }
    if text.to_lowercase().contains("www.") {
        problems.push("a www. name".to_owned());
    }
    // A host name written without a scheme, inside or outside a fenced block: every dotted word
    // whose last part begins with a letter is a host, unless it is a file name.
    for host in host_names(&text) {
        if !["nage.example", "github.com", "ghcr.io"].contains(&host.as_str()) {
            problems.push(format!("the host name {host}"));
        }
    }
    for address in ipv6_addresses(&text) {
        problems.push(format!("the IPv6 address {address}"));
    }
    assert!(problems.is_empty(), "{GUIDE} names {}", problems.join(", "));
    // A file name and an abbreviation are no host names; a home network's name still is.
    let files = "Edit nginx.conf, unpack nage-0.2.0-linux-x86_64.tar.gz, e.g. here, i.e. there.";
    assert_eq!(
        host_names(files),
        Vec::<String>::new(),
        "file names taken for host names in {files:?}"
    );
    for home in ["nas.fritz.box", "nas.lan"] {
        let line = format!("proxy_pass http://{home}:8787; # or {home} alone");
        assert!(
            host_names(&line).iter().any(|host| host == home),
            "the home network's name {home} is not taken for a host name in {line:?}"
        );
    }
}

/// The endings of the file names the guide may write, which are no host names.
const FILE_ENDINGS: &[&str] = &[
    "conf",
    "gz",
    "tgz",
    "json",
    "sh",
    "toml",
    "lock",
    "md",
    "txt",
    "yml",
    "yaml",
    "html",
    "rs",
    "sqlite",
    "sqlite-wal",
    "sqlite-shm",
];

/// Abbreviations written with dots, which are no host names.
const ABBREVIATIONS: &[&str] = &["e.g", "i.e"];

/// Every dotted word of a text whose last part begins with a letter and is no file ending, and
/// that is no abbreviation such as "e.g.": a word of letters, digits, `-` and `_`, with at least
/// two parts and none empty.
fn host_names(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || ".-_".contains(c)))
        .map(|word| word.trim_matches(['.', '-']))
        .filter(|word| {
            let labels: Vec<&str> = word.split('.').collect();
            let last = labels.last().copied().unwrap_or_default();
            labels.len() >= 2
                && labels.iter().all(|label| !label.is_empty())
                && last.starts_with(|c: char| c.is_ascii_alphabetic())
                && !FILE_ENDINGS.contains(&last.to_ascii_lowercase().as_str())
                && !ABBREVIATIONS.iter().any(|a| a.eq_ignore_ascii_case(word))
        })
        .map(str::to_owned)
        .collect()
}

/// Every IPv6 address of a text but `::1`: a run of hexadecimal digits and colons that holds
/// `::` or seven colons.
fn ipv6_addresses(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_hexdigit() || c == ':'))
        .filter(|run| (run.contains("::") || run.matches(':').count() >= 7) && *run != "::1")
        .map(str::to_owned)
        .collect()
}

/// Wherever the guide writes `NAGE_TRUST_PROXY:`, the value is the quoted `"true"`.
///
/// Proves: SPEC decision 191: wherever the guide writes NAGE_TRUST_PROXY with a colon, its value is
/// the quoted true
#[test]
fn the_guide_keeps_the_proxy_setting_quoted() {
    let text = guide();
    let key = "NAGE_TRUST_PROXY:";
    let mut seen = 0;
    for (at, _) in text.match_indices(key) {
        seen += 1;
        let value = text[at + key.len()..].trim_start_matches(' ');
        assert!(
            value.starts_with("\"true\""),
            "{GUIDE} writes {key} without the quoted \"true\": {}",
            value.lines().next().unwrap_or_default()
        );
    }
    assert!(seen > 0, "{GUIDE} never writes {key}");
    // The advice for a refused form names the field the quoted "true" gives.
    let refused = section(&text, "When a form is refused");
    assert!(
        refused.contains("`trust_proxy=true`") && !refused.contains("trust_proxy=false"),
        "When a form is refused does not tell the reader to look for trust_proxy=true:\n{refused}"
    );
}

/// The look-up table Settings has a row for each setting Nage reads.
///
/// Proves: SPEC decision 179: the look-up table Settings has a row for each of NAGE_DATA_DIR,
/// NAGE_BIND, NAGE_TRUST_PROXY, TZ, NAGE_TZ, NAGE_LOG and NO_COLOR
#[test]
fn the_settings_table_names_every_setting() {
    let settings = section(&guide(), "Settings");
    let rows = first_cells(&settings);
    for setting in [
        "NAGE_DATA_DIR",
        "NAGE_BIND",
        "NAGE_TRUST_PROXY",
        "TZ",
        "NAGE_TZ",
        "NAGE_LOG",
        "NO_COLOR",
    ] {
        assert!(
            rows.iter().any(|row| row == setting),
            "Settings has no row for {setting}: {rows:?}"
        );
    }
    // The column In the image says what the Dockerfile sets, or "not set".
    for cells in table_rows(&settings) {
        let (Some(setting), Some(in_image)) = (cells.first(), cells.get(1)) else {
            panic!("a row of Settings has fewer than two cells: {cells:?}");
        };
        let wanted = dockerfile_env(setting).unwrap_or_else(|| "not set".to_owned());
        assert_eq!(
            in_image, &wanted,
            "Settings says {setting} is {in_image:?} in the image, the Dockerfile {wanted:?}"
        );
    }
}

/// The command lines of the `sh` blocks of a text that run sqlite3, as another account than uid
/// 1000: a line whose program, after `sudo` and its options, is sqlite3, or is setpriv running
/// sqlite3, and that is neither `sudo -u '#1000' sqlite3 ...` nor `sudo setpriv --reuid 1000
/// --regid 1000 --clear-groups sqlite3 ...`. Prose that names the tool is no command line.
fn sqlite3_runs_not_as_uid_1000(text: &str) -> Vec<String> {
    let (blocks, _) = outline(text);
    blocks
        .iter()
        .filter(|block| block.info == "sh")
        .flat_map(|block| block.lines.iter())
        .map(|line| line.trim())
        .filter(|line| match runs_program(line) {
            Some("sqlite3") => true,
            Some("setpriv") => line.split_whitespace().any(|word| word == "sqlite3"),
            _ => false,
        })
        .filter(|line| {
            !line.starts_with("sudo -u '#1000' sqlite3 ")
                && !line
                    .starts_with("sudo setpriv --reuid 1000 --regid 1000 --clear-groups sqlite3 ")
        })
        .map(str::to_owned)
        .collect()
}

/// The program a command line runs: its first word, or after `sudo` the first word that is
/// neither one of sudo's options nor the value of one.
fn runs_program(line: &str) -> Option<&str> {
    /// sudo's options that take the next word as their value.
    const WITH_VALUE: &[&str] = &[
        "-u",
        "-g",
        "-C",
        "-D",
        "-h",
        "-p",
        "-r",
        "-t",
        "-T",
        "-U",
        "--user",
        "--group",
        "--close-from",
        "--chdir",
        "--host",
        "--prompt",
        "--role",
        "--type",
        "--command-timeout",
        "--other-user",
    ];
    let mut words = line.split_whitespace();
    let first = words.next()?;
    if first != "sudo" {
        return Some(first);
    }
    while let Some(word) = words.next() {
        if WITH_VALUE.contains(&word) {
            words.next();
        } else if word == "--" {
            return words.next();
        } else if !word.starts_with('-') {
            return Some(word);
        }
    }
    None
}

/// The cells of a look-up table's rows, header and rule left out, backticks removed.
fn table_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
            inner
                .split('|')
                .map(|cell| cell.replace('`', "").trim().to_owned())
                .collect()
        })
        .collect()
}

/// The value the Dockerfile's `ENV` gives `name`, if it gives one.
fn dockerfile_env(name: &str) -> Option<String> {
    let key = format!("{name}=");
    read("Dockerfile")
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .find_map(|word| word.strip_prefix(key.as_str()).map(str::to_owned))
}

/// The look-up table Commands has a row for each command.
///
/// Proves: SPEC decision 179: the look-up table Commands has a row for each of serve, health, init,
/// user add, user reset-password, check, migrate and migrate --dry-run
#[test]
fn the_commands_table_names_every_command() {
    // A row's command: its words without the placeholders and the flags that take one.
    let rows: Vec<String> = first_cells(&section(&guide(), "Commands"))
        .iter()
        .map(|cell| {
            let words: Vec<&str> = cell.split_whitespace().collect();
            words
                .iter()
                .enumerate()
                .filter(|&(at, word)| {
                    !word.starts_with('<')
                        && !(word.starts_with("--")
                            && words.get(at + 1).is_some_and(|n| n.starts_with('<')))
                })
                .map(|(_, word)| *word)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    for command in [
        "serve",
        "health",
        "init",
        "user add",
        "user reset-password",
        "check",
        "migrate",
        "migrate --dry-run",
    ] {
        assert!(
            rows.iter().any(|row| row == command),
            "Commands has no row for {command}: {rows:?}"
        );
    }
    // Every flag a row names is a flag of that command, as its own help lists it.
    for cell in first_cells(&section(&guide(), "Commands")) {
        let words: Vec<&str> = cell
            .split_whitespace()
            .map(|word| word.trim_end_matches(','))
            .collect();
        let flags: Vec<&str> = words
            .iter()
            .copied()
            .filter(|word| word.starts_with("--"))
            .collect();
        if flags.is_empty() {
            continue;
        }
        let subcommand: Vec<&str> = words
            .iter()
            .copied()
            .take_while(|word| !word.starts_with('-') && !word.starts_with('<'))
            .collect();
        let out = Command::new(env!("CARGO_BIN_EXE_nage"))
            .args(&subcommand)
            .arg("--help")
            .env_clear()
            .stdin(Stdio::null())
            .output()
            .expect("nage --help runs");
        assert!(
            out.status.success(),
            "nage {} --help exited {}",
            subcommand.join(" "),
            out.status
        );
        let help = String::from_utf8(out.stdout).expect("the help is UTF-8");
        for flag in flags {
            assert!(
                help.split_whitespace()
                    .any(|word| word.trim_end_matches(',') == flag),
                "the row {cell:?} names {flag}, which nage {} --help does not list:\n{help}",
                subcommand.join(" ")
            );
        }
    }
}

/// The backup and the way back keep their cautions.
///
/// Proves: SPEC decisions 186 and 187: the guide copies with sqlite3 .backup run as uid 1000, warns
/// never to copy nage.sqlite alone while Nage runs, moves the -wal and -shm files aside before a
/// restore, and keeps the newest two copies
#[test]
fn the_backup_and_the_way_back_keep_their_cautions() {
    let text = guide();
    let backup = section(&text, "Back up");
    assert!(
        backup
            .lines()
            .any(|l| l.starts_with("sudo -u '#1000' sqlite3 ") && l.contains(".backup ")),
        "Back up does not copy with sqlite3 .backup as uid 1000:\n{backup}"
    );
    assert!(
        backup
            .replace('`', "")
            .to_lowercase()
            .contains("never copy nage.sqlite alone while nage runs"),
        "Back up does not warn never to copy nage.sqlite alone while Nage runs:\n{backup}"
    );
    let back = section(&text, "Go back to the version before");
    let aside = back
        .lines()
        .position(|l| {
            l.contains(" mv ") && l.contains("nage.sqlite-wal") && l.contains("nage.sqlite-shm")
        })
        .unwrap_or_else(|| {
            panic!("going back does not move the -wal and -shm files aside:\n{back}")
        });
    let restore = back
        .lines()
        .position(|l| l.contains(" cp ") && l.ends_with(" data/nage.sqlite"))
        .unwrap_or_else(|| panic!("going back does not restore a copy:\n{back}"));
    assert!(
        aside < restore,
        "going back restores the copy before it moves the -wal and -shm files aside:\n{back}"
    );
    let stop = back
        .lines()
        .position(|l| l == "docker compose stop")
        .unwrap_or_else(|| panic!("going back does not stop the server first:\n{back}"));
    assert!(
        stop < aside,
        "going back moves the files aside before it stops the server:\n{back}"
    );
    let owner = back
        .lines()
        .position(|l| l == "sudo chown 1000:1000 data/nage.sqlite")
        .unwrap_or_else(|| panic!("going back does not hand the copy to uid 1000:\n{back}"));
    assert!(
        restore < owner,
        "going back hands nage.sqlite to uid 1000 before it restores the copy:\n{back}"
    );
    // Every sqlite3 the guide runs, runs as uid 1000.
    let others = sqlite3_runs_not_as_uid_1000(&text);
    assert!(
        others.is_empty(),
        "{GUIDE} runs sqlite3 as another account than uid 1000: {others:?}"
    );
    // Only a command line that runs sqlite3 is judged: prose naming the tool is none, while a run
    // as another account fails, with sudo or without it.
    let prose = "- `sqlite3`, for backups.\n\nCopy with sqlite3 while Nage runs.\n";
    assert_eq!(
        sqlite3_runs_not_as_uid_1000(prose),
        Vec::<String>::new(),
        "prose naming sqlite3 is taken for a run of it"
    );
    for run in [
        "sudo -u root sqlite3 data/nage.sqlite \".backup 'copies/x.sqlite'\"",
        "sudo sqlite3 data/nage.sqlite \".backup 'copies/x.sqlite'\"",
        "sqlite3 data/nage.sqlite \".backup 'copies/x.sqlite'\"",
    ] {
        let block = format!("Copy it:\n\n```sh\n{run}\n```\n");
        assert_eq!(
            sqlite3_runs_not_as_uid_1000(&block),
            vec![run.to_owned()],
            "a run of sqlite3 as another account than uid 1000 passes"
        );
    }
    assert!(
        text.to_lowercase().contains("keep the newest two copies"),
        "{GUIDE} does not say to keep the newest two copies"
    );
}

/// Neither the guide nor the example names a zone but Etc/UTC, and both name the command that
/// lists every zone. Every value either gives `TZ`, `NAGE_TZ` or the log's `tz=` is exactly
/// `Etc/UTC`, a positive rule: no rule about the shape of a word can tell an abbreviation of a
/// zone from a word such as GET or PORT, and no list of abbreviations is kept here, since it would
/// name or give away the zone of the machine that wrote it. That zone itself is held out of every
/// changed file by the private scans of the slice's gate, which know it.
///
/// Proves: SPEC decision 191: every name of the shape Area/City (a capital letter and letters, a
/// slash, a capital letter and letters or underscores) in docs/deploy.md and
/// deploy/compose.example.yaml is Etc/UTC or the placeholder Area/City itself, and both name
/// timedatectl list-timezones
#[test]
fn neither_the_guide_nor_the_example_names_a_zone_but_etc_utc() {
    for file in [GUIDE, EXAMPLE] {
        let text = read(file);
        for zone in zone_names(&text) {
            assert!(
                zone == "Etc/UTC" || zone == "Area/City",
                "{file} names the zone {zone}"
            );
        }
        // The tz database's names of another shape than Area/City.
        for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || "+-".contains(c))) {
            assert!(!OTHER_ZONES.contains(&word), "{file} names the zone {word}");
        }
        // Every value given to a zone setting is Etc/UTC.
        let values = zone_values(&text);
        assert!(!values.is_empty(), "{file} gives no zone setting a value");
        for value in values {
            assert_eq!(
                value, "Etc/UTC",
                "{file} gives a zone setting the value {value:?}"
            );
        }
        assert!(
            text.contains("timedatectl list-timezones"),
            "{file} does not name timedatectl list-timezones"
        );
    }
    // The values the rule reads, and words of a sentence it does not read as values.
    let sample = "TZ: \"Etc/UTC\"\nNAGE_TZ=XXT\nbind=0.0.0.0:8787 tz=Etc/UTC tz_source=TZ log=info\n| `TZ` | not set | UTC |\nGET /health PORT\n";
    assert_eq!(zone_values(sample), ["Etc/UTC", "XXT", "Etc/UTC"]);
}

/// Every port line the guide quotes stands in the example, and the guide writes no mapping right
/// after `ports:`.
///
/// Proves: SPEC decisions 188 and 191: every port line the guide quotes, a dash and a quoted
/// mapping, stands in deploy/compose.example.yaml, its comment included, and the guide writes no
/// mapping on the same line as ports with a colon
#[test]
fn the_guide_quotes_the_examples_port_lines_as_the_file_has_them() {
    let text = guide();
    let example: Vec<String> = read(EXAMPLE)
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim().to_owned())
        .collect();
    let mut quoted = 0;
    for (at, _) in text.match_indices("- \"") {
        let tail = &text[at + 3..];
        let Some(end) = tail.find('"') else {
            continue;
        };
        let mapping = &tail[..end];
        if !mapping.contains(':')
            || !mapping
                .chars()
                .all(|c| c.is_ascii_digit() || c == ':' || c == '.')
        {
            continue;
        }
        quoted += 1;
        let line = format!("- \"{mapping}\"");
        assert!(
            example.contains(&line),
            "{GUIDE} quotes the port line {line}, which {EXAMPLE} does not have"
        );
    }
    assert!(quoted > 0, "{GUIDE} quotes no port line of {EXAMPLE}");
    for (at, _) in text.match_indices("ports:") {
        let after = text[at + "ports:".len()..].trim_start_matches([' ', '\t']);
        assert!(
            !after.starts_with(|c: char| c.is_ascii_digit() || c == '"' || c == '['),
            "{GUIDE} writes a mapping on the same line as ports: {}",
            after.lines().next().unwrap_or_default()
        );
    }
}
