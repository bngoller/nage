//! The style file as the look's rules read it (milestone 2 slice 6a): every colour named once
//! for each palette, the contrast of every pair the SPEC row lists recomputed from those named
//! colours, the fonts named at the addresses Nage serves them at, and the rules no browser test
//! measures (no Pico, no outside address, the switch point, reduced motion, the marks).

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::{assert_one_mark, asset, crate_file};

/// The row's "the violet": where a role takes the violet, its palette names it.
const VIOLET: &str = "var(--n-violet)";

/// The colours decision 123 lists, by role: the custom property naming each, what it paints,
/// and its value in the dark and in the light palette, as the style file must write it. A value
/// `var(--n-violet)` is the row's "the violet" (the dark palette's main button's shadow and
/// mark's shadow, the light palette's link and focus ring), so each palette has one violet.
const COLOURS: [(&str, &str, &str, &str); 29] = [
    ("--n-page", "the page", "#0b1023", "#f7f4ea"),
    ("--n-panel", "a panel", "#10162f", "#ffffff"),
    ("--n-raised", "a raised ground", "#18203f", "#f1ede0"),
    ("--n-outline", "a panel's outline", "#2f3963", "#10162f"),
    (
        "--n-hair",
        "the hair line between rows",
        "#1f2850",
        "#e6e2d4",
    ),
    (
        "--n-edge",
        "the edge of form boxes and buttons",
        "#8791c7",
        "#10162f",
    ),
    ("--n-text", "text", "#f2f1ea", "#10162f"),
    ("--n-quiet", "quiet text", "#aeb4d0", "#4a5073"),
    ("--n-accent", "the accent", "#ffd300", "#ffd300"),
    (
        "--n-accent-hover",
        "the accent on hover",
        "#ffe24d",
        "#ffdf33",
    ),
    ("--n-on-accent", "ink on the accent", "#10162f", "#10162f"),
    ("--n-link", "a link", "#ffd300", VIOLET),
    (
        "--n-current-bg",
        "the entry in view's ground",
        "#18203f",
        "#ffd300",
    ),
    (
        "--n-current-ink",
        "the entry in view's ink",
        "#ffd300",
        "#10162f",
    ),
    (
        "--n-current-bar",
        "the entry in view's bar",
        "#ffd300",
        "#10162f",
    ),
    ("--n-done", "done", "#7fe0a7", "#00693e"),
    ("--n-not-done", "not done", "#ff8fa0", "#be1809"),
    (
        "--n-on-kind",
        "a message's sign on its block",
        "#0b1023",
        "#ffffff",
    ),
    ("--n-violet", "the violet", "#8b7dff", "#3a10e5"),
    ("--n-focus", "the focus ring", "#ffd300", VIOLET),
    (
        "--n-shadow-loud",
        "the main button's shadow",
        VIOLET,
        "#10162f",
    ),
    (
        "--n-loud-edge",
        "the main button's edge",
        "#ffd300",
        "#10162f",
    ),
    ("--n-shadow", "the other hard shadows", "#3a4470", "#10162f"),
    ("--n-input-bg", "a box ground", "#0b1023", "#fffdf7"),
    (
        "--n-total-bg",
        "the Net worth block's ground",
        "#18203f",
        "#ffffff",
    ),
    (
        "--n-total-ink",
        "the Net worth block's ink",
        "#f2f1ea",
        "#10162f",
    ),
    (
        "--n-total-label",
        "the Net worth block's label",
        "#aeb4d0",
        "#4a5073",
    ),
    (
        "--n-total-edge",
        "the Net worth block's edge",
        "#3a4470",
        "#10162f",
    ),
    ("--n-mark-shadow", "the mark's shadow", VIOLET, "#10162f"),
];

/// The drawings that cannot be a mask painted in a named colour (decision 123): the custom
/// property holding each `data:` address, the named colour every colour inside it must equal in
/// its palette, and what it draws.
const DRAWN: [(&str, &str, &str); 1] = [("--n-chevron", "--n-quiet", "the select's arrow")];

/// The lengths a palette block names besides its colours (decision 123): the custom property,
/// what it sizes, and its value in the dark and in the light palette. The Net worth block's
/// hard shadow, in the palette's shadow colour, stands 4 px right and below in the light palette
/// and shows nowhere in the dark one.
const LENGTHS: [(&str, &str, &str, &str); 1] = [(
    "--n-total-lift",
    "the offset of the Net worth block's hard shadow",
    "0",
    "4px",
)];

/// Text and the signs read as text: at least 4.5 to 1 (decision 123).
const TEXT: f64 = 4.5;

/// Large text and the edges of boxes and buttons: at least 3 to 1 (decision 123).
const LARGE: f64 = 3.0;

/// Every pair decision 123 lists: the colour in front, the ground, the least ratio and what the
/// pair is.
const PAIRS: [(&str, &str, f64, &str); 30] = [
    ("--n-text", "--n-page", TEXT, "text on the page"),
    ("--n-text", "--n-panel", TEXT, "text on a panel"),
    ("--n-text", "--n-raised", TEXT, "text on a raised ground"),
    ("--n-text", "--n-input-bg", TEXT, "text on a box ground"),
    ("--n-quiet", "--n-page", TEXT, "quiet text on the page"),
    ("--n-quiet", "--n-panel", TEXT, "quiet text on a panel"),
    (
        "--n-quiet",
        "--n-raised",
        TEXT,
        "quiet text on a raised ground",
    ),
    ("--n-link", "--n-page", TEXT, "a link on the page"),
    ("--n-link", "--n-panel", TEXT, "a link on a panel"),
    (
        "--n-current-ink",
        "--n-current-bg",
        TEXT,
        "the entry in view's ink on its ground",
    ),
    ("--n-on-accent", "--n-accent", TEXT, "ink on the accent"),
    ("--n-done", "--n-panel", TEXT, "done on a panel"),
    ("--n-not-done", "--n-panel", TEXT, "not done on a panel"),
    ("--n-not-done", "--n-page", TEXT, "not done on the page"),
    (
        "--n-on-kind",
        "--n-done",
        TEXT,
        "a message's sign on the done block",
    ),
    (
        "--n-on-kind",
        "--n-not-done",
        TEXT,
        "a message's sign on the not-done block",
    ),
    (
        "--n-total-ink",
        "--n-total-bg",
        TEXT,
        "the Net worth ink on its ground",
    ),
    (
        "--n-total-label",
        "--n-total-bg",
        TEXT,
        "the Net worth label on its ground",
    ),
    ("--n-edge", "--n-panel", LARGE, "the edge on a panel"),
    ("--n-edge", "--n-page", LARGE, "the edge on the page"),
    (
        "--n-edge",
        "--n-input-bg",
        LARGE,
        "the edge on a box ground",
    ),
    (
        "--n-edge",
        "--n-raised",
        LARGE,
        "the edge on a raised ground",
    ),
    (
        "--n-not-done",
        "--n-input-bg",
        LARGE,
        "not done as a refused box's edge on a box ground",
    ),
    ("--n-focus", "--n-page", LARGE, "the focus ring on the page"),
    ("--n-focus", "--n-panel", LARGE, "the focus ring on a panel"),
    (
        "--n-quiet",
        "--n-input-bg",
        LARGE,
        "the select's drawn arrow on a box ground",
    ),
    ("--n-text", "--n-panel", LARGE, "a menu sign on a panel"),
    (
        "--n-current-ink",
        "--n-current-bg",
        LARGE,
        "the entry in view's sign on its ground",
    ),
    (
        "--n-quiet",
        "--n-panel",
        LARGE,
        "the person sign on a panel",
    ),
    (
        "--n-on-accent",
        "--n-accent",
        LARGE,
        "Nage's mark, its navy on its accent",
    ),
];

