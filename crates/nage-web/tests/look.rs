//! The style file as the look's rules read it (milestone 2 slice 6a): every colour named once
//! for each palette, the contrast of every pair the SPEC row lists recomputed from those named
//! colours, the fonts named at the addresses Nage serves them at, and the rules no browser test
//! measures (no Pico, no outside address, the switch point, reduced motion, the marks); and
//! since milestone 2 slice 7 the owners' colours, the parts of a place in a phone design and a
//! computer design, the outlined button beside the one yellow action and the Accounts sign; and
//! since milestone 2 slice 7a the opening balance's pages in both designs, the tick box in the
//! outline colour, and a section's label that never breaks inside a word beside a wide figure.

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

/// The six owner colours decision 138 adds to decision 123's list: the custom property naming
/// each, its hue, and its tone in the dark and in the light palette. Each paints an owner's pill,
/// its text and its edge, on a ground of the panel mixed with `--n-owner-tint` of that colour.
const OWNERS: [(&str, &str, &str, &str); 6] = [
    ("--n-owner-1", "teal", "#5fd4c8", "#00756c"),
    ("--n-owner-2", "violet", "#b8a2ff", "#6a3fd0"),
    ("--n-owner-3", "blue", "#7fb2ff", "#1f5fc4"),
    ("--n-owner-4", "green", "#a9d46a", "#36700f"),
    ("--n-owner-5", "orange", "#ffa868", "#b04a00"),
    ("--n-owner-6", "orchid", "#f08cd2", "#a3287f"),
];

/// The share of an owner's colour in its pill's ground, by palette (decision 138).
const OWNER_TINT: &str = "--n-owner-tint";

/// Every colour of decision 123's list as decision 138 amends it: [`COLOURS`], then [`OWNERS`].
fn listed_colours() -> impl Iterator<Item = (&'static str, &'static str, &'static str, &'static str)>
{
    COLOURS.into_iter().chain(OWNERS)
}

/// The drawings that cannot be a mask painted in a named colour (decision 123): the custom
/// property holding each `data:` address, the named colour every colour inside it must equal in
/// its palette, and what it draws.
const DRAWN: [(&str, &str, &str); 1] = [("--n-chevron", "--n-quiet", "the select's arrow")];

