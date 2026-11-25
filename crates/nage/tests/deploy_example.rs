//! `deploy/compose.example.yaml`, the example Compose file of a server (PLAN M2 slice 9, SPEC
//! decisions 188 and 191), read as text (no YAML parser: no new package): the image is this
//! version's, exact; `NAGE_TRUST_PROXY` is the quoted `"true"`; the data folder is a bind that is
//! never created by itself; no `latest`, no `${`, no `build:`, no `healthcheck`, and no
//! `NAGE_DATA_DIR`, `NAGE_BIND` or `NAGE_TZ`; the published container port is the image's; and the
//! file names no address but `127.0.0.1`, no host but the registry's and no zone but `Etc/UTC`.

use std::fs;
use std::path::Path;

/// The workspace version, which the example's image must carry.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The example, relative to the repository's root.
const EXAMPLE: &str = "deploy/compose.example.yaml";

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn example() -> String {
    read(EXAMPLE)
}

/// The example's lines that are not comments or blank, as written.
fn keys(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .collect()
}

/// The example's comment lines, the `#` and the spaces around it removed.
fn comments(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix('#'))
        .map(str::trim)
        .collect()
}

/// The values of every key line `name: value`, trimmed.
fn values<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let prefix = format!("{name}:");
    keys(text)
        .into_iter()
        .filter_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .map(str::trim)
        .collect()
}

/// The list items under the key `name` (its lines that begin with `- `), comments left out.
fn items<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let lines = keys(text);
    let wanted = format!("{name}:");
    let Some(start) = lines.iter().position(|line| line.trim() == wanted) else {
        return Vec::new();
    };
    let indent = lines[start].len() - lines[start].trim_start().len();
    lines[start + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .filter_map(|line| line.trim().strip_prefix("- "))
        .collect()
}

/// The keys of the service `nage`, in their order: its lines four spaces in that are not list
/// items.
fn service_keys(text: &str) -> Vec<&str> {
    let lines = keys(text);
    let Some(start) = lines.iter().position(|line| *line == "  nage:") else {
        panic!("{EXAMPLE} has no service nage");
    };
    lines[start + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() >= 4)
        .filter(|line| line.len() - line.trim_start().len() == 4 && !line.trim().starts_with("- "))
        .filter_map(|line| line.trim().split_once(':').map(|(key, _)| key))
        .collect()
}

/// Asserts that the service holds exactly the keys of decision 191: no command or entrypoint that
/// would move the data folder or the port, no network mode that would make Docker ignore `ports`.
fn assert_service_keys(text: &str) {
    assert_eq!(
        service_keys(text),
        [
            "image",
            "container_name",
            "restart",
            "user",
            "environment",
            "ports",
            "volumes"
        ],
        "the keys of the service nage in {EXAMPLE}"
    );
}

/// The container port of the Dockerfile's `NAGE_BIND`.
fn image_port() -> String {
    let dockerfile = read("Dockerfile");
    let setting = dockerfile
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| line.split_once("NAGE_BIND=").map(|(_, rest)| rest))
        .expect("the Dockerfile sets NAGE_BIND");
    let bind = setting.split_whitespace().next().unwrap_or_default();
    let (_, port) = bind.rsplit_once(':').expect("NAGE_BIND has a port");
    assert!(
        !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()),
        "the Dockerfile's NAGE_BIND has no port: {bind}"
    );
    port.to_owned()
}

/// Every IPv4 address of a text, as written.
fn ipv4_addresses(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for word in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = word.split('.').collect();
        if parts.len() >= 4 && parts.iter().take(4).all(|p| !p.is_empty() && p.len() <= 3) {
            found.push(parts[..4].join("."));
        }
    }
    found
}

/// The example's one image is `ghcr.io/bngoller/nage` at exactly this version.
///
/// Proves: SPEC decisions 165 and 190: the example's one image line is ghcr.io/bngoller/nage and
/// exactly this version
#[test]
fn the_example_runs_this_versions_image_by_its_exact_name() {
    let text = example();
    assert_eq!(
        values(&text, "image"),
        [format!("ghcr.io/bngoller/nage:{VERSION}").as_str()],
        "the image lines of {EXAMPLE}"
    );
}