/// The one width query of the style file, the 48rem switch point, without white space.
const SWITCH: &str = "@media(min-width:48rem)";

/// The group under which nothing moves, without white space.
const REDUCED: &str = "@media(prefers-reduced-motion:reduce)";

/// The mark's two colours fixed in its drawing (decision 125): its navy and its tile.
const MARK_INK: &str = "#10162f";
const MARK_ACCENT: &str = "#ffd300";

/// The four fonts of decision 126: the family, the weight and the name each file is served under.
const FONTS: [(&str, &str, &str); 4] = [
    ("IBM Plex Sans", "400", "IBMPlexSans-Regular-Latin1.woff2"),
    ("IBM Plex Sans", "600", "IBMPlexSans-SemiBold-Latin1.woff2"),
    ("IBM Plex Mono", "400", "IBMPlexMono-Regular-Latin1.woff2"),
    ("IBM Plex Mono", "600", "IBMPlexMono-SemiBold-Latin1.woff2"),
];

/// The three palette blocks: what each is, whether it is the light palette, the group it stands
/// in and its selector, both without white space and quotes. The dark palette leads at `:root`
/// (its block is the `:root` rule that sets `color-scheme`); the light one is written once for
/// the device's setting and once for `data-theme`.
const BLOCKS: [(&str, bool, Option<&str>, &str); 3] = [
    (
        "the dark palette's block (:root, with its color-scheme)",
        false,
        None,
        ":root",
    ),
    (
        "the light palette's block for the device's setting (:root:not([data-theme=dark]) under prefers-color-scheme light)",
        true,
        Some("@media(prefers-color-scheme:light)"),
        ":root:not([data-theme=dark])",
    ),
    (
        "the light palette's block for data-theme (:root[data-theme=light])",
        true,
        None,
        ":root[data-theme=light]",
    ),
];

/// One style rule of the sheet, with the groups (`@media` preludes) it stands in.
struct Rule {
    groups: Vec<String>,
    selector: String,
    decls: Vec<(String, String)>,
}

impl Rule {
    /// The value of the last declaration of `name`.
    fn value(&self, name: &str) -> Option<&str> {
        self.decls
            .iter()
            .rev()
            .find(|(declared, _)| declared == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The style file, its comments removed, and its rules.
struct Sheet {
    text: String,
    rules: Vec<Rule>,
    groups: Vec<String>,
}

impl Sheet {
    /// The embedded style file, as Nage serves it.
    fn load() -> Self {
        let raw = String::from_utf8(asset("app.css").body.to_vec())
            .unwrap_or_else(|error| panic!("app.css is not UTF-8: {error}"));
        let text = strip_comments(&raw);
        let mut rules = Vec::new();
        let mut groups = Vec::new();
        let mut pos = 0;
        parse_nodes(&text, &mut pos, &[], &mut rules, &mut groups);
        Self {
            text,
            rules,
            groups,
        }
    }

    /// The palette blocks of [`BLOCKS`], each found exactly once, or what is wrong.
    fn blocks(&self) -> Vec<Result<Block<'_>, String>> {
        BLOCKS
            .iter()
            .map(|&(what, light, group, selector)| {
                let found: Vec<usize> = self
                    .rules
                    .iter()
                    .enumerate()
                    .filter(|(_, rule)| {
                        let groups: Vec<String> =
                            rule.groups.iter().map(|group| norm(group)).collect();
                        let in_group = match group {
                            Some(group) => groups == [group],
                            None => groups.is_empty(),
                        };
                        in_group
                            && norm(&rule.selector) == selector
                            && (light || rule.value("color-scheme").is_some())
                    })
                    .map(|(index, _)| index)
                    .collect();
                match found.as_slice() {
                    [index] => Ok(Block {
                        what,
                        light,
                        index: *index,
                        rule: &self.rules[*index],
                    }),
                    _ => Err(format!("{what}: found {} times", found.len())),
                }
            })
            .collect()
    }

    /// Whether `name` is used by name somewhere in the sheet.
    fn uses(&self, name: &str) -> bool {
        self.text.contains(&format!("var({name})"))
    }
}

/// A palette block of the sheet.
struct Block<'a> {
    what: &'static str,
    light: bool,
    index: usize,
    rule: &'a Rule,
}

impl Block<'_> {
    /// The colour `name` stands for in this block, following `var()` references inside it.
    fn resolve(&self, name: &str) -> Option<String> {
        let mut current = name.to_owned();
        for _ in 0..4 {
            let value = self.rule.value(&current)?;
            match value
                .strip_prefix("var(")
                .and_then(|rest| rest.strip_suffix(')'))
            {
                Some(inner) => current = inner.trim().to_owned(),
                None => return Some(value.to_ascii_lowercase()),
            }
        }
        None
    }
}