/// The lengths a palette block names besides its colours (decision 123): the custom property,
/// what it sizes, and its value in the dark and in the light palette. The Net worth block's
/// hard shadow, in the palette's shadow colour, stands 4 px right and below in the light palette
/// and shows nowhere in the dark one. An owner's pill takes 14 percent of its colour in its
/// ground in the dark palette and 8 percent in the light one (decision 138).
const LENGTHS: [(&str, &str, &str, &str); 2] = [
    (
        "--n-total-lift",
        "the offset of the Net worth block's hard shadow",
        "0",
        "4px",
    ),
    (
        OWNER_TINT,
        "the share of an owner's colour in its pill's ground",
        "14%",
        "8%",
    ),
];

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
        for (name, role, dark, light) in listed_colours() {
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
                || listed_colours().any(|(name, ..)| name == declared)
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
                || listed_colours().any(|(name, ..)| name == declared)
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

    for name in listed_colours()
        .map(|(name, ..)| name)
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
        ("header.bar li.accounts>a", "var(--n-sign-accounts)"),
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
            "the menu's signs do not follow their entries' classes (a house for li.home, a ledger for li.accounts, a lock for li.password, a person for li.who, an arrow leaving a door for li.sign-out): {signs:?}"
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

/// The parts of `selector` separated by its top-level commas, each without white space and
/// quotes.
fn selector_parts(selector: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut from = 0;
    while let Some((at, _)) = find_stop(selector, from, &[',']) {
        parts.push(norm(&selector[from..at]));
        from = at + 1;
    }
    parts.push(norm(&selector[from..]));
    parts
}

/// The fraction a percentage such as `14%` stands for.
fn percent(value: &str) -> Option<f64> {
    value
        .trim()
        .strip_suffix('%')?
        .parse::<f64>()
        .ok()
        .map(|share| share / 100.0)
}

/// The `#rrggbb` colour `color-mix(in srgb, <front> <share>, <ground>)` paints: each channel of
/// `front` taken at `share` and of `ground` at the rest.
fn mix(front: &str, share: f64, ground: &str) -> Option<String> {
    let channels = |hex: &str| -> Option<[f64; 3]> {
        let digits = hex.strip_prefix('#')?;
        if digits.len() != 6 {
            return None;
        }
        let mut out = [0.0; 3];
        for (index, slot) in out.iter_mut().enumerate() {
            *slot = f64::from(u8::from_str_radix(digits.get(index * 2..index * 2 + 2)?, 16).ok()?);
        }
        Some(out)
    };
    let (front, ground) = (channels(front)?, channels(ground)?);
    let mut hex = String::from("#");
    for (one, other) in front.iter().zip(ground) {
        let value = (one * share + other * (1.0 - share))
            .round()
            .clamp(0.0, 255.0);
        hex.push_str(&format!("{:02x}", value as u8));
    }
    Some(hex)
}

/// The rule of the main action's yellow in a choice of buttons, moved by the phone design.
const CHOICE_MAIN: &str =
    ".actions.choice>:is(button[type=\"submit\"]:not(.outline),a.button:not(.outline))";

/// One declaration a part of a place holds: what it gives, the rule's selector, the property and
/// its value without white space and quotes.
type Part = (&'static str, &'static str, &'static str, &'static str);

/// The drawing of the sign that a line or a step leads on, as a mask over the quiet ink.
const SIGN_NEXT: &str = "var(--n-sign-next)center/containno-repeat";

/// The phone design of a place's parts, outside every media query (decision 130).
const PHONE: [Part; 35] = [
    (
        "an archived line reads in the quiet ink",
        "ul.rows li.archived :is(.name,.amount)",
        "color",
        "var(--n-quiet)",
    ),
    (
        "the trail's page itself is in the quiet ink",
        "nav.trail [aria-current=\"page\"]",
        "color",
        "var(--n-quiet)",
    ),
    (
        "an account inside another stands one level in",
        "ul.rows li.level-2>a,ul.rows li.other.level-2",
        "padding-left",
        "calc(1rem+var(--n-indent))",
    ),
    (
        "an account two levels in stands two levels in",
        "ul.rows li.level-3>a,ul.rows li.other.level-3",
        "padding-left",
        "calc(1rem+var(--n-indent)*2)",
    ),
    (
        "an account three levels in stands three levels in",
        "ul.rows li.level-4>a,ul.rows li.other.level-4",
        "padding-left",
        "calc(1rem+var(--n-indent)*3)",
    ),
    (
        "an account four levels in stands four levels in",
        "ul.rows li.level-5>a,ul.rows li.other.level-5",
        "padding-left",
        "calc(1rem+var(--n-indent)*4)",
    ),
    (
        "the trail's separator stands between steps, never after the page itself",
        "nav.trail li:not(:last-child)::after",
        "mask",
        SIGN_NEXT,
    ),
    (
        "the trail's separator is the sign, for WebKit too",
        "nav.trail li:not(:last-child)::after",
        "-webkit-mask",
        SIGN_NEXT,
    ),
    (
        "a line's chevron is the sign",
        "ul.rows a::after",
        "mask",
        SIGN_NEXT,
    ),
    (
        "a line's chevron is the sign, for WebKit too",
        "ul.rows a::after",
        "-webkit-mask",
        SIGN_NEXT,
    ),
    (
        "a choice's chevron is the sign",
        "ul.choose a::after",
        "mask",
        SIGN_NEXT,
    ),
    (
        "a choice's chevron is the sign, for WebKit too",
        "ul.choose a::after",
        "-webkit-mask",
        SIGN_NEXT,
    ),
    (
        "an owner's name keeps its capitals",
        ".tag.owner",
        "text-transform",
        "none",
    ),
    (
        "the built-in and archived marks are in the quiet ink",
        ".tag",
        "color",
        "var(--n-quiet)",
    ),
    (
        "what is saved on the clash page is not drawn as a box to type in",
        ".clash dl.facts dd",
        "border",
        "var(--n-outline-width)dashedvar(--n-outline)",
    ),
    (
        "a section's head and its lines are laid out as the panel's lines",
        "section.kind",
        "display",
        "flex",
    ),
    (
        "a section's total stands at the right of its line",
        "section.kind>.panel-head>.amount",
        "margin-left",
        "auto",
    ),
    (
        "a section's lines take a line of the panel of their own",
        "section.kind>:is(ul.rows,p.empty)",
        "flex",
        "00100%",
    ),
    (
        "a section's lines come right after its head",
        "section.kind>:is(ul.rows,p.empty)",
        "order",
        "1",
    ),
    (
        "a section's New line takes a line of the panel of its own",
        "section.kind a.add",
        "flex",
        "00100%",
    ),
    (
        "the trail's links are 44 px targets",
        "nav.trail a",
        "min-height",
        "var(--n-tap)",
    ),
    (
        "the trail's steps wrap",
        "nav.trail ol",
        "flex-wrap",
        "wrap",
    ),
    (
        "a row is the whole line as one link",
        "ul.rows a",
        "display",
        "grid",
    ),
    (
        "a row is 48 px tall at least",
        "ul.rows a",
        "min-height",
        "3rem",
    ),
    (
        "the owner and the marks stand under the name",
        "ul.rows .what",
        "flex-direction",
        "column",
    ),
    (
        "a mark is a small outlined pill",
        ".tag",
        "border-radius",
        "999px",
    ),
    (
        "the choices stand one under another",
        "ul.choose",
        "display",
        "grid",
    ),
    (
        "a choice is a large target",
        "ul.choose a",
        "min-height",
        "4.75rem",
    ),
    (
        "a panel is outlined",
        ".panel",
        "border",
        "var(--n-outline-width)solidvar(--n-outline)",
    ),
    (
        "a section's New line is its panel's last line",
        "section.kind a.add",
        "order",
        "2",
    ),
    (
        "a section's New line is a full-width line",
        "section.kind a.add",
        "min-height",
        "3rem",
    ),
    (
        "a page's buttons stand one under another",
        ".actions",
        "flex-direction",
        "column",
    ),
    (
        "a page's buttons are full width",
        ".actions>:is(button,a.button)",
        "width",
        "calc(100%-var(--n-press))",
    ),
    (
        "a page's buttons are 48 px tall at least",
        ".actions>:is(button,a.button)",
        "min-height",
        "3rem",
    ),
    (
        "a confirmation's main button comes last, nearest the thumb",
        CHOICE_MAIN,
        "order",
        "1",
    ),
];

/// The computer design of a place's parts, from 48rem on (decision 130).
const COMPUTER: [Part; 26] = [
    (
        "a section's New line keeps to its words beside the heading, not a line of its own",
        "section.kind a.add",
        "flex",
        "none",
    ),
    (
        "an account's actions panel stands in the second column",
        ".thing>.doing",
        "grid-column",
        "2",
    ),
    (
        "an account's facts stand two side by side",
        ".thing>dl.facts",
        "grid-template-columns",
        "repeat(2,minmax(0,1fr))",
    ),
    (
        "a problem line beside its box is edged in the red of a refusal",
        "form.sided .field>small.problem",
        "border-left-color",
        "var(--n-not-done)",
    ),
    (
        "a form's label and box stand in the first column",
        "form.sided .field>:is(label,input,select)",
        "grid-column",
        "1",
    ),
    (
        "what is saved now stands beside the person's change",
        ".clash",
        "grid-template-columns",
        "repeat(2,minmax(0,1fr))",
    ),
    (
        "a page's one button stands beside its title",
        "header.head.split",
        "display",
        "flex",
    ),
    (
        "a line shows under the pointer that the whole line is the link",
        "ul.rows a:hover",
        "background",
        "var(--n-raised)",
    ),
    (
        "the trail is compact",
        "nav.trail a",
        "min-height",
        "1.75rem",
    ),
    ("a row is compact", "ul.rows a", "min-height", "2.375rem"),
    (
        "the owner and the marks stand beside the name",
        "ul.rows .what",
        "flex-direction",
        "row",
    ),
    (
        "a mark stands centred on the name's line",
        "ul.rows .what>.tag,ul.rows li:is(.level-2,.level-3,.level-4,.level-5) .what>.tag",
        "align-self",
        "center",
    ),
    (
        "the choices stand two by two",
        "ul.choose",
        "grid-template-columns",
        "repeat(2,minmax(0,1fr))",
    ),
    (
        "Accounts stands beside Categories, Built in below",
        ".groups",
        "grid-template-columns",
        "repeat(2,minmax(0,1fr))",
    ),
    (
        "Built in stands right under Accounts, however long Categories is beside them",
        ".groups",
        "grid-template-rows",
        "auto1fr",
    ),
    (
        "a box with no line beside it is as wide as a box with one, on the create form as on the change form",
        "form.sided .field:not(:has(>small))",
        "grid-template-columns",
        "minmax(0,var(--n-box))",
    ),
    (
        "a panel's head is compact",
        ".panel-head",
        "min-height",
        "2.5rem",
    ),
    (
        "a section's New line stands beside its heading",
        "section.kind a.add",
        "order",
        "0",
    ),
    (
        "a section's total stands after the New line",
        "section.kind>.panel-head>.amount",
        "order",
        "1",
    ),
    (
        "a page's buttons stand in a row",
        ".actions",
        "flex-direction",
        "row",
    ),
    (
        "a page's buttons are as wide as their words",
        ".actions>:is(button,a.button)",
        "width",
        "auto",
    ),
    (
        "a confirmation's main button comes first",
        CHOICE_MAIN,
        "order",
        "0",
    ),
    (
        "an account's actions stand in a column beside its facts",
        ".thing",
        "grid-template-columns",
        "minmax(0,1fr)var(--n-side)",
    ),
    (
        "an account's actions are a column in their panel",
        ".thing>.doing>.actions",
        "flex-direction",
        "column",
    ),
    (
        "a form's box and its lines are two columns",
        "form.sided .field",
        "display",
        "grid",
    ),
    (
        "a form's help and problem lines stand beside the box",
        "form.sided .field>small",
        "grid-column",
        "2",
    ),
];

/// Each of the six owner colours of decision 138 is named once in every palette block with its
/// tone for that palette, beside the share of it a pill's ground takes; the pill paints its text
/// and its edge in the owner's colour on the panel mixed with that share of it, one class per
/// colour; and in every palette block each colour reaches 4.5 to 1 on its pill's ground,
/// recomputed from the named colours, and is neither the yellow of the one action nor the red of
/// a refusal.
///
/// Proves: Decision 138 amending 123: six owner colours named in every palette block, each on its pill's ground at 4.5 to 1 or more.
#[test]
fn the_owner_colours_are_named_in_both_palettes_with_their_contrast() {
    const STUB: &str = "look: the owner colours are not named";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();

    for block in sheet.blocks() {
        let block = match block {
            Ok(block) => block,
            Err(problem) => {
                problems.push(format!("the palette blocks: {problem}"));
                continue;
            }
        };
        let tint = block.rule.value(OWNER_TINT).and_then(percent);
        let panel = block.resolve("--n-panel");
        let (Some(tint), Some(panel)) = (tint, panel) else {
            problems.push(format!(
                "{} names no {OWNER_TINT} as a percentage or no --n-panel",
                block.what
            ));
            continue;
        };
        let forbidden: Vec<String> = ["--n-accent", "--n-accent-hover", "--n-not-done"]
            .into_iter()
            .filter_map(|name| block.resolve(name))
            .collect();
        for (name, hue, dark, light) in OWNERS {
            let expected = if block.light { light } else { dark };
            let values: Vec<&str> = block
                .rule
                .decls
                .iter()
                .filter(|(declared, _)| declared == name)
                .map(|(_, value)| value.as_str())
                .collect();
            if values != [expected] {
                problems.push(format!(
                    "{name} ({hue}) is not named once as {expected} in {}: {values:?}",
                    block.what
                ));
                continue;
            }
            if forbidden.iter().any(|colour| colour == expected) {
                problems.push(format!(
                    "{name} ({hue}) is the yellow of the one action or the red of a refusal in {}: {expected}",
                    block.what
                ));
            }
            let ground = mix(expected, tint, &panel);
            let ratio = ground
                .as_deref()
                .and_then(|ground| contrast(expected, ground));
            match ratio {
                Some(ratio) if ratio >= TEXT => {}
                _ => problems.push(format!(
                    "{name} ({hue} {expected}) on its pill's ground {ground:?} in {}: {ratio:?} to 1, below {TEXT} to 1",
                    block.what
                )),
            }
        }
    }

    match rule_at(&sheet, &[], ".tag.owner") {
        Ok(pill) => {
            if normed(pill, "color").as_deref() != Some("var(--n-own)") {
                problems.push(format!(
                    "an owner's pill does not paint its text in the owner's colour, var(--n-own): {:?}",
                    pill.decls
                ));
            }
            if normed(pill, "background").as_deref()
                != Some("color-mix(insrgb,var(--n-own)var(--n-owner-tint),var(--n-panel))")
            {
                problems.push(format!(
                    "an owner's pill's ground is not the panel mixed with --n-owner-tint of the owner's colour: {:?}",
                    pill.decls
                ));
            }
        }
        Err(problem) => problems.push(format!("an owner's pill: {problem}")),
    }
    match rule_at(&sheet, &[], ".tag") {
        Ok(tag) => {
            if normed(tag, "border").map(|border| border.to_ascii_lowercase())
                != Some("1pxsolidcurrentcolor".to_owned())
            {
                problems.push(format!(
                    "a pill's edge is not drawn in its text's colour: {:?}",
                    tag.decls
                ));
            }
        }
        Err(problem) => problems.push(format!("a pill: {problem}")),
    }
    for (index, (name, hue, ..)) in OWNERS.iter().enumerate() {
        let selector = format!(".tag.owner-{}", index + 1);
        let expected = format!("var({name})");
        match rule_at(&sheet, &[], &selector) {
            Ok(rule) => {
                if normed(rule, "--n-own").as_deref() != Some(expected.as_str()) {
                    problems.push(format!(
                        "{selector} does not take {name} ({hue}) as its owner's colour: {:?}",
                        rule.decls
                    ));
                }
            }
            Err(problem) => problems.push(format!("the {hue} owner's class: {problem}")),
        }
    }

    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// The parts of a place (the trail, the rows, the marks, the choices, the panels and a page's
/// buttons, an account's page and a form's lines) have a phone design outside every media query
/// and a computer design under the 48rem switch point: on a phone 44 px targets, lines and
/// buttons one under another and full width, a section's New line the last line of its panel and
/// a confirmation's main button last; on a computer compact lines, Accounts beside Categories
/// with Built in right under Accounts, a section's New line beside its heading, buttons in a row
/// with the main one first, an account's actions a column beside its facts, a form's help and
/// problem lines beside the box and a box without them as wide as one with them. Both
/// keep the quiet ink of an archived line, the trail's page, the marks; the indents by level;
/// the signs that lead on drawn as masks; and each part of a section in its own place among the
/// panel's lines (since decision 142 lines that wrap, so that a total without room beside its
/// label moves to a line of its own).
///
/// Proves: Decision 130 with P10: the trail, the rows, the tags, the choices, the panels and the actions have a phone design and a computer design from 48rem, the phone's moving the New line and the main button last.
#[test]
fn the_parts_of_a_place_are_styled_for_a_phone_and_a_computer() {
    const STUB: &str = "look: the parts of a place are not styled";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();
    for (design, groups, parts) in [
        ("on a phone", Vec::new(), &PHONE[..]),
        ("on a computer", vec![SWITCH], &COMPUTER[..]),
    ] {
        for &(what, selector, property, expected) in parts {
            match rule_at(&sheet, &groups, selector) {
                Ok(rule) => {
                    if normed(rule, property).as_deref() != Some(expected) {
                        problems.push(format!(
                            "{design}, {what}: {selector} does not set {property} {expected}: {:?}",
                            rule.value(property)
                        ));
                    }
                }
                Err(problem) => problems.push(format!("{design}, {what}: {problem}")),
            }
        }
    }
    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// The selectors whose rules may paint the yellow of the one action as a ground: the page's main
/// button (a form's submit, a link drawn as a button, the submit of a page's buttons) with its
/// hover, the appearance in force and the selection's tint. The ticked box is drawn in the
/// outline colour since decision 141, never on the yellow.
const YELLOW_GROUNDS: [&str; 8] = [
    "main form>button[type=submit]",
    "main form>button[type=submit]:hover",
    "main a.button",
    "main a.button:hover",
    "main .actions>button[type=submit]",
    "main .actions>button[type=submit]:hover",
    "header.bar li.look button[aria-pressed=true]",
    "::selection",
];

/// Beside the page's one yellow action stands a second kind of button, outlined in the text's ink
/// on no ground with the edge's colour and the ordinary hard shadow; the submit of a page's
/// buttons is the yellow action, with the loud edge and shadow and the yellow of a hover; links
/// inside a signed-in page are in the text's ink, underlined in the edge's colour, which no
/// palette makes a yellow; and no rule paints the yellow as a ground but the main action's, the
/// appearance in force and the selection's tint.
///
/// Proves: Decision 123 with 130: an outlined button in the text's ink stands beside the one yellow action, and links inside a page are not yellow.
#[test]
fn only_the_main_action_is_yellow() {
    const STUB: &str = "look: a second kind of button is not styled";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();
    for (selector, wants) in [
        (
            "main :is(a.button,button).outline",
            &[
                ("background", "transparent"),
                ("color", "var(--n-text)"),
                ("border-color", "var(--n-edge)"),
                ("box-shadow", "var(--n-press)var(--n-press)0var(--n-shadow)"),
            ][..],
        ),
        (
            "main .actions>button[type=\"submit\"]",
            &[
                ("background", "var(--n-accent)"),
                ("color", "var(--n-on-accent)"),
                ("border-color", "var(--n-loud-edge)"),
                (
                    "box-shadow",
                    "var(--n-press)var(--n-press)0var(--n-shadow-loud)",
                ),
            ][..],
        ),
        (
            "main .actions>button[type=\"submit\"]:hover",
            &[("background", "var(--n-accent-hover)")][..],
        ),
        (
            "main.content a:not(.button)",
            &[
                ("color", "var(--n-text)"),
                ("text-decoration-color", "var(--n-edge)"),
            ][..],
        ),
    ] {
        match rule_at(&sheet, &[], selector) {
            Ok(rule) => {
                for (property, expected) in wants {
                    if normed(rule, property).as_deref() != Some(*expected) {
                        problems.push(format!(
                            "{selector} does not set {property} {expected}: {:?}",
                            rule.value(property)
                        ));
                    }
                }
            }
            Err(problem) => problems.push(problem),
        }
    }
    for block in sheet.blocks().into_iter().flatten() {
        let yellows: Vec<String> = ["--n-accent", "--n-accent-hover"]
            .into_iter()
            .filter_map(|name| block.resolve(name))
            .collect();
        if block
            .resolve("--n-text")
            .is_none_or(|text| yellows.contains(&text))
        {
            problems.push(format!(
                "the text's ink is missing or a yellow in {}",
                block.what
            ));
        }
    }
    for rule in &sheet.rules {
        let paints = rule.decls.iter().any(|(declared, value)| {
            (declared == "background" || declared == "background-color")
                && (value.contains("var(--n-accent)") || value.contains("var(--n-accent-hover)"))
        });
        if !paints {
            continue;
        }
        for part in selector_parts(&rule.selector) {
            if !YELLOW_GROUNDS.iter().any(|allowed| norm(allowed) == part) {
                problems.push(format!(
                    "{part} paints the yellow of the one action as a ground"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// The Accounts entry's sign, a ledger, is the custom property `--n-sign-accounts`, named once
/// among the named sizes and signs as a drawing in a `data:` address that paints only in black
/// or nothing (a mask); the entry's class `li.accounts` sets the menu's `--n-sign` to it; and the
/// menu's signs are drawn by one rule as a mask over the entry's own ink.
///
/// Proves: Decisions 123 and 137: the Accounts entry's ledger sign is a named custom property tied to its class and painted as a mask.
#[test]
fn the_menu_sign_for_accounts_is_a_named_mask() {
    const STUB: &str = "look: the Accounts sign is not named";
    const SIGN: &str = "--n-sign-accounts";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();

    let named: Vec<&Rule> = sheet
        .rules
        .iter()
        .filter(|rule| rule.decls.iter().any(|(declared, _)| declared == SIGN))
        .collect();
    match named.as_slice() {
        [rule]
            if rule.groups.is_empty()
                && norm(&rule.selector) == ":root"
                && rule.value("color-scheme").is_none()
                && rule
                    .decls
                    .iter()
                    .filter(|(declared, _)| declared == SIGN)
                    .count()
                    == 1 =>
        {
            let value = rule.value(SIGN).unwrap_or("");
            if !value.starts_with("url(\"data:image/svg+xml,") {
                problems.push(format!("{SIGN} is not a drawing in a data address: {value}"));
            }
            let (addresses, _) = urls(value);
            for address in &addresses {
                for (attribute, paint) in paints(address) {
                    if paint != "black" && paint != "none" {
                        problems.push(format!(
                            "{SIGN} paints {attribute}='{paint}', not a mask's black or none"
                        ));
                    }
                }
            }
            if !hex_colours(value).is_empty() {
                problems.push(format!("{SIGN} writes a colour of its own: {value}"));
            }
        }
        _ => problems.push(format!(
            "{SIGN} is not named once among the named sizes and signs (:root, no palette): {} rule(s) name it",
            named.len()
        )),
    }

    match rule_at(&sheet, &[], "header.bar li.accounts>a") {
        Ok(rule) => {
            if normed(rule, "--n-sign").as_deref() != Some("var(--n-sign-accounts)") {
                problems.push(format!(
                    "li.accounts does not take var({SIGN}) as its sign: {:?}",
                    rule.decls
                ));
            }
        }
        Err(problem) => problems.push(format!("the Accounts entry: {problem}")),
    }

    let drawing: Vec<&Rule> = sheet
        .rules
        .iter()
        .filter(|rule| {
            rule.groups.is_empty()
                && selector_parts(&rule.selector)
                    .iter()
                    .any(|part| part == "header.barli>a::before")
        })
        .collect();
    match drawing.as_slice() {
        [rule] => {
            for property in ["mask", "-webkit-mask"] {
                if !normed(rule, property).is_some_and(|mask| mask.starts_with("var(--n-sign)")) {
                    problems.push(format!(
                        "the menu's signs are not drawn as a mask of var(--n-sign) by {property}: {:?}",
                        rule.value(property)
                    ));
                }
            }
            if normed(rule, "background").as_deref() != Some("currentColor") {
                problems.push(format!(
                    "the menu's signs are not painted in the entry's own ink: {:?}",
                    rule.value("background")
                ));
            }
        }
        _ => problems.push(format!(
            "the menu's signs are not drawn by one rule for header.bar li>a::before: {} rule(s)",
            drawing.len()
        )),
    }

    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// Checks each part of `parts` against the rule of its selector standing in exactly `groups`,
/// pushing what is wrong, prefixed with `design`, onto `problems`.
fn check_parts(
    sheet: &Sheet,
    design: &str,
    groups: &[&str],
    parts: &[Part],
    problems: &mut Vec<String>,
) {
    for &(what, selector, property, expected) in parts {
        match rule_at(sheet, groups, selector) {
            Ok(rule) => {
                if normed(rule, property).as_deref() != Some(expected) {
                    problems.push(format!(
                        "{design}, {what}: {selector} does not set {property} {expected}: {:?}",
                        rule.value(property)
                    ));
                }
            }
            Err(problem) => problems.push(format!("{design}, {what}: {problem}")),
        }
    }
}

/// The size a `font-size` of `rem` units stands for, in rem.
fn rem(value: &str) -> Option<f64> {
    value.trim().strip_suffix("rem")?.parse::<f64>().ok()
}

/// The phone design of the opening balance's pages, outside every media query (decisions 130
/// and 141 to 144).
const OPENING_PHONE: [Part; 34] = [
    (
        "the date box shows its day at the left like every other box",
        "input[type=date]::-webkit-date-and-time-value",
        "text-align",
        "left",
    ),
    (
        "the confirmation's line after a typed zero stands out",
        "section.confirm>p.typed",
        "font-weight",
        "600",
    ),
    (
        "the warning stands as one block with a bar in the existing second colour",
        ".caution",
        "border-left",
        "4pxsolidvar(--n-violet)",
    ),
    (
        "the warning's example is in the quiet ink",
        ".caution>p.example",
        "color",
        "var(--n-quiet)",
    ),
    (
        "the opening balance's day stands under its figure",
        "dl.facts>div.opening dd",
        "flex-direction",
        "column",
    ),
    (
        "the opening balance's figure and its day stand at the right",
        "dl.facts>div.opening dd",
        "align-items",
        "flex-end",
    ),
    (
        "the opening balance's day is in the quiet ink",
        "dl.facts>div.opening dd>.on",
        "color",
        "var(--n-quiet)",
    ),
    (
        "None yet is in the quiet ink",
        "dl.facts dd.none",
        "color",
        "var(--n-quiet)",
    ),
    (
        "a fact's figure moves to a line of its own when it has no room beside its label",
        "dl.facts>div",
        "flex-wrap",
        "wrap",
    ),
    (
        "a fact's figure stands at the right, also on a line of its own",
        "dl.facts dd",
        "margin-left",
        "auto",
    ),
    (
        "Remove stands under Save",
        ".actions",
        "flex-direction",
        "column",
    ),
    (
        "the way back comes last, after Remove",
        ".actions>a.back",
        "order",
        "1",
    ),
    (
        "the tick box stands apart from the amount box above it",
        ".field>:is(input,small)~label.tick",
        "margin-top",
        ".875rem",
    ),
    (
        "the confirmation's figure stands in a dashed box",
        "section.confirm>p.gone",
        "border",
        "var(--n-outline-width)dashedvar(--n-outline)",
    ),
    (
        "the confirmation's figure never widens the card",
        "section.confirm>p.gone",
        "overflow-wrap",
        "anywhere",
    ),
    (
        "a clash's No opening balance is in the quiet ink",
        ".clash p.nothing",
        "color",
        "var(--n-quiet)",
    ),
    (
        "the confirmation's and a clash's yellow button comes last, nearest the thumb",
        CHOICE_MAIN,
        "order",
        "1",
    ),
    (
        "the opening balance's figure is lighter than the Balance",
        "dl.facts>div.opening dd>.amount",
        "font-weight",
        "400",
    ),
    (
        "the warning is a filled block on the raised ground",
        ".caution",
        "background",
        "var(--n-raised)",
    ),
    (
        "the warning's corners are rounded beside its bar",
        ".caution",
        "border-radius",
        "0var(--n-control-radius)var(--n-control-radius)0",
    ),
    (
        "the warning holds its sentences clear of its edges",
        ".caution",
        "padding",
        ".875rem1rem.875rem1.125rem",
    ),
    (
        "the confirmation's figure lays its parts out on lines",
        "section.confirm>p.gone",
        "display",
        "flex",
    ),
    (
        "the confirmation's figure lets its parts wrap",
        "section.confirm>p.gone",
        "flex-wrap",
        "wrap",
    ),
    (
        "the confirmation's place stands on a line of its own",
        "section.confirm>p.gone>.where",
        "flex",
        "10100%",
    ),
    (
        "the confirmation's place is in the quiet ink",
        "section.confirm>p.gone>.where",
        "color",
        "var(--n-quiet)",
    ),
    (
        "the confirmation's place is small",
        "section.confirm>p.gone>.where",
        "font-size",
        ".875rem",
    ),
    (
        "the confirmation's figure is large",
        "section.confirm>p.gone>.amount",
        "font-size",
        "1.375rem",
    ),
    (
        "the confirmation's figure is bold",
        "section.confirm>p.gone>.amount",
        "font-weight",
        "600",
    ),
    (
        "the confirmation's day beside the figure is in the quiet ink",
        "section.confirm>p.gone>.on",
        "color",
        "var(--n-quiet)",
    ),
    (
        "a clash's saved figure is at the regular weight",
        ".clash dl.facts dd.amount",
        "font-weight",
        "400",
    ),
    (
        "a clash's saved figure is as large as a box's figure",
        ".clash dl.facts dd.amount",
        "font-size",
        "1rem",
    ),
    (
        "a line divides the boxes of the person's change, as under Saved now",
        ".clash .field+.field",
        "border-top",
        "1pxsolidvar(--n-hair)",
    ),
    (
        "a typed amount is in the figure letters",
        ".field>input[inputmode=decimal]",
        "font-family",
        "var(--n-mono)",
    ),
    (
        "every figure is in the figure letters",
        ".amount",
        "font-family",
        "var(--n-mono)",
    ),
];

/// The computer design of the opening balance's pages, from 48rem on (decisions 130 and 141 to
/// 144).
const OPENING_COMPUTER: [Part; 14] = [
    (
        "the opening balance's day stands beside its figure on one line",
        ".thing>dl.facts>div.opening dd",
        "flex-direction",
        "row",
    ),
    (
        "the opening balance's figure and its day share a baseline",
        ".thing>dl.facts>div.opening dd",
        "align-items",
        "baseline",
    ),
    (
        "the warning has more room inside it",
        ".caution",
        "padding",
        "1rem1.25rem",
    ),
    (
        "Remove stands at the far right of the form's row",
        ".actions>:is(button,a.button).apart",
        "margin-left",
        "auto",
    ),
    (
        "the way back stands beside Save",
        ".actions>a.back",
        "order",
        "0",
    ),
    (
        "Save, the way back and Remove stand in a row",
        ".actions",
        "flex-direction",
        "row",
    ),
    (
        "the opening balance takes a row of the facts of its own",
        ".thing>dl.facts>div.opening",
        "grid-column",
        "1/-1",
    ),
    (
        "the opening balance's row is ruled off above",
        ".thing>dl.facts>div.opening",
        "border-top",
        "1pxsolidvar(--n-hair)",
    ),
    (
        "the opening balance's row has no rule at its left",
        ".thing>dl.facts>div.opening",
        "border-left",
        "0",
    ),
    (
        "every fact's figure stands at the left of its column",
        ".thing>dl.facts dd",
        "margin-left",
        "0",
    ),
    (
        "the confirmation is a narrow card",
        "section.confirm",
        "max-width",
        "var(--n-confirm)",
    ),
    (
        "the confirmation's and a clash's yellow button comes first",
        CHOICE_MAIN,
        "order",
        "0",
    ),
    (
        "what is saved now stands beside the person's change",
        ".clash",
        "grid-template-columns",
        "repeat(2,minmax(0,1fr))",
    ),
    (
        "the form's help and problem lines stand beside the box",
        "form.sided .field>small",
        "grid-column",
        "2",
    ),
];

/// The opening balance's pages have a phone design outside every media query and a computer
/// design under the 48rem switch point, the look picked on the prototype's pictures. The form's
/// warning is one filled block on the raised ground with rounded corners and room inside it, a
/// bar in the existing second colour, the violet, at its left, its example in the quiet ink; a
/// typed amount and every figure are in the figure letters; on an account's page the fact
/// Opening balance shows its figure smaller than the Balance, at the regular weight on a phone,
/// with its day under it on a phone and beside it on one line on a computer, None yet in the
/// quiet ink, and a figure without room beside its label moves to a line of its own at the
/// right; on a phone Remove stands under Save, full width with no margin of its own, and the
/// way back comes last, on a computer Save and the way back stand in a row with Remove at its far
/// right, by a rule that outweighs the phone's margin of a page's buttons; the tick box stands
/// apart from the amount box above it in both designs; the date box shows its day at the left;
/// the line after a typed zero stands out; and the fact takes a row of
/// the facts of its own; the confirmation's figure stands in a dashed box, the account's full
/// place small and quiet on a line of its own, the figure large and bold under it with its day
/// quiet beside it, the card narrow on a computer, its yellow button last on a phone and first
/// on a computer; a clash's saved figure is at the regular weight and the size of a box's, a line
/// divides the boxes of the person's change as it divides what is saved, and the panels stand
/// one above the other on a phone and side by side on a computer.
///
/// Proves: Decisions 130, 141, 143 and 144 with the hand check's H1: the fact Opening balance with its day under its figure on a phone and beside it on a computer, the filled warning with its bar in the existing second colour, the amount box in the figure letters, Remove at the far right of the form's row on a computer and under Save on a phone, the confirmation's dashed figure with the place on a line of its own, and the clash panels with a line between the boxes have both designs.
#[test]
fn the_opening_balance_pages_are_styled_for_a_phone_and_a_computer() {
    const STUB: &str = "look: the opening-balance pages are not styled";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();
    check_parts(&sheet, "on a phone", &[], &OPENING_PHONE, &mut problems);
    check_parts(
        &sheet,
        "on a computer",
        &[SWITCH],
        &OPENING_COMPUTER,
        &mut problems,
    );

    for (design, groups, balance, opening) in [
        (
            "on a phone",
            Vec::new(),
            "dl.facts dd.amount",
            "dl.facts>div.opening dd>.amount",
        ),
        (
            "on a computer",
            vec![SWITCH],
            ".thing>dl.facts dd.amount",
            ".thing>dl.facts>div.opening dd>.amount",
        ),
    ] {
        let size = |selector: &str| {
            rule_at(&sheet, &groups, selector)
                .ok()
                .and_then(|rule| rule.value("font-size"))
                .and_then(rem)
        };
        match (size(balance), size(opening)) {
            (Some(balance), Some(opening)) if opening < balance => {}
            (balance_size, opening_size) => problems.push(format!(
                "{design}, the opening balance's figure is not smaller than the Balance: {opening} at {opening_size:?} rem, {balance} at {balance_size:?} rem"
            )),
        }
    }

    // On a phone Remove is a full-width button under Save: no rule outside the switch gives a
    // button that stands apart a margin of its own.
    for rule in sheet.rules.iter().filter(|rule| {
        rule.groups.is_empty()
            && selector_parts(&rule.selector)
                .iter()
                .any(|part| part.contains(".apart"))
    }) {
        for property in [
            "margin-left",
            "margin",
            "margin-inline-start",
            "margin-inline",
        ] {
            if let Some(value) = rule.value(property) {
                problems.push(format!(
                    "on a phone, Remove does not stand full width under Save: {} sets {property} {value}",
                    rule.selector
                ));
            }
        }
    }
    // The tick box stands apart from the amount box above it in both designs: no rule for the
    // tick box's line sets its top margin otherwise.
    for rule in sheet.rules.iter().filter(|rule| {
        selector_parts(&rule.selector)
            .iter()
            .any(|part| part.ends_with("label.tick"))
    }) {
        if rule.value("margin").is_some() {
            problems.push(format!(
                "{} sets the tick box's margin in {:?}: {:?}",
                rule.selector,
                rule.groups,
                rule.value("margin")
            ));
        }
        if let Some(value) = normed(rule, "margin-top")
            && value != ".875rem"
        {
            problems.push(format!(
                "the tick box crowds the amount box above it: {} sets margin-top {value} in {:?}",
                rule.selector, rule.groups
            ));
        }
    }

    for rule in sheet
        .rules
        .iter()
        .filter(|rule| rule.groups.is_empty() && norm(&rule.selector) == ".clash")
    {
        if rule.value("display").is_some() || rule.value("grid-template-columns").is_some() {
            problems.push(format!(
                "on a phone, what is saved now does not stand above the person's change: .clash sets {:?}",
                rule.decls
            ));
        }
    }

    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// The tick box with its words is a target of at least 44 by 44 in both designs: the label a
/// line of that height at least, the box itself that wide and that tall; and it is drawn in the
/// outline colour of a button that is not the page's one action, never in the yellow of the one
/// action: the tick in the outlined button's ink, the box's edge in its edge, on the box's own
/// ground, a pair the row lists, in every rule that paints the box or draws the tick, the large
/// box's included; the tick is the sign of a tick, hidden until the box is ticked and shown when
/// it is; every colour its rules name is one of the row's, none of them
/// the accent, the accent on hover or the ink on the accent, nor a colour that is the accent's
/// yellow in either palette (the dark palette's link, the light palette's entry in view), and
/// nothing else they name is a colour.
///
/// Proves: Decisions 123 and 141: the tick box with its words is a target of at least 44 by 44 and is drawn in the outline colour, never in the yellow of the one action; no new colour.
#[test]
fn the_tick_box_is_drawn_in_the_outline_colour() {
    const STUB: &str = "look: the tick box is not styled";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();

    let taps: Vec<String> = sheet
        .rules
        .iter()
        .filter(|rule| rule.groups.is_empty() && norm(&rule.selector) == ":root")
        .filter_map(|rule| normed(rule, "--n-tap"))
        .collect();
    if taps != ["44px"] {
        problems.push(format!("--n-tap is not 44px once: {taps:?}"));
    }
    for (selector, properties) in [
        ("label.tick", &["min-height"][..]),
        (
            "label.tick input[type=\"checkbox\"]",
            &["min-width", "min-height"][..],
        ),
    ] {
        match rule_at(&sheet, &[], selector) {
            Ok(rule) => {
                for property in properties {
                    if normed(rule, property).as_deref() != Some("var(--n-tap)") {
                        problems.push(format!(
                            "{selector} does not set {property} var(--n-tap): {:?}",
                            rule.value(property)
                        ));
                    }
                }
            }
            Err(problem) => problems.push(format!("the tick box's target: {problem}")),
        }
    }
    for rule in sheet.rules.iter().filter(|rule| {
        !rule.groups.is_empty()
            && selector_parts(&rule.selector)
                .iter()
                .any(|part| part.contains("label.tick"))
    }) {
        for property in ["min-height", "min-width", "height", "width"] {
            if let Some(value) = normed(rule, property)
                && value != "var(--n-tap)"
            {
                problems.push(format!(
                    "{} makes the tick box's target {property} {value} on a computer",
                    rule.selector
                ));
            }
        }
    }

    let outline = rule_at(&sheet, &[], "main :is(a.button,button).outline");
    let (ink, edge) = match outline {
        Ok(rule) => (normed(rule, "color"), normed(rule, "border-color")),
        Err(problem) => {
            problems.push(format!("the outlined button: {problem}"));
            (None, None)
        }
    };
    let by_name = |value: &str| {
        value
            .strip_prefix("var(")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(str::to_owned)
    };
    let mut grounds: Vec<(String, Option<String>)> = Vec::new();
    match rule_at(&sheet, &[], "input[type=checkbox]") {
        Ok(rule) => {
            let border = normed(rule, "border").unwrap_or_default();
            if edge.as_deref().is_none_or(|edge| !border.ends_with(edge)) {
                problems.push(format!(
                    "the tick box's edge is not the outlined button's edge {edge:?}: {border}"
                ));
            }
            grounds.push((rule.selector.clone(), normed(rule, "background-color")));
        }
        Err(problem) => problems.push(format!("the tick box: {problem}")),
    }
    // Every rule that paints the box itself, the large box's and the ticked box's included, gives
    // it a ground the tick's ink stands on as a pair the row lists.
    for rule in sheet.rules.iter().filter(|rule| {
        selector_parts(&rule.selector)
            .iter()
            .any(|part| part.contains("checkbox") && !part.contains("::"))
    }) {
        for property in ["background-color", "background"] {
            if rule.value(property).is_some() && norm(&rule.selector) != "input[type=checkbox]" {
                grounds.push((rule.selector.clone(), normed(rule, property)));
            }
        }
    }
    match rule_at(&sheet, &[], "input[type=checkbox]::before") {
        Ok(rule) => {
            let tick = normed(rule, "background");
            if tick.is_none() || tick != ink {
                problems.push(format!(
                    "the tick is not drawn in the outlined button's ink {ink:?}: {tick:?}"
                ));
            }
            if normed(rule, "visibility").as_deref() != Some("hidden") {
                problems.push(format!(
                    "an unticked box shows its tick: input[type=checkbox]::before sets visibility {:?}",
                    rule.value("visibility")
                ));
            }
            for property in ["mask", "-webkit-mask"] {
                if !normed(rule, property)
                    .is_some_and(|mask| mask.starts_with("var(--n-sign-check)"))
                {
                    problems.push(format!(
                        "the tick is not the sign of a tick, var(--n-sign-check): {property} {:?}",
                        rule.value(property)
                    ));
                }
            }
        }
        Err(problem) => problems.push(format!("the tick: {problem}")),
    }
    match rule_at(&sheet, &[], "input[type=checkbox]:checked::before") {
        Ok(rule) if normed(rule, "visibility").as_deref() == Some("visible") => {}
        Ok(rule) => problems.push(format!(
            "a ticked box shows no tick: input[type=checkbox]:checked::before sets visibility {:?}",
            rule.value("visibility")
        )),
        Err(problem) => problems.push(format!("the ticked box's tick: {problem}")),
    }
    // Every rule that draws the tick, the large box's included, paints it in the ink and draws
    // the sign of a tick.
    for rule in sheet.rules.iter().filter(|rule| {
        selector_parts(&rule.selector)
            .iter()
            .any(|part| part.contains("checkbox") && part.ends_with("::before"))
    }) {
        for property in ["background", "background-color"] {
            if let Some(value) = normed(rule, property)
                && Some(&value) != ink.as_ref()
            {
                problems.push(format!(
                    "{} paints the tick in {value}, not the outlined button's ink {ink:?}",
                    rule.selector
                ));
            }
        }
        for property in ["mask", "-webkit-mask", "mask-image", "-webkit-mask-image"] {
            if let Some(value) = normed(rule, property)
                && !value.starts_with("var(--n-sign-check)")
            {
                problems.push(format!(
                    "{} draws another sign than the tick: {property} {value}",
                    rule.selector
                ));
            }
        }
    }
    for (selector, ground) in &grounds {
        let pair = ink
            .as_deref()
            .and_then(by_name)
            .zip(ground.as_deref().and_then(by_name));
        let listed = pair.as_ref().is_some_and(|(front, back)| {
            PAIRS
                .iter()
                .any(|(listed_front, listed_back, ..)| listed_front == front && listed_back == back)
        });
        if !listed {
            problems.push(format!(
                "the tick on the ground {selector} gives the box is not a pair the row lists: {ink:?} on {ground:?}"
            ));
        }
    }

    let sizes: Vec<&Rule> = sheet
        .rules
        .iter()
        .filter(|rule| {
            rule.groups.is_empty()
                && norm(&rule.selector) == ":root"
                && rule.value("color-scheme").is_none()
        })
        .collect();
    let yellows = ["--n-accent", "--n-accent-hover", "--n-on-accent"];
    let blocks: Vec<Block<'_>> = sheet.blocks().into_iter().flatten().collect();
    let yellow_in = |block: &Block<'_>, name: &str| {
        block.resolve(name).is_some_and(|value| {
            ["--n-accent", "--n-accent-hover"]
                .into_iter()
                .filter_map(|yellow| block.resolve(yellow))
                .any(|yellow| yellow == value)
        })
    };
    for rule in sheet.rules.iter().filter(|rule| {
        selector_parts(&rule.selector)
            .iter()
            .any(|part| part.contains("checkbox") || part.contains("label.tick"))
    }) {
        for (declared, value) in &rule.decls {
            for (at, _) in value.match_indices("var(") {
                let name: String = value[at + 4..]
                    .chars()
                    .take_while(|ch| *ch != ')' && *ch != ',')
                    .collect();
                let name = name.trim();
                if yellows.contains(&name) {
                    problems.push(format!(
                        "{} draws the tick box in the yellow of the one action: {declared} {value}",
                        rule.selector
                    ));
                } else if let Some(block) = blocks.iter().find(|block| yellow_in(block, name)) {
                    problems.push(format!(
                        "{} draws the tick box in {name}, the yellow of the one action in {}: {declared} {value}",
                        rule.selector, block.what
                    ));
                } else if !listed_colours().any(|(listed, ..)| listed == name) {
                    let size = sizes.iter().find_map(|sizes| sizes.value(name));
                    match size {
                        Some(size) if hex_colours(size).is_empty() => {}
                        _ => problems.push(format!(
                            "{} names {name}, neither a colour of the row nor a named size or sign: {declared} {value}",
                            rule.selector
                        )),
                    }
                }
            }
        }
    }

    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}

/// The places where a section's label stands beside its figure (decision 142): on Home a
/// section's heading and Net worth, on the list a section's heading. Each: what it is, the rule
/// that lays out the label and the figure, the label's rule and the figure's rule.
const LABELS: [(&str, &str, &str, &str); 2] = [
    (
        "on Home, a section's heading and Net worth",
        "section.balances h2,p.net-worth",
        "section.balances h2 .name,p.net-worth .name",
        "section.balances h2 .amount,p.net-worth .amount",
    ),
    (
        "on the list, a section's heading",
        "section.kind",
        "section.kind>.panel-head>h3",
        "section.kind>.panel-head>.amount",
    ),
];

/// On Home and on the list the label of a section's heading and Net worth never break inside a
/// word: each label wraps only between words and is never made narrower than its longest word
/// (where a rule for every name sets min-width 0, the label's own rule sets it back to auto);
/// the label and its figure stand in lines that wrap, so that a figure without room beside its
/// label moves to a line of its own, at the right; no rule in either design undoes it, a later
/// rule naming one of the same parts included. An
/// account's name keeps its wrapping, inside a word too (139), on Home and on the list, and a
/// figure never wraps.
///
/// Proves: Decision 142 with 130 and the pause's P8: on Home and on the list the label of a section's heading and Net worth never break inside a word, and a figure without room beside its label moves to a line of its own at the right, while an account's name keeps its wrapping, inside a word too (139).
#[test]
fn a_section_label_never_breaks_inside_a_word_and_its_figure_moves_below() {
    const STUB: &str = "look: a long name is not styled";
    let sheet = Sheet::load();
    let mut problems: Vec<String> = Vec::new();

    for (what, line, label, figure) in LABELS {
        let mut wants: Vec<(&str, &str, &str)> = vec![
            (line, "flex-wrap", "wrap"),
            (label, "overflow-wrap", "normal"),
            (label, "word-break", "normal"),
            (figure, "margin-left", "auto"),
        ];
        if line == "section.kind" {
            wants.push((line, "display", "flex"));
        }
        for (selector, property, expected) in wants {
            match rule_at(&sheet, &[], selector) {
                Ok(rule) => {
                    if normed(rule, property).as_deref() != Some(expected) {
                        problems.push(format!(
                            "{what}: {selector} does not set {property} {expected}: {:?}",
                            rule.value(property)
                        ));
                    }
                }
                Err(problem) => problems.push(format!("{what}: {problem}")),
            }
            for rule in sheet
                .rules
                .iter()
                .filter(|rule| !rule.groups.is_empty() && norm(&rule.selector) == norm(selector))
            {
                if let Some(value) = normed(rule, property)
                    && value != expected
                {
                    problems.push(format!(
                        "{what}: {selector} sets {property} {value} in {:?}, undoing {expected}",
                        rule.groups
                    ));
                }
            }
            // A later rule naming one of the same parts weighs as much and wins, in either
            // design: none may undo the declaration, by its own name or by the margin shorthand.
            if let Ok(base) = rule_at(&sheet, &[], selector) {
                let parts = selector_parts(selector);
                let at = sheet
                    .rules
                    .iter()
                    .position(|rule| std::ptr::eq(rule, base))
                    .unwrap_or(usize::MAX);
                for rule in sheet
                    .rules
                    .iter()
                    .skip(at.saturating_add(1))
                    .filter(|rule| {
                        selector_parts(&rule.selector)
                            .iter()
                            .any(|part| parts.contains(part))
                    })
                {
                    if let Some(value) = normed(rule, property)
                        && value != expected
                    {
                        problems.push(format!(
                            "{what}: the later {} sets {property} {value} in {:?}, undoing {expected}",
                            rule.selector, rule.groups
                        ));
                    }
                    if property == "margin-left"
                        && let Some(value) = rule.value("margin")
                    {
                        problems.push(format!(
                            "{what}: the later {} sets margin {value} in {:?}, undoing {property} {expected}",
                            rule.selector, rule.groups
                        ));
                    }
                }
            }
        }
        for rule in sheet
            .rules
            .iter()
            .filter(|rule| norm(&rule.selector) == norm(label))
        {
            if normed(rule, "min-width").as_deref() == Some("0") {
                problems.push(format!(
                    "{what}: {label} may be made narrower than its longest word (min-width 0) in {:?}",
                    rule.groups
                ));
            }
        }
        let ends = |selector: &str| -> Vec<String> {
            selector
                .split(',')
                .filter_map(|part| part.split_whitespace().next_back())
                .filter_map(|compound| compound.rsplit('>').next())
                .map(str::to_owned)
                .collect()
        };
        let label_ends = ends(label);
        let squeezed: Vec<&str> = sheet
            .rules
            .iter()
            .filter(|rule| {
                norm(&rule.selector) != norm(label)
                    && normed(rule, "min-width").as_deref() == Some("0")
                    && ends(&rule.selector)
                        .iter()
                        .any(|end| label_ends.contains(end))
            })
            .map(|rule| rule.selector.as_str())
            .collect();
        if !squeezed.is_empty() {
            match rule_at(&sheet, &[], label) {
                Ok(rule) if normed(rule, "min-width").as_deref() == Some("auto") => {}
                Ok(rule) => problems.push(format!(
                    "{what}: {squeezed:?} set min-width 0 on the label's kind of element and {label} does not set it back to auto: {:?}",
                    rule.value("min-width")
                )),
                Err(problem) => problems.push(format!("{what}: {problem}")),
            }
        }
    }

    for (what, selector) in [
        (
            "an account's name on Home",
            "section.balances .name,p.net-worth .name",
        ),
        ("an account's name on the list", "ul.rows .name"),
    ] {
        match rule_at(&sheet, &[], selector) {
            Ok(rule) => {
                if normed(rule, "overflow-wrap").as_deref() != Some("anywhere") {
                    problems.push(format!(
                        "{what} no longer wraps inside a word: {selector} sets overflow-wrap {:?}",
                        rule.value("overflow-wrap")
                    ));
                }
            }
            Err(problem) => problems.push(format!("{what}: {problem}")),
        }
    }
    for rule in &sheet.rules {
        let names_a_line = selector_parts(&rule.selector).iter().any(|part| {
            (part.contains("li") || part.contains("ul.rows")) && part.ends_with(".name")
        });
        if names_a_line && normed(rule, "overflow-wrap").is_some_and(|value| value != "anywhere") {
            problems.push(format!(
                "an account's name no longer wraps inside a word: {} sets overflow-wrap {:?}",
                rule.selector,
                rule.value("overflow-wrap")
            ));
        }
    }
    match rule_at(&sheet, &[], ".amount") {
        Ok(rule) if normed(rule, "white-space").as_deref() == Some("nowrap") => {}
        Ok(rule) => problems.push(format!(
            "a figure may wrap: .amount sets white-space {:?}",
            rule.value("white-space")
        )),
        Err(problem) => problems.push(format!("the figures: {problem}")),
    }

    assert!(problems.is_empty(), "{STUB}:\n{}", problems.join("\n"));
}