/// The example gives `NAGE_TRUST_PROXY` the quoted `"true"`.
///
/// Proves: SPEC decision 191: the example's NAGE_TRUST_PROXY line is the quoted true, never a bare
/// true
#[test]
fn the_example_quotes_true_for_the_proxy_setting() {
    let text = example();
    assert_eq!(
        values(&text, "NAGE_TRUST_PROXY"),
        ["\"true\""],
        "the NAGE_TRUST_PROXY lines of {EXAMPLE}"
    );
}

/// The data folder is a long-form bind that Docker never creates.
///
/// Proves: SPEC decision 191: the data folder is a long-form bind of ./data at /data with
/// create_host_path false
#[test]
fn the_example_never_creates_the_data_folder() {
    let text = example();
    let lines: Vec<&str> = keys(&text).into_iter().map(str::trim).collect();
    let bind = [
        "- type: bind",
        "source: ./data",
        "target: /data",
        "bind:",
        "create_host_path: false",
    ];
    assert!(
        lines.windows(bind.len()).any(|w| w == bind),
        "{EXAMPLE} does not bind ./data at /data with create_host_path: false"
    );
    let volumes = items(&text, "volumes");
    assert_eq!(
        volumes,
        ["type: bind"],
        "{EXAMPLE} has another volume than the data folder's bind"
    );
    // The bind holds those lines and nothing else, so nothing makes it read only.
    let raw = keys(&text);
    let dash = raw
        .iter()
        .position(|line| line.trim() == "- type: bind")
        .unwrap_or_else(|| panic!("{EXAMPLE} has no bind"));
    let indent = raw[dash].len() - raw[dash].trim_start().len();
    let entry: Vec<&str> = raw[dash + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .map(|line| line.trim())
        .collect();
    assert_eq!(
        entry,
        bind[1..],
        "the lines of the data folder's bind in {EXAMPLE}"
    );
    // No command, entrypoint or network mode moves the book off the bind.
    assert_service_keys(&text);
    // The folder is made first and handed to uid 1000, in the example's own words and in the
    // guide's first start.
    let said = comments(&text);
    for wanted in ["mkdir -m 0700 data", "sudo chown 1000:1000 data"] {
        assert!(
            said.contains(&wanted),
            "{EXAMPLE} does not say {wanted:?} before the first start"
        );
    }
    assert!(
        read("docs/deploy.md")
            .lines()
            .any(|line| line == "sudo chown 1000:1000 data"),
        "docs/deploy.md does not hand the data folder to uid 1000"
    );
}

/// The example names no moving tag, no variable to fill in and no build.
///
/// Proves: SPEC decisions 165 and 191: the example holds no latest, no variable to fill in and no
/// build key
#[test]
fn the_example_names_no_moving_tag_no_variable_and_no_build() {
    let text = example();
    assert!(!text.contains("latest"), "{EXAMPLE} names latest");
    assert!(
        !text.contains("${"),
        "{EXAMPLE} holds a variable to fill in"
    );
    assert!(!text.contains("$("), "{EXAMPLE} holds a command to fill in");
    assert!(
        values(&text, "build").is_empty(),
        "{EXAMPLE} has a build key"
    );
}

/// The example has no health check of its own, so the image's applies.
///
/// Proves: SPEC decisions 69 and 191: the example has no healthcheck key, so the image's own
/// health check applies
#[test]
fn the_example_keeps_the_images_health_check() {
    let text = example();
    assert!(
        keys(&text)
            .iter()
            .all(|line| !line.to_lowercase().contains("healthcheck")),
        "{EXAMPLE} has a healthcheck key"
    );
}

/// The example publishes 8787 on 127.0.0.1 alone to the image's container port, keeping Nage to
/// the machine itself, and its comment gives the line that opens it to the home network.
///
/// Proves: SPEC decisions 201 and 191: the example's one published port maps 127.0.0.1:8787 to the
/// container port of the Dockerfile's NAGE_BIND, and its comment gives the line on every address
#[test]
fn the_example_publishes_the_images_container_port() {
    let text = example();
    let port = image_port();
    assert_eq!(
        items(&text, "ports"),
        [format!("\"127.0.0.1:8787:{port}\"").as_str()],
        "the published ports of {EXAMPLE}"
    );
    let open = format!("- \"8787:{port}\"");
    assert!(
        comments(&text).contains(&open.as_str()),
        "{EXAMPLE} has no comment giving {open}"
    );
    // No network mode, under which Docker would ignore the published ports.
    assert_service_keys(&text);
}

/// The example sets none of the settings the image sets itself, and no NAGE_TZ.
///
/// Proves: SPEC decision 191: the example sets no NAGE_DATA_DIR, no NAGE_BIND and no NAGE_TZ
#[test]
fn the_example_sets_none_of_the_images_own_settings() {
    let text = example();
    for setting in ["NAGE_DATA_DIR", "NAGE_BIND", "NAGE_TZ"] {
        assert!(
            keys(&text).iter().all(|line| !line.contains(setting)),
            "{EXAMPLE} sets {setting}"
        );
    }
    // Nor does a command line, with --data-dir or --bind.
    assert_service_keys(&text);
}

/// One service `nage`, with the container name, the restart policy, the user and TZ of
/// decision 70.
///
/// Proves: SPEC decisions 70 and 191: one service nage with container_name nage, restart
/// unless-stopped, user 1000:1000 quoted, and TZ set to the quoted placeholder Etc/UTC
#[test]
fn the_example_keeps_the_service_pattern_of_decision_70() {
    let text = example();
    let lines = keys(&text);
    let start = lines
        .iter()
        .position(|line| *line == "services:")
        .unwrap_or_else(|| panic!("{EXAMPLE} has no services key"));
    let services: Vec<&str> = lines[start + 1..]
        .iter()
        .filter(|line| line.len() - line.trim_start().len() == 2)
        .map(|line| line.trim())
        .collect();
    assert_eq!(services, ["nage:"], "the services of {EXAMPLE}");
    assert_eq!(values(&text, "container_name"), ["nage"]);
    assert_eq!(values(&text, "restart"), ["unless-stopped"]);
    assert_eq!(values(&text, "user"), ["\"1000:1000\""]);
    assert_eq!(values(&text, "TZ"), ["\"Etc/UTC\""]);
}

/// The example names no address but 127.0.0.1 and no host but the registry.
///
/// Proves: SPEC decision 195: the only IPv4 address in the example is 127.0.0.1 and the only host
/// name is ghcr.io
#[test]
fn the_example_names_no_address_but_loopback_and_no_host_but_the_registry() {
    let text = example();
    for address in ipv4_addresses(&text) {
        assert_eq!(address, "127.0.0.1", "{EXAMPLE} names an IPv4 address");
    }
    assert!(!text.contains("://"), "{EXAMPLE} holds a web address");
    // Every dotted word whose last part begins with a letter is a host, unless it is a file name
    // of the repository (compose.yaml, deploy.md).
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')) {
        let word = word.trim_matches('.');
        let labels: Vec<&str> = word.split('.').collect();
        let last = labels.last().copied().unwrap_or_default();
        if labels.len() < 2
            || labels.iter().any(|l| l.is_empty())
            || !last.starts_with(|c: char| c.is_ascii_alphabetic())
            || ["yaml", "yml", "md"].contains(&last)
        {
            continue;
        }
        assert_eq!(word, "ghcr.io", "{EXAMPLE} names the host {word}");
    }
    assert!(text.contains("ghcr.io/"), "{EXAMPLE} names no registry");
    // No IPv6 address but ::1: a run of hexadecimal digits and colons holding :: or seven colons.
    for run in text.split(|c: char| !(c.is_ascii_hexdigit() || c == ':')) {
        assert!(
            !(run.contains("::") || run.matches(':').count() >= 7) || run == "::1",
            "{EXAMPLE} names the IPv6 address {run}"
        );
    }
}