/// `css` without its comments.
fn strip_comments(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// `text` without white space and quotes, for comparing selectors and preludes.
fn norm(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '"' && *ch != '\'')
        .collect()
}

/// The first of `stops` in `text` from `from` that stands outside quotes and parentheses.
fn find_stop(text: &str, from: usize, stops: &[char]) -> Option<(usize, char)> {
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    for (offset, ch) in text[from..].char_indices() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && stops.contains(&ch) => return Some((from + offset, ch)),
            _ => {}
        }
    }
    None
}

/// Reads the rules from `pos` to the closing brace of the current group, pushing each with the
/// groups it stands in, and every group's prelude.
fn parse_nodes(
    text: &str,
    pos: &mut usize,
    outer: &[String],
    rules: &mut Vec<Rule>,
    groups: &mut Vec<String>,
) {
    while let Some((at, ch)) = find_stop(text, *pos, &['{', '}']) {
        if ch == '}' {
            *pos = at + 1;
            return;
        }
        let prelude = text[*pos..at].trim().to_owned();
        *pos = at + 1;
        if prelude.starts_with("@media") || prelude.starts_with("@supports") {
            groups.push(prelude.clone());
            let mut inner = outer.to_vec();
            inner.push(prelude);
            parse_nodes(text, pos, &inner, rules, groups);
        } else {
            let (end, _) = find_stop(text, *pos, &['}'])
                .unwrap_or_else(|| panic!("app.css: the block of {prelude} is not closed"));
            let decls = declarations(&text[*pos..end]);
            *pos = end + 1;
            rules.push(Rule {
                groups: outer.to_vec(),
                selector: prelude,
                decls,
            });
        }
    }
}

/// The declarations of a block's body, in order.
fn declarations(body: &str) -> Vec<(String, String)> {
    let mut decls = Vec::new();
    let mut from = 0;
    loop {
        let end = find_stop(body, from, &[';']).map_or(body.len(), |(at, _)| at);
        if let Some((name, value)) = body[from..end].trim().split_once(':') {
            decls.push((name.trim().to_owned(), value.trim().to_owned()));
        }
        if end >= body.len() {
            return decls;
        }
        from = end + 1;
    }
}

/// Every colour spelled with a hash or a percent-encoded hash in `value`, lowercase, with a hash.
fn hex_colours(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["#", "%23"] {
        for (at, _) in value.match_indices(marker) {
            let digits: String = value[at + marker.len()..]
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .collect();
            if !digits.is_empty() {
                found.push(format!("#{}", digits.to_ascii_lowercase()));
            }
        }
    }
    found
}

/// The relative luminance of a `#rrggbb` colour (WCAG 2).
fn luminance(hex: &str) -> Option<f64> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let mut sum = 0.0;
    for (index, weight) in [0.2126, 0.7152, 0.0722].into_iter().enumerate() {
        let channel = u8::from_str_radix(digits.get(index * 2..index * 2 + 2)?, 16).ok()?;
        let c = f64::from(channel) / 255.0;
        let linear = if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
        sum += weight * linear;
    }
    Some(sum)
}

/// The contrast ratio of two `#rrggbb` colours (WCAG 2).
fn contrast(one: &str, other: &str) -> Option<f64> {
    let (one, other) = (luminance(one)?, luminance(other)?);
    let (light, dark) = if one >= other {
        (one, other)
    } else {
        (other, one)
    };
    Some((light + 0.05) / (dark + 0.05))
}

/// The arguments of every `url()` in `text`, quotes removed, and `text` without them.
fn urls(text: &str) -> (Vec<String>, String) {
    let mut found = Vec::new();
    let mut rest_text = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("url(") {
        rest_text.push_str(&rest[..start + 4]);
        let after = rest[start + 4..].trim_start();
        let (argument, remainder) = match after.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let body = &after[1..];
                let end = body.find(quote).unwrap_or(body.len());
                (&body[..end], body.get(end + 1..).unwrap_or("").trim_start())
            }
            _ => {
                let end = after.find(')').unwrap_or(after.len());
                (after[..end].trim_end(), &after[end..])
            }
        };
        found.push(argument.to_owned());
        rest = remainder;
    }
    rest_text.push_str(rest);
    (found, rest_text)
}

/// The colours CSS names by a word (CSS Color 4), the system colours among them, lowercase;
/// `currentcolor` and `transparent`, which name no colour of their own, are not among them.
const NAMED_COLOURS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
    "accentcolor",
    "accentcolortext",
    "activetext",
    "buttonborder",
    "buttonface",
    "buttontext",
    "canvas",
    "canvastext",
    "field",
    "fieldtext",
    "graytext",
    "highlight",
    "highlighttext",
    "linktext",
    "mark",
    "marktext",
    "selecteditem",
    "selecteditemtext",
    "visitedtext",
];

/// The CSS functions that write a colour by its channels.
const COLOUR_FUNCTIONS: &[&str] = &[
    "rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch", "color",
];

/// The words of a declaration's `value` outside quotes and `url()` arguments, lowercase, each
/// with whether a `(` follows it (a function).
fn words(value: &str) -> Vec<(String, bool)> {
    let (_, outside) = urls(value);
    let mut plain = String::new();
    let mut quote: Option<char> = None;
    for ch in outside.chars() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None => plain.push(ch),
        }
    }
    let is_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_';
    let mut found = Vec::new();
    let mut rest = plain.as_str();
    while let Some(start) = rest.find(is_word) {
        let tail = &rest[start..];
        let end = tail.find(|ch: char| !is_word(ch)).unwrap_or(tail.len());
        found.push((
            tail[..end].to_ascii_lowercase(),
            tail[end..].starts_with('('),
        ));
        rest = &tail[end..];
    }
    found
}

/// Every paint of a `data:` address: each attribute whose name ends with `fill`, `stroke` or
/// `color`, with its quoted value, lowercase.
fn paints(address: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (at, _) in address.match_indices('=') {
        let start = address[..at]
            .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
            .map_or(0, |index| index + 1);
        let name = address[start..at].to_ascii_lowercase();
        if !(name.ends_with("fill") || name.ends_with("stroke") || name.ends_with("color")) {
            continue;
        }
        let after = &address[at + 1..];
        let value = match after.chars().next() {
            Some(quote @ ('\'' | '"')) => after[1..].split(quote).next().unwrap_or(""),
            _ => after.split([' ', '/', '%']).next().unwrap_or(""),
        };
        found.push((name, value.to_ascii_lowercase()));
    }
    found
}

/// The one rule of the sheet with the selector `selector` standing in exactly the groups
/// `groups`, both compared without white space and quotes, or what is wrong.
fn rule_at<'a>(sheet: &'a Sheet, groups: &[&str], selector: &str) -> Result<&'a Rule, String> {
    let found: Vec<&Rule> = sheet
        .rules
        .iter()
        .filter(|rule| {
            norm(&rule.selector) == norm(selector)
                && rule
                    .groups
                    .iter()
                    .map(|group| norm(group))
                    .collect::<Vec<String>>()
                    == groups
                        .iter()
                        .map(|group| norm(group))
                        .collect::<Vec<String>>()
        })
        .collect();
    match found.as_slice() {
        [rule] => Ok(*rule),
        _ => Err(format!(
            "the rule {selector} in {groups:?} stands {} times",
            found.len()
        )),
    }
}

/// The value of the declaration `name` of `rule`, without white space and quotes.
fn normed(rule: &Rule, name: &str) -> Option<String> {
    rule.value(name).map(norm)
}

/// Every `.ts` file under `dir`, recursively.
fn ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            ts_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ts") {
            out.push(path);
        }
    }
}

/// The switch points the e2e suite names: every number of three digits or more compared with
/// `>=` or `<` on a line about a width, with the file it stands in.
fn suite_switch_points() -> Vec<(String, u32)> {
    let e2e = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e");
    let mut files = Vec::new();
    ts_files(&e2e.join("tests"), &mut files);
    ts_files(&e2e.join("lib"), &mut files);
    let mut points = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()));
        for line in text.lines().filter(|line| line.contains("width")) {
            for op in [">=", "<"] {
                for (at, _) in line.match_indices(op) {
                    let digits: String = line[at + op.len()..]
                        .trim_start()
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    if digits.len() >= 3
                        && let Ok(number) = digits.parse()
                    {
                        points.push((file.display().to_string(), number));
                    }
                }
            }
        }
    }
    points
}

/// Each colour decision 123 lists is defined exactly once in each palette block, by the name
/// the row's role takes, with the row's value (the violet named where the row says the violet);
/// the light palette's two blocks are equal name for name and value for value, `color-scheme`
/// included, and each block sets its palette's `color-scheme`; no colour is defined or written
/// outside the palette blocks, neither spelled with a hash nor by a colour's name nor by a
/// colour function such as `rgb()`; every named colour is used by name; every colour inside a
/// `data:` address, spelled with `#` or `%23`, equals the named colour of its drawing in its
/// palette, and no `fill`, `stroke` or colour inside one is a colour's name, but `none`, and
/// `black` on a mask, which paints nothing itself; the Net worth block's shadow offset is named
/// once in each palette block (0 in dark, 4px in light) and the block's hard shadow stands at it
/// in the palette's shadow colour; and the base layout's head carries
/// `<meta name="color-scheme" content="dark light">`.
///
/// Proves: decision 123: every colour the row lists defined once in each palette block as a
/// named custom property with the row's value; the blocks of one palette, for the device's
/// setting and for data-theme, equal name for name and value for value, color-scheme included;
/// every named colour used by name; every colour inside a data address, spelled with a hash or a
/// percent-encoded hash, equal to its named colour; and the color-scheme meta dark light in the
/// base layout's head
#[test]
fn the_style_file_defines_every_colour_once_by_name_for_both_palettes() {
    let sheet = Sheet::load();
    let blocks: Vec<Block<'_>> = sheet
        .blocks()
        .into_iter()
        .map(|block| {
            block.unwrap_or_else(|problem| panic!("the style file does not define {problem}"))
        })
        .collect();

    for block in &blocks {
        let scheme = if block.light { "light" } else { "dark" };
        assert_eq!(
            block.rule.value("color-scheme"),
            Some(scheme),
            "the style file does not define the color-scheme {scheme} in {}",
            block.what
        );
        for (name, role, dark, light) in COLOURS {
            let expected = if block.light { light } else { dark };
            let values: Vec<&str> = block
                .rule
                .decls
                .iter()
                .filter(|(declared, _)| declared == name)
                .map(|(_, value)| value.as_str())
                .collect();
            assert_eq!(
                values,
                [expected],
                "the style file does not define {name} ({role}) once as {expected} in {}",
                block.what
            );
        }
        for (name, role, dark, light) in LENGTHS {
            let expected = if block.light { light } else { dark };
            let values: Vec<&str> = block
                .rule
                .decls
                .iter()
                .filter(|(declared, _)| declared == name)
                .map(|(_, value)| value.as_str())
                .collect();
            assert_eq!(
                values,
                [expected],
                "the style file does not define {name} ({role}) once as {expected} in {}",
                block.what
            );
        }
        for (declared, value) in &block.rule.decls {
            let listed = declared == "color-scheme"
                || COLOURS.iter().any(|(name, ..)| name == declared)
                || DRAWN.iter().any(|(name, ..)| name == declared)
                || LENGTHS.iter().any(|(name, ..)| name == declared);
            assert!(
                listed,
                "the style file does not define {declared} among the row's colours, yet {} holds it: {value}",
                block.what
            );
        }
        for (name, colour, what) in DRAWN {
            let values: Vec<&str> = block
                .rule
                .decls
                .iter()
                .filter(|(declared, _)| declared == name)
                .map(|(_, value)| value.as_str())
                .collect();
            let [value] = values.as_slice() else {
                panic!(
                    "the style file does not define {name} ({what}) once in {}: {values:?}",
                    block.what
                );
            };
            assert!(
                value.starts_with("url(\"data:image/svg+xml,"),
                "the style file does not define {name} ({what}) as a data address in {}: {value}",
                block.what
            );
            let expected = block.resolve(colour).unwrap_or_else(|| {
                panic!("the style file does not define {colour} in {}", block.what)
            });
            let found = hex_colours(value);
            assert!(
                !found.is_empty() && found.iter().all(|hex| *hex == expected),
                "the style file does not define {name} ({what}) in {colour}'s value {expected} in {}: it paints {found:?}",
                block.what
            );
        }
    }

    let light: Vec<Vec<(String, String)>> = blocks
        .iter()
        .filter(|block| block.light)
        .map(|block| {
            let mut decls = block.rule.decls.clone();
            decls.sort();
            decls
        })
        .collect();
    assert_eq!(
        light.len(),
        2,
        "the style file does not define the light palette's two blocks"
    );
    assert_eq!(
        light[0], light[1],
        "the style file does not define the light palette's two blocks equal, name for name and value for value"
    );

    let palette: BTreeSet<usize> = blocks.iter().map(|block| block.index).collect();
    for (index, rule) in sheet.rules.iter().enumerate() {
        if palette.contains(&index) {
            continue;
        }
        for (declared, value) in &rule.decls {
            let named = declared == "color-scheme"
                || COLOURS.iter().any(|(name, ..)| name == declared)
                || DRAWN.iter().any(|(name, ..)| name == declared)
                || LENGTHS.iter().any(|(name, ..)| name == declared);
            assert!(
                !named,
                "the style file does not define {declared} in a palette block only: {} sets it to {value}",
                rule.selector
            );
            let written = hex_colours(value);
            assert!(
                written.is_empty(),
                "the style file does not define every colour by name: {} writes {written:?} in {declared}: {value}",
                rule.selector
            );
            for (word, call) in words(value) {
                assert!(
                    !NAMED_COLOURS.contains(&word.as_str())
                        && !(call && COLOUR_FUNCTIONS.contains(&word.as_str())),
                    "the style file does not define every colour by name: {} writes the colour {word}{} in {declared}: {value}",
                    rule.selector,
                    if call { "()" } else { "" }
                );
            }
        }
    }

    for rule in &sheet.rules {
        for (declared, value) in &rule.decls {
            let drawn = DRAWN.iter().any(|(name, ..)| name == declared);
            for address in urls(value)
                .0
                .iter()
                .filter(|address| address.starts_with("data:"))
            {
                for (attribute, paint) in paints(address) {
                    let allowed = paint == "none"
                        || paint.starts_with('#')
                        || paint.starts_with("%23")
                        || (!drawn && paint == "black");
                    assert!(
                        allowed,
                        "the style file does not define every colour by name: {} draws {attribute}='{paint}' inside the data address of {declared}, neither none, a colour spelled with a hash checked against its named colour, nor black on a mask",
                        rule.selector
                    );
                }
            }
        }
    }

    for name in COLOURS
        .iter()
        .map(|(name, ..)| *name)
        .chain(DRAWN.iter().map(|(name, ..)| *name))
        .chain(LENGTHS.iter().map(|(name, ..)| *name))
    {
        assert!(
            sheet.uses(name),
            "the style file does not define {name} for a use: nothing uses var({name})"
        );
    }

    let total = rule_at(&sheet, &[], "p.net-worth").unwrap_or_else(|problem| {
        panic!("the style file does not define the Net worth block: {problem}")
    });
    assert_eq!(
        normed(total, "box-shadow").as_deref(),
        Some("var(--n-total-lift)var(--n-total-lift)0var(--n-shadow)"),
        "the style file does not define the Net worth block's hard shadow at its named offset in the palette's shadow colour"
    );

    let base = String::from_utf8(crate_file("templates/base.html"))
        .unwrap_or_else(|error| panic!("base.html is not UTF-8: {error}"));
    let head = base.split("</head>").next().unwrap_or("");
    assert!(
        base.contains("</head>")
            && head.contains(r#"<meta name="color-scheme" content="dark light">"#),
        "the style file does not define the light controls without the color-scheme meta dark light in the base layout's head:\n{head}"
    );
}

/// Every pair of colours decision 123 lists meets its least contrast in every palette block,
/// recomputed from the colours the block names: 4.5 to 1 for text, 3 to 1 for large text and
/// the edges of boxes and buttons; and every rule that paints a named colour on a named ground
/// (`color` with `background` or `background-color`) paints one of the listed pairs.
///
/// Proves: decision 123: 4.5 to 1 for text and 3 to 1 for large text and edges, recomputed from
/// the named colours of every palette block for every pair the row lists
#[test]
fn every_listed_pair_of_colours_meets_its_contrast_in_both_palettes() {
    let sheet = Sheet::load();
    let mut below = Vec::new();
    for block in sheet.blocks() {
        let block = block.unwrap_or_else(|problem| {
            panic!("the style file has no colour named for a palette block: {problem}")
        });
        for (front, ground, least, what) in PAIRS {
            let colour = |name: &str| {
                block.resolve(name).unwrap_or_else(|| {
                    panic!(
                        "the style file has no colour named {name} in {}",
                        block.what
                    )
                })
            };
            let (front_hex, ground_hex) = (colour(front), colour(ground));
            let ratio = contrast(&front_hex, &ground_hex).unwrap_or_else(|| {
                panic!(
                    "the style file has no colour named {front} or {ground} as #rrggbb in {}: {front_hex}, {ground_hex}",
                    block.what
                )
            });
            if ratio < least {
                below.push(format!(
                    "{what} ({front} {front_hex} on {ground} {ground_hex}) in {}: {ratio:.2} to 1, below {least} to 1",
                    block.what
                ));
            }
        }
    }
    assert!(
        below.is_empty(),
        "pairs below their contrast:\n{}",
        below.join("\n")
    );

    let listed: BTreeSet<(&str, &str)> = PAIRS
        .iter()
        .map(|(front, ground, ..)| (*front, *ground))
        .collect();
    let by_name = |value: &str| {
        value
            .trim()
            .strip_prefix("var(")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(|name| name.trim().to_owned())
    };
    let mut unlisted = Vec::new();
    for rule in &sheet.rules {
        let ground = rule
            .decls
            .iter()
            .rev()
            .find(|(declared, _)| declared == "background" || declared == "background-color")
            .and_then(|(_, value)| by_name(value));
        let front = rule.value("color").and_then(by_name);
        if let (Some(front), Some(ground)) = (front, ground)
            && !listed.contains(&(front.as_str(), ground.as_str()))
        {
            unlisted.push(format!("{} paints {front} on {ground}", rule.selector));
        }
    }
    assert!(
        unlisted.is_empty(),
        "pairs the style file paints that the row does not list, so no contrast holds them:\n{}",
        unlisted.join("\n")
    );
}

/// The colour literals of the mark's template and of the tab icon are the style file's ink on
/// the accent and accent, in every palette block, and the template's tile, filled with
/// `currentColor`, is painted by the style file in the accent by name; the template, the tab
/// icon and the home-screen icon's source draw the same tile and the same tilde.
///
/// Proves: decision 125 and decision 123: the colour literals of templates/mark.html and
/// static/icons/mark.svg are the style file's ink on the accent and accent, 10162f and ffd300,
/// so the mark's pair is recomputed from values held equal
#[test]
fn the_mark_is_drawn_in_the_ink_on_the_accent_and_the_accent() {
    let sheet = Sheet::load();
    for block in sheet.blocks() {
        let block = block.unwrap_or_else(|problem| {
            panic!("the mark is not drawn in the named colours: the style file lacks {problem}")
        });
        for (name, expected) in [("--n-on-accent", MARK_INK), ("--n-accent", MARK_ACCENT)] {
            assert_eq!(
                block.resolve(name).as_deref(),
                Some(expected),
                "the mark is not drawn in the named colours: {name} in {} is not {expected}",
                block.what
            );
        }
    }
    let pair: BTreeSet<String> = [MARK_INK, MARK_ACCENT]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for (file, whole) in [
        ("templates/mark.html", false),
        ("static/icons/mark.svg", true),
    ] {
        let text = String::from_utf8(crate_file(file))
            .unwrap_or_else(|error| panic!("{file} is not UTF-8: {error}"));
        let found: BTreeSet<String> = hex_colours(&text).into_iter().collect();
        assert!(
            !found.is_empty() && found.is_subset(&pair) && (!whole || found == pair),
            "the mark is not drawn in the named colours: {file} paints {found:?}, not the ink on the accent {MARK_INK} and the accent {MARK_ACCENT}"
        );
        assert!(
            found.contains(MARK_INK),
            "the mark is not drawn in the named colours: {file} has no {MARK_INK}"
        );
    }
    let template = String::from_utf8(crate_file("templates/mark.html"))
        .unwrap_or_else(|error| panic!("mark.html is not UTF-8: {error}"));
    assert!(
        template.contains(r#"fill="currentColor""#),
        "the mark is not drawn in the named colours: templates/mark.html's tile is not filled with currentColor"
    );
    let painted = sheet.rules.iter().any(|rule| {
        rule.selector
            .split(',')
            .all(|selector| selector.trim().ends_with(".mark"))
            && rule.value("color") == Some("var(--n-accent)")
    });
    assert!(
        painted,
        "the mark is not drawn in the named colours: no rule paints .mark with var(--n-accent)"
    );
    assert_one_mark("the mark is not drawn in the named colours as one mark");
}

/// Each of the four IBM Plex files is named by one `@font-face` at the address Nage serves it
/// at, as woff2, with its family, weight and `font-display: swap`; there are no other
/// `@font-face` rules, none uses a `data:` address, and every address the style file names under
/// `/static/` is one Nage serves; `html` sets `font-synthesis: none`, the headings weight 600,
/// and the small labels (form labels, section names, group names and the Appearance legend)
/// Plex Mono in spaced capitals by `text-transform`.
///
/// Proves: decision 126 and decision 46: each font named at the address Nage serves it at, with
/// font-display swap, and no data address for a font
#[test]
fn the_style_file_names_each_font_at_its_served_address() {
    let sheet = Sheet::load();
    let faces: Vec<&Rule> = sheet
        .rules
        .iter()
        .filter(|rule| rule.selector == "@font-face")
        .collect();
    for (family, weight, file) in FONTS {
        let address = nage_web::asset_url(file).unwrap_or_else(|| {
            panic!("the style file does not name the font {file}: Nage does not serve it")
        });
        let named: Vec<&&Rule> = faces
            .iter()
            .filter(|face| {
                face.value("font-family").map(norm) == Some(norm(family))
                    && face.value("font-weight") == Some(weight)
            })
            .collect();
        let [face] = named.as_slice() else {
            panic!(
                "the style file does not name the font {family} at weight {weight} once: {} @font-face rules",
                named.len()
            );
        };
        assert_eq!(
            face.value("src").map(norm),
            Some(norm(&format!("url({address}) format(\"woff2\")"))),
            "the style file does not name the font {family} {weight} at its served address {address}"
        );
        assert_eq!(
            face.value("font-display"),
            Some("swap"),
            "the style file does not name the font {family} {weight} with font-display swap"
        );
        assert_eq!(
            face.value("font-style"),
            Some("normal"),
            "the style file does not name the font {family} {weight} as normal, not italic"
        );
    }
    assert_eq!(
        faces.len(),
        FONTS.len(),
        "the style file does not name the fonts in exactly four @font-face rules"
    );
    for face in &faces {
        let src = face.value("src").unwrap_or("");
        assert!(
            !src.contains("data:"),
            "the style file does not name the font at an address: a data address: {src}"
        );
    }
    let served: BTreeSet<&str> = nage_web::assets()
        .iter()
        .map(|asset| asset.url.as_str())
        .collect();
    for address in urls(&sheet.text).0 {
        if address.starts_with("/static/") {
            assert!(
                served.contains(address.as_str()),
                "the style file does not name the font or file at a served address: {address}"
            );
        }
    }

    let found = |groups: &[&str], selector: &str| {
        rule_at(&sheet, groups, selector)
            .unwrap_or_else(|problem| panic!("the style file does not name the font: {problem}"))
    };
    let html = found(&[], "html");
    assert_eq!(
        normed(html, "font-synthesis").as_deref(),
        Some("none"),
        "the style file does not name the font with font-synthesis none on html, so a browser may fake a bold or an italic Nage does not serve"
    );
    let headings = found(&[], "h1,h2,h3,h4,h5,h6");
    assert_eq!(
        normed(headings, "font-weight").as_deref(),
        Some("600"),
        "the style file does not name the font of the headings at weight 600"
    );
    let labels = found(
        &[],
        "section.balances h2 .name,section.balances h3,p.net-worth .name,li.look legend,.field>label:not(.tick)",
    );
    for (name, expected) in [
        ("font-family", "var(--n-mono)"),
        ("text-transform", "uppercase"),
        ("letter-spacing", "var(--n-label-track)"),
    ] {
        assert_eq!(
            normed(labels, name).as_deref(),
            Some(expected),
            "the style file does not name the font of the small labels (form labels, section names, group names, the Appearance legend) with {name} {expected}"
        );
    }
}

/// The rules of the look that no browser test measures: no Pico; no outside address and no
/// import; no box-drawing character or its escape; no sign selected by an address, in any case
/// of letters, and each menu sign set by its entry's class; no text colour of the light palette
/// a yellow (the accent or the accent on hover); htmx's two indicator rules as htmx would inject
/// them; every width query exactly `min-width: 48rem`, the e2e suite's 768 px, and the root at
/// 100% outside it and 93.75% inside it (16 px on a phone, 15 px on a computer); every
/// transition and animation off under reduced motion, in that group alone, and none marked
/// `!important` elsewhere; keyboard focus a 2 px ring in the focus colour, 3 px from the
/// control; a button pressing 3 px onto its shadow; amounts in Plex Mono with digits of equal
/// width, right-aligned; the light palette under both selectors; every drawn `content` empty or
/// with the empty alternative text `/ ""`; and a plain `content` before every alternative-text
/// one.
///
/// Proves: decision 123 and decision 126: no Pico, no outside address or import, no box-drawing
/// character or its escape, no sign selected by an address (an href selector), no text colour of
/// the light palette the accent, htmx's two indicator rules kept, the 48rem switch point the same
/// in every media query and equal to the e2e suite's 768 px, every transition and animation off
/// under reduced motion, the light palette under both selectors, a plain content before an
/// alternative-text one
#[test]
fn the_style_file_keeps_the_rules_of_the_look() {
    let sheet = Sheet::load();
    let raw = String::from_utf8(asset("app.css").body.to_vec())
        .unwrap_or_else(|error| panic!("app.css is not UTF-8: {error}"));
    let mut problems: Vec<String> = Vec::new();

    if raw.to_ascii_lowercase().contains("pico") {
        problems.push("it names Pico".to_owned());
    }

    if sheet.text.contains("@import") {
        problems.push("it imports another sheet".to_owned());
    }
    let (addresses, without_urls) = urls(&sheet.text);
    for address in &addresses {
        if !(address.starts_with("/static/") || address.starts_with("data:image/svg+xml,")) {
            problems.push(format!("it names an address outside Nage: {address}"));
        }
    }
    if without_urls.contains("http") || without_urls.contains("//") {
        problems.push("it names an outside address outside url()".to_owned());
    }

    if raw
        .chars()
        .any(|ch| ('\u{2500}'..='\u{257f}').contains(&ch))
    {
        problems.push("it holds a box-drawing character".to_owned());
    }
    for (at, _) in raw.match_indices('\\') {
        let digits: String = raw[at + 1..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .take(6)
            .collect();
        if let Ok(code) = u32::from_str_radix(&digits, 16)
            && (0x2500..=0x257f).contains(&code)
        {
            problems.push(format!(
                "it holds the escape of a box-drawing character: \\{digits}"
            ));
        }
    }

    for rule in &sheet.rules {
        if norm(&rule.selector).to_ascii_lowercase().contains("[href") {
            problems.push(format!("it selects by an address: {}", rule.selector));
        }
    }

    let mut signs: Vec<(String, String)> = sheet
        .rules
        .iter()
        .filter_map(|rule| normed(rule, "--n-sign").map(|sign| (norm(&rule.selector), sign)))
        .collect();
    signs.sort();
    let mut expected_signs: Vec<(String, String)> = [
        ("header.bar li.home>a", "var(--n-sign-home)"),
        ("header.bar li.password>a", "var(--n-sign-lock)"),
        ("header.bar li.who", "var(--n-sign-person)"),
        ("header.bar li.sign-out button", "var(--n-sign-leave)"),
    ]
    .into_iter()
    .map(|(selector, sign)| (norm(selector), sign.to_owned()))
    .collect();
    expected_signs.sort();
    if signs != expected_signs {
        problems.push(format!(
            "the menu's signs do not follow their entries' classes (a house for li.home, a lock for li.password, a person for li.who, an arrow leaving a door for li.sign-out): {signs:?}"
        ));
    }

    let blocks: Vec<Block<'_>> = sheet
        .blocks()
        .into_iter()
        .filter_map(|block| match block {
            Ok(block) => Some(block),
            Err(problem) => {
                problems.push(format!("the palette blocks: {problem}"));
                None
            }
        })
        .collect();
    let light: Vec<&Block<'_>> = blocks.iter().filter(|block| block.light).collect();
    if light.len() != 2 {
        problems.push(
            "the light palette does not stand under both selectors (the device's setting and data-theme)"
                .to_owned(),
        );
    }
    for block in &light {
        let (Some(accent), Some(hover)) = (
            block.resolve("--n-accent"),
            block.resolve("--n-accent-hover"),
        ) else {
            problems.push(format!("{} names no accent or accent on hover", block.what));
            continue;
        };
        let yellows = [accent, hover];
        for rule in &sheet.rules {
            let drawing = rule
                .selector
                .split(',')
                .all(|selector| selector.trim().ends_with(".mark"));
            if drawing {
                continue;
            }
            for (declared, value) in &rule.decls {
                if declared != "color" {
                    continue;
                }
                let name = value
                    .strip_prefix("var(")
                    .and_then(|rest| rest.strip_suffix(')'));
                if let Some(name) = name
                    && block
                        .resolve(name.trim())
                        .is_some_and(|colour| yellows.contains(&colour))
                {
                    problems.push(format!(
                        "{} sets yellow text in {}: color {value}",
                        rule.selector, block.what
                    ));
                }
            }
        }
    }

    let htmx = String::from_utf8_lossy(asset("htmx.min.js").body);
    for (template, rule) in [
        (
            "{opacity:0;visibility: hidden}",
            ".htmx-indicator{opacity:0;visibility: hidden}",
        ),
        (
            "{opacity:1;visibility: visible;transition: opacity 200ms ease-in}",
            ".htmx-request .htmx-indicator, .htmx-request.htmx-indicator{opacity:1;visibility: visible;transition: opacity 200ms ease-in}",
        ),
    ] {
        if !htmx.contains(template) {
            problems.push(format!(
                "the vendored htmx no longer injects {template}: re-copy its indicator rules"
            ));
        }
        if !raw.lines().any(|line| line == rule) {
            problems.push(format!(
                "htmx's indicator rule is not kept as copied: {rule}"
            ));
        }
    }

    let mut switch_px = None;
    let width_groups: Vec<String> = sheet
        .groups
        .iter()
        .map(|group| norm(group))
        .filter(|group| group.contains("width"))
        .collect();
    if width_groups.is_empty() {
        problems.push("it has no media query at the 48rem switch point".to_owned());
    }
    for group in &width_groups {
        if group == SWITCH {
            switch_px = Some(48 * 16);
        } else {
            problems.push(format!(
                "a media query is not the 48rem switch point, {SWITCH}: {group}"
            ));
        }
    }
    let mut root_sizes: Vec<(Vec<String>, String)> = sheet
        .rules
        .iter()
        .filter(|rule| norm(&rule.selector) == "html")
        .filter_map(|rule| {
            normed(rule, "font-size")
                .map(|size| (rule.groups.iter().map(|group| norm(group)).collect(), size))
        })
        .collect();
    root_sizes.sort();
    let expected_sizes = vec![
        (Vec::new(), "100%".to_owned()),
        (vec![SWITCH.to_owned()], "93.75%".to_owned()),
    ];
    if root_sizes != expected_sizes {
        problems.push(format!(
            "the root is not 16 px on a phone and 15 px on a computer (html at 100%, and at 93.75% under {SWITCH} alone): {root_sizes:?}"
        ));
    }
    let points = suite_switch_points();
    if points.is_empty() {
        problems.push("the e2e suite names no switch point".to_owned());
    }
    for (file, point) in &points {
        if Some(*point) != switch_px {
            problems.push(format!(
                "the e2e suite's switch point {point} px in {file} is not the style file's 48rem (768 px)"
            ));
        }
    }

    let in_reduced = |rule: &Rule| {
        rule.groups
            .iter()
            .map(|group| norm(group))
            .collect::<Vec<String>>()
            == [REDUCED]
    };
    let reduced = sheet.rules.iter().any(|rule| {
        in_reduced(rule)
            && norm(&rule.selector) == "*,*::before,*::after"
            && rule.value("transition").map(norm).as_deref() == Some("none!important")
            && rule.value("animation").map(norm).as_deref() == Some("none!important")
    });
    if !reduced {
        problems.push(
            "under prefers-reduced-motion, in no other group, not every transition and animation is off (*, *::before, *::after: transition and animation none !important)"
                .to_owned(),
        );
    }
    for rule in sheet.rules.iter().filter(|rule| !in_reduced(rule)) {
        for (declared, value) in &rule.decls {
            if (declared.starts_with("transition") || declared.starts_with("animation"))
                && norm(value).contains("!important")
            {
                problems.push(format!(
                    "{} marks its {declared} !important, which outranks the reduced-motion rule: {value}",
                    rule.selector
                ));
            }
        }
    }

    match rule_at(
        &sheet,
        &[],
        ":is(a,button,summary,input,select,textarea):focus-visible",
    ) {
        Ok(focus) => {
            if normed(focus, "outline").as_deref() != Some("2pxsolidvar(--n-focus)")
                || normed(focus, "outline-offset").as_deref() != Some("3px")
            {
                problems.push(format!(
                    "keyboard focus is not a 2 px ring in var(--n-focus), 3 px from the control: {:?}",
                    focus.decls
                ));
            }
        }
        Err(problem) => problems.push(format!("keyboard focus: {problem}")),
    }

    let press = sheet
        .rules
        .iter()
        .filter(|rule| rule.groups.is_empty() && norm(&rule.selector) == ":root")
        .filter_map(|rule| normed(rule, "--n-press"))
        .collect::<Vec<String>>();
    if press != ["3px"] {
        problems.push(format!(
            "a button does not press 3 px onto its shadow: --n-press is {press:?}"
        ));
    }
    match rule_at(
        &sheet,
        &[],
        r#"button:active,a.button:active,header.bar details>summary[role="button"]:active"#,
    ) {
        Ok(active) => {
            if normed(active, "transform").as_deref()
                != Some("translate(var(--n-press),var(--n-press))")
            {
                problems.push(format!(
                    "a pressed button does not move by --n-press onto its shadow: {:?}",
                    active.decls
                ));
            }
        }
        Err(problem) => problems.push(format!("the pressed button: {problem}")),
    }

    match rule_at(&sheet, &[], ".amount") {
        Ok(amount) => {
            for (name, expected) in [
                ("font-family", "var(--n-mono)"),
                ("font-variant-numeric", "tabular-nums"),
                ("text-align", "right"),
            ] {
                if normed(amount, name).as_deref() != Some(expected) {
                    problems.push(format!(
                        "amounts are not set with {name} {expected} (Plex Mono, digits of equal width, right-aligned): {:?}",
                        amount.decls
                    ));
                }
            }
        }
        Err(problem) => problems.push(format!("the amounts: {problem}")),
    }

    for rule in &sheet.rules {
        if let Some(content) = rule.value("content") {
            let silent = match find_stop(content, 0, &['/']) {
                Some((slash, _)) => content[slash + 1..].trim() == "\"\"",
                None => content.trim() == "\"\"" || content.trim() == "none",
            };
            if !silent {
                problems.push(format!(
                    "{} draws content {content} that a screen reader would read: a sign's content is empty or carries the alternative text / \"\"",
                    rule.selector
                ));
            }
        }
        for (index, (declared, value)) in rule.decls.iter().enumerate() {
            if declared != "content" {
                continue;
            }
            let Some((slash, _)) = find_stop(value, 0, &['/']) else {
                continue;
            };
            let plain = value[..slash].trim();
            let before = index
                .checked_sub(1)
                .and_then(|previous| rule.decls.get(previous));
            let kept =
                before.is_some_and(|(name, previous)| name == "content" && previous == plain);
            if !kept {
                problems.push(format!(
                    "{} gives content {value} with no plain content {plain} before it",
                    rule.selector
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the style file breaks a rule of the look:\n{}",
        problems.join("\n")
    );
}
