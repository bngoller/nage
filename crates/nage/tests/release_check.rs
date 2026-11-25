//! `scripts/release-check.sh`, the release's checks of its tag and the Release page's text (PLAN
//! M2 slice 9, SPEC decisions 165, 167, 168, 171 and 193), run as a process with bash: `version`
//! prints the workspace version; `tag <tag> <commit>` accepts a tag that is `v` and that version,
//! a version of three whole numbers, annotated on `origin` under exactly `refs/tags/<tag>`, naming
//! a commit that is the one `origin` reports for `refs/heads/main` or one of its ancestors, and
//! refuses every other in the three labelled lines, the first broken rule only; `notes <tag>
//! <image> <guide>` prints the tag's own message as `origin` holds it and four fixed lines; a
//! remote that cannot be asked and a `Cargo.toml` without a version get three lines of their own;
//! any other use exits 2. The script's header lists every refusal word for word.
//!
//! Each test copies the script into a temporary repository of its own, with its own `Cargo.toml`
//! and a bare `origin` beside it, and runs git there with a fixed identity, fixed dates, `TZ=UTC`
//! and none of the machine's own git settings. A run of each subcommand leaves that repository's
//! refs, `FETCH_HEAD` and objects as they were.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nage_core::test_support::TestDir;

/// The script under test, as the tree holds it.
const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/release-check.sh"
);

/// The workspace version of the temporary repositories, and its tag.
const VERSION: &str = "0.2.0";
const TAG: &str = "v0.2.0";

/// The image with its digest and the guide's address, as release.yml hands them to `notes`.
const IMAGE: &str = concat!(
    "ghcr.io/bngoller/nage@sha256:",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
);
const GUIDE: &str = "https://github.com/bngoller/nage/blob/v0.2.0/docs/deploy.md";

/// The annotated tag's message on origin.
const MESSAGE: &str = "Nage 0.2.0\n\nThe first version you can run at home.";

/// The fixed date of every commit and tag the tests make.
const DATE: &str = "2026-01-01T00:00:00Z";

/// The Error lines of the three subcommands.
const VERSION_ERROR: &str = "The version was not read.";
const TAG_ERROR: &str = "Nothing was published.";
const NOTES_ERROR: &str = "The Release page's text was not written.";

/// A `Cargo.toml` whose `[workspace.package]` table holds `version`.
fn cargo_toml(version: &str) -> String {
    format!(
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{version}\"\nedition = \"2024\"\n"
    )
}

/// The three labelled lines of a refusal, as stderr carries them.
fn refusal(error: &str, why: &str, what_now: &str) -> String {
    format!("Error: {error}\nWhy: {why}\nWhat now: {what_now}\n")
}

/// The refusal of a `Cargo.toml` without a workspace version, under the subcommand's Error line.
fn no_version(error: &str) -> String {
    refusal(
        error,
        "Cargo.toml holds no version in its [workspace.package] table.",
        "Put the version in the [workspace.package] table of Cargo.toml on main, then run the release again.",
    )
}

/// The refusal of a version that is not three whole numbers.
fn not_whole(version: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The version in Cargo.toml, {version}, is not three whole numbers such as 0.2.0; Nage publishes whole versions only."
        ),
        "Land a version of three whole numbers in Cargo.toml on main, then tag that commit.",
    )
}

/// The refusal of a tag that is not `v` and the version.
fn not_v_and_version(tag: &str, version: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The tag {tag} is not v followed by this commit's version, {version} (Cargo.toml)."
        ),
        &format!(
            "Release this commit as v{version}, or land the version you meant in Cargo.toml on main and tag that commit."
        ),
    )
}

/// The refusal of a repository without a remote named origin.
fn no_origin(error: &str, what: &str) -> String {
    refusal(
        error,
        &format!("This repository has no remote named origin, so {what} could not be checked."),
        "Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to.",
    )
}

/// The refusal of an origin that does not answer.
fn no_answer(error: &str, what: &str) -> String {
    refusal(
        error,
        &format!("The remote origin did not answer, so {what}."),
        "Run the release again; if origin still does not answer, check its address with git remote get-url origin.",
    )
}

/// The refusal of `notes` in a repository without a remote named origin.
fn notes_no_origin() -> String {
    refusal(
        NOTES_ERROR,
        "This repository has no remote named origin, so the tag's message could not be read.",
        "Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to.",
    )
}

/// The refusal of a tag origin does not hold, by `tag`.
fn not_held(tag: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!("The remote origin holds no tag {tag}."),
        &format!("Push the tag with git push origin {tag}, then run the release again."),
    )
}

/// The refusal of a lightweight tag on origin, by `tag`.
fn lightweight(tag: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The tag {tag} on origin is a lightweight tag; a release needs an annotated tag, whose message becomes the Release page's text."
        ),
        "Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC).",
    )
}

/// The refusal of a tag that names another commit than the one given.
fn another_commit(tag: &str, tagged: &str, given: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The tag {tag} on origin names the commit {tagged}, but this run was given {given}."
        ),
        "Run the release from the tag itself, so that it builds the commit the tag names.",
    )
}

/// The refusal of a local copy of main that is not the main origin holds.
fn stale_main() -> String {
    refusal(
        TAG_ERROR,
        "The local copy of main (origin/main) is not the main that origin holds, so the commit could not be looked for on it.",
        "Fetch first with git fetch origin main, then run the release again.",
    )
}

/// The refusal of a commit main does not hold.
fn not_on_main(tag: &str, commit: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The commit {commit} of the tag {tag} is not on main: it is neither the commit origin holds for main nor one of its ancestors."
        ),
        "Release under the next version number, tagging a commit that main holds.",
    )
}

/// The refusal of a tag whose message on origin is empty.
fn empty_message(tag: &str) -> String {
    refusal(
        TAG_ERROR,
        &format!(
            "The tag {tag} on origin has an empty message; its message becomes the Release page's text."
        ),
        "Release under the next version number with a tag whose message says what the release holds (git tag -a, made with TZ=UTC).",
    )
}

/// git's four signature marks, each with a block as a signed tag carries it after its message.
const SIGNATURES: [(&str, &str); 4] = [
    (
        "-----BEGIN PGP SIGNATURE-----",
        "-----BEGIN PGP SIGNATURE-----\n\nwsBcBAABCAAQBQJpVbuAAAoJEAAAAAAAAAAA\n-----END PGP SIGNATURE-----\n",
    ),
    (
        "-----BEGIN PGP MESSAGE-----",
        "-----BEGIN PGP MESSAGE-----\n\nowGbwMvMwCGWZbPx9upfVQ8BAAAAAAAAAAAA\n-----END PGP MESSAGE-----\n",
    ),
    (
        "-----BEGIN SSH SIGNATURE-----",
        "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAA\n-----END SSH SIGNATURE-----\n",
    ),
    (
        "-----BEGIN SIGNED MESSAGE-----",
        "-----BEGIN SIGNED MESSAGE-----\nMIAGCSqGSIb3DQEHAqCAMIACAQExDTALBglghkgBZQMEAgEw\n-----END SIGNED MESSAGE-----\n",
    ),
];

/// The accepting line of `tag`.
fn accepted(tag: &str, version: &str, commit: &str) -> String {
    format!(
        "Success: The tag {tag} may be released: annotated on origin, version {version}, on main at {commit}.\n"
    )
}

/// The Release page's text for `message` and the version 0.2.0.
fn notes_text(message: &str) -> String {
    format!(
        "{message}\n\nImage: {IMAGE}\nPull it as: ghcr.io/bngoller/nage:0.2.0\nDeployment guide: {GUIDE}\nTo update: put 0.2.0 in your Compose file's image line, then pull and start (the guide's \"Update to a new version\").\n"
    )
}

/// The refusal of `notes` for a lightweight tag on origin.
fn notes_lightweight(tag: &str) -> String {
    refusal(
        NOTES_ERROR,
        &format!("The tag {tag} on origin is a lightweight tag, so it has no message to show."),
        "Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC).",
    )
}

/// The refusal of `notes` for a tag origin does not hold.
fn notes_not_held(tag: &str) -> String {
    refusal(
        NOTES_ERROR,
        &format!("The remote origin holds no tag {tag}, so there is no message to show."),
        &format!("Push the annotated tag with git push origin {tag}, then run the release again."),
    )
}

/// The refusal of any other use.
fn usage() -> String {
    refusal(
        "The command was not run.",
        "release-check takes version, tag <tag> <commit> or notes <tag> <image-with-digest> <guide-address>.",
        "Run scripts/release-check.sh with one of them.",
    )
}

/// Gives `command` git's fixed surroundings: no global or system configuration, none of the
/// caller's git variables, a made-up identity, one fixed date, `TZ=UTC` and English messages.
fn fixed_git_env(command: &mut Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
    ] {
        command.env_remove(name);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Sam")
        .env("GIT_AUTHOR_EMAIL", "sam@nage.example")
        .env("GIT_COMMITTER_NAME", "Sam")
        .env("GIT_COMMITTER_EMAIL", "sam@nage.example")
        .env("GIT_AUTHOR_DATE", DATE)
        .env("GIT_COMMITTER_DATE", DATE)
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .env("LANG", "C");
}

/// What a run of the script gave.
struct Out {
    what: String,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl fmt::Display for Out {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` exited {:?}\n--- stdout\n{}--- stderr\n{}---",
            self.what, self.code, self.stdout, self.stderr
        )
    }
}

impl Out {
    /// Asserts exit 0, `stdout` exactly and nothing on stderr.
    fn assert_success(&self, stdout: &str) {
        assert_eq!(self.code, Some(0), "{self}");
        assert_eq!(self.stdout, stdout, "{self}");
        assert_eq!(self.stderr, "", "{self}");
    }

    /// Asserts exit `code`, nothing on stdout and `stderr` exactly.
    fn assert_refused(&self, code: i32, stderr: &str) {
        assert_eq!(self.code, Some(code), "{self}");
        assert_eq!(self.stdout, "", "{self}");
        assert_eq!(self.stderr, stderr, "{self}");
    }
}

/// A repository's refs, `FETCH_HEAD` and the names of its loose and packed objects.
#[derive(Debug, PartialEq, Eq)]
struct RepoState {
    refs: String,
    fetch_head: Option<String>,
    objects: Vec<String>,
}

/// A temporary folder holding a bare `origin.git`, a repository `work` whose remote origin it is
/// (one commit on main, pushed and fetched), and the `tmp` folder the script is given as `TMPDIR`.
struct Repos {
    dir: TestDir,
    first: String,
}

impl Repos {
    /// The two repositories, with the workspace version 0.2.0.
    fn new() -> Self {
        let dir = TestDir::new();
        let repos = Self {
            dir,
            first: String::new(),
        };
        fs::create_dir(repos.tmp()).expect("create tmp");
        repos.git(
            repos.dir.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        repos.git(repos.dir.path(), &["init", "-q", "-b", "main", "work"]);
        let work = repos.work();
        fs::write(work.join("Cargo.toml"), cargo_toml(VERSION)).expect("write Cargo.toml");
        fs::create_dir(work.join("scripts")).expect("create scripts");
        fs::copy(SCRIPT, work.join("scripts/release-check.sh")).expect("copy the script");
        repos.git(&work, &["add", "-A"]);
        repos.git(&work, &["commit", "-q", "-m", "The first commit"]);
        let origin = repos.origin();
        repos.git(
            &work,
            &[
                "remote",
                "add",
                "origin",
                origin.to_str().expect("utf-8 path"),
            ],
        );
        repos.git(&work, &["push", "-q", "origin", "main"]);
        repos.git(&work, &["fetch", "-q", "origin"]);
        let first = repos.git(&work, &["rev-parse", "HEAD"]);
        Self { first, ..repos }
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn origin(&self) -> PathBuf {
        self.dir.path().join("origin.git")
    }

    fn tmp(&self) -> PathBuf {
        self.dir.path().join("tmp")
    }

    /// Runs git in `cwd`, asserting success; returns its stdout without the final newline.
    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command.args(args).current_dir(cwd);
        fixed_git_env(&mut command);
        let output = command
            .output()
            .unwrap_or_else(|error| panic!("cannot run git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {args:?} in {} failed: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf-8 output")
            .trim_end()
            .to_owned()
    }

    /// Git in `work`.
    fn work_git(&self, args: &[&str]) -> String {
        self.git(&self.work(), args)
    }

    /// Git in the bare origin.
    fn origin_git(&self, args: &[&str]) -> String {
        self.git(&self.origin(), args)
    }

    /// A new empty commit in `work`, returning its id.
    fn commit(&self, message: &str) -> String {
        self.work_git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.work_git(&["rev-parse", "HEAD"])
    }

    /// Pushes main to origin and fetches it back, so that origin/main follows.
    fn push_main(&self) {
        self.work_git(&["push", "-q", "origin", "main"]);
        self.work_git(&["fetch", "-q", "origin"]);
    }

    /// An annotated tag in `work` on `commit` with `message`.
    fn annotated(&self, tag: &str, commit: &str, message: &str) {
        self.work_git(&["tag", "-a", tag, "-m", message, commit]);
    }

    /// A lightweight tag in `work` on `commit`.
    fn light(&self, tag: &str, commit: &str) {
        self.work_git(&["tag", tag, commit]);
    }

    /// Pushes the tag of `work` to origin under exactly `refs/tags/<tag>`.
    fn push_tag(&self, tag: &str) {
        let refspec = format!("refs/tags/{tag}:refs/tags/{tag}");
        self.work_git(&["push", "-q", "origin", &refspec]);
    }

    /// Writes an annotated tag `tag` on `commit` straight into origin, its message part being
    /// `body` as given (a signature block included), as a signed tag arrives there.
    fn origin_tag_object(&self, tag: &str, commit: &str, body: &str) {
        let object = format!(
            "object {commit}\ntype commit\ntag {tag}\ntagger Sam <sam@nage.example> 1767225600 +0000\n\n{body}"
        );
        let file = self.dir.path().join("tag-object");
        fs::write(&file, object).expect("write the tag object");
        let id = self.origin_git(&[
            "hash-object",
            "-t",
            "tag",
            "-w",
            file.to_str().expect("utf-8 path"),
        ]);
        self.origin_git(&["update-ref", &format!("refs/tags/{tag}"), &id]);
        fs::remove_file(&file).expect("remove the tag object's file");
    }

    /// Deletes the tag of `work`, leaving origin's.
    fn delete_local_tag(&self, tag: &str) {
        self.work_git(&["tag", "-d", tag]);
    }

    /// Writes `work`'s Cargo.toml as it stands (not committed).
    fn write_cargo_toml(&self, contents: &str) {
        fs::write(self.work().join("Cargo.toml"), contents).expect("write Cargo.toml");
    }

    /// Runs `bash scripts/release-check.sh <args>` inside `repo`.
    fn run_in(&self, repo: &Path, args: &[&str]) -> Out {
        self.run_with(repo, &repo.join("scripts/release-check.sh"), args, &[])
    }

    /// Runs `bash <script> <args>` inside `repo`, `script` as given (relative to `repo` or
    /// not), with `envs` added to git's fixed surroundings.
    fn run_with(&self, repo: &Path, script: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Out {
        let mut command = Command::new("bash");
        command.arg(script).args(args).current_dir(repo);
        fixed_git_env(&mut command);
        command.env("TMPDIR", self.tmp());
        for (name, value) in envs {
            command.env(name, value);
        }
        let output = command
            .output()
            .unwrap_or_else(|error| panic!("cannot run bash: {error}"));
        Out {
            what: format!("release-check.sh {}", args.join(" ")),
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
            stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
        }
    }

    /// Runs the script inside `work`.
    fn run(&self, args: &[&str]) -> Out {
        self.run_in(&self.work(), args)
    }

    /// The state of `repo` the script must leave as it was.
    fn state(&self, repo: &Path) -> RepoState {
        let refs = self.git(
            repo,
            &[
                "for-each-ref",
                "--format=%(objectname) %(objecttype) %(refname)",
            ],
        );
        let git_dir = repo.join(".git");
        let fetch_head = fs::read_to_string(git_dir.join("FETCH_HEAD")).ok();
        let mut objects = Vec::new();
        for entry in fs::read_dir(git_dir.join("objects")).expect("read objects") {
            let entry = entry.expect("objects entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.path().is_dir() {
                objects.push(name);
                continue;
            }
            for inner in fs::read_dir(entry.path()).expect("read an objects folder") {
                let inner = inner.expect("objects folder entry");
                objects.push(format!("{name}/{}", inner.file_name().to_string_lossy()));
            }
        }
        objects.sort();
        RepoState {
            refs,
            fetch_head,
            objects,
        }
    }

    /// Asserts that the script's temporary folder is empty again.
    fn assert_tmp_empty(&self) {
        let left: Vec<String> = fs::read_dir(self.tmp())
            .expect("read tmp")
            .map(|entry| {
                entry
                    .expect("tmp entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(left.is_empty(), "left in the temporary folder: {left:?}");
    }
}

/// Asserts that `out` refused in exactly three lines, Error, Why and What now, the Why being `why`.
fn assert_three_lines(out: &Out, why: &str) {
    assert_eq!(out.code, Some(1), "{out}");
    assert_eq!(out.stdout, "", "{out}");
    let lines: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].starts_with("Error: "), "{out}");
    assert_eq!(lines[1], format!("Why: {why}"), "{out}");
    assert!(lines[2].starts_with("What now: "), "{out}");
}

/// Asserts that a refusal holds none of git's own words and no advice to push the tag.
fn assert_no_git_text(out: &Out) {
    for word in ["fatal", "error:", "git push", "hint:"] {
        assert!(!out.stderr.contains(word), "`{word}` in {out}");
    }
}

/// The script's header, the comment lines after its first line, each without its `#` and the
/// spaces around it.
fn header() -> Vec<String> {
    let script = fs::read_to_string(SCRIPT).expect("read the script");
    script
        .lines()
        .skip(1)
        .take_while(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim().to_owned())
        .collect()
}

/// Asserts that the script's header holds each of `texts` word for word, its lines one after
/// another, and in the order given.
fn assert_in_header(texts: &[String]) {
    let header = header();
    let mut from = 0;
    for text in texts {
        let lines: Vec<&str> = text.lines().collect();
        let at = header
            .windows(lines.len())
            .enumerate()
            .skip(from)
            .find(|(_, window)| window.iter().map(String::as_str).eq(lines.iter().copied()))
            .map(|(at, _)| at);
        match at {
            Some(at) => from = at + lines.len(),
            None => panic!(
                "the header of scripts/release-check.sh does not hold, after its line {from}, word for word:\n{text}"
            ),
        }
    }
}

/// `version` prints the workspace version as Cargo.toml holds it, the table of another package
/// and an uncommitted suffix included.
///
/// Proves: SPEC decision 193: version prints the version of the workspace.package table of the Cargo.toml beside the script as it stands, a suffix such as 0.2.0-rc.1 included, and nothing else, exit 0, nothing on standard error
#[test]
fn version_prints_the_workspace_version_alone() {
    let repos = Repos::new();
    repos.run(&["version"]).assert_success("0.2.0\n");

    repos.write_cargo_toml(
        "[package]\nname = \"other\"\nversion = \"9.9.9\"\n\n[workspace]\nmembers = []\n\n[workspace.package]\nedition = \"2024\"\nversion = \"0.2.0-rc.1\"\n\n[dependencies]\nversion = \"1.0.0\"\n",
    );
    repos.run(&["version"]).assert_success("0.2.0-rc.1\n");

    // TOML allows spaces inside a table header's brackets.
    repos.write_cargo_toml(
        "[workspace]\nmembers = []\n\n[ workspace.package ]\nversion = \"0.2.1\"\n",
    );
    repos.run(&["version"]).assert_success("0.2.1\n");

    // A rust-version key of the same table is not the version.
    repos.write_cargo_toml(
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.2.0\"\nrust-version = \"1.99.1\"\nedition = \"2024\"\n",
    );
    repos.run(&["version"]).assert_success("0.2.0\n");

    // A last line without a line end is read.
    repos.write_cargo_toml("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.2.1\"");
    repos.run(&["version"]).assert_success("0.2.1\n");

    // A TOML literal string, in single quotes, is a version too.
    repos.write_cargo_toml("[workspace]\nmembers = []\n\n[workspace.package]\nversion = '0.2.2'\n");
    repos.run(&["version"]).assert_success("0.2.2\n");

    // The script run by a relative path under an exported CDPATH that holds the repository: cd
    // would print the folder it found, a second line in the repository's path, unless the script
    // clears CDPATH.
    let work = repos.work();
    repos
        .run_with(
            &work,
            Path::new("scripts/release-check.sh"),
            &["version"],
            &[("CDPATH", &work)],
        )
        .assert_success("0.2.2\n");
}

/// An annotated tag on origin on main's own commit, and one on an ancestor of main, are each
/// accepted in one Success line.
///
/// Proves: SPEC decisions 171 and 193: an annotated v and the version on origin, naming the given commit, which is the commit origin reports for refs/heads/main or one of its ancestors while the local origin/main is that same commit, is accepted with one Success line, exit 0
#[test]
fn a_tag_that_keeps_every_rule_is_accepted_in_one_success_line() {
    let at_main = Repos::new();
    at_main.annotated(TAG, &at_main.first, MESSAGE);
    at_main.push_tag(TAG);
    at_main
        .run(&["tag", TAG, &at_main.first])
        .assert_success(&accepted(TAG, VERSION, &at_main.first));

    let on_ancestor = Repos::new();
    on_ancestor.annotated(TAG, &on_ancestor.first, MESSAGE);
    on_ancestor.push_tag(TAG);
    on_ancestor.commit("A later commit");
    on_ancestor.commit("A still later commit");
    on_ancestor.push_main();
    on_ancestor
        .run(&["tag", TAG, &on_ancestor.first])
        .assert_success(&accepted(TAG, VERSION, &on_ancestor.first));
}

/// Another version, a tag without its `v` and a tag with a suffix the version lacks are each
/// refused before origin is asked.
///
/// Proves: SPEC decisions 171 and 193: another version, a tag without its v and a tag with a suffix the version lacks are refused in the three labelled lines naming the tag and the version, exit 1
#[test]
fn a_tag_that_is_not_v_and_the_version_is_refused() {
    let repos = Repos::new();
    for tag in ["v0.3.0", "0.2.0", "v0.2.0-rc.1"] {
        repos.annotated(tag, &repos.first, MESSAGE);
        repos.push_tag(tag);
        repos
            .run(&["tag", tag, &repos.first])
            .assert_refused(1, &not_v_and_version(tag, VERSION));
    }
}

/// A tag made in the local repository and never pushed is refused.
///
/// Proves: SPEC decision 193: a tag made locally and never pushed is refused in the three labelled lines naming origin, exit 1
#[test]
fn a_tag_that_origin_does_not_hold_is_refused() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos
        .run(&["tag", TAG, &repos.first])
        .assert_refused(1, &not_held(TAG));
}

/// Origin's tag decides, never the local one: lightweight on origin and annotated here is
/// refused; annotated on origin and lightweight here is accepted.
///
/// Proves: SPEC decision 193: a lightweight tag on origin is refused even when the local tag of that name is annotated, and an annotated tag on origin is accepted even when the local one is lightweight, as a checkout may make it
#[test]
fn a_lightweight_tag_on_origin_is_refused_whatever_the_local_tag() {
    let light_there = Repos::new();
    light_there.light(TAG, &light_there.first);
    light_there.push_tag(TAG);
    light_there.delete_local_tag(TAG);
    light_there.annotated(TAG, &light_there.first, MESSAGE);
    light_there
        .run(&["tag", TAG, &light_there.first])
        .assert_refused(1, &lightweight(TAG));

    let annotated_there = Repos::new();
    annotated_there.annotated(TAG, &annotated_there.first, MESSAGE);
    annotated_there.push_tag(TAG);
    annotated_there.delete_local_tag(TAG);
    annotated_there.light(TAG, &annotated_there.first);
    annotated_there
        .run(&["tag", TAG, &annotated_there.first])
        .assert_success(&accepted(TAG, VERSION, &annotated_there.first));
}

/// Only the refs named exactly count: a branch of the tag's name and nested annotated tags whose
/// names end like it never make a tag annotated, nor stand for a tag origin lacks.
///
/// Proves: SPEC decision 193: a branch named like the tag beside a lightweight tag on origin is refused as not annotated, and an annotated tag x/ and the tag name on origin, with no tag of the name itself, is refused as a tag origin does not hold
#[test]
fn only_the_exact_refs_on_origin_count() {
    let branch = Repos::new();
    branch.origin_git(&["tag", TAG, &branch.first]);
    branch.origin_git(&["update-ref", &format!("refs/heads/{TAG}"), &branch.first]);
    branch.origin_git(&[
        "tag",
        "-a",
        &format!("x/{TAG}"),
        "-m",
        MESSAGE,
        &branch.first,
    ]);
    branch.origin_git(&[
        "tag",
        "-a",
        &format!("x/refs/tags/{TAG}"),
        "-m",
        MESSAGE,
        &branch.first,
    ]);
    branch
        .run(&["tag", TAG, &branch.first])
        .assert_refused(1, &lightweight(TAG));

    let nested = Repos::new();
    nested.origin_git(&[
        "tag",
        "-a",
        &format!("x/{TAG}"),
        "-m",
        MESSAGE,
        &nested.first,
    ]);
    nested.origin_git(&[
        "tag",
        "-a",
        &format!("x/refs/tags/{TAG}"),
        "-m",
        MESSAGE,
        &nested.first,
    ]);
    nested
        .run(&["tag", TAG, &nested.first])
        .assert_refused(1, &not_held(TAG));
}

/// A tag on origin naming another commit than the one the run was given is refused, naming both.
///
/// Proves: SPEC decision 193: a tag on origin naming another commit than the one given is refused in the three labelled lines naming both commits, exit 1
#[test]
fn a_tag_that_names_another_commit_is_refused() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    let second = repos.commit("The second commit");
    repos.push_main();
    repos
        .run(&["tag", TAG, &second])
        .assert_refused(1, &another_commit(TAG, &repos.first, &second));
}

/// A local origin/main behind origin's main, and an origin whose main the local repository does
/// not hold, are each refused with the advice to fetch first.
///
/// Proves: SPEC decision 193: when the local origin/main is behind the main origin reports, or the commit origin reports for main is not in the local repository, the tag is refused in three lines of their own saying the local copy of main is not what origin holds and to fetch first, exit 1
#[test]
fn a_local_main_that_is_not_origins_main_is_refused() {
    let behind = Repos::new();
    behind.annotated(TAG, &behind.first, MESSAGE);
    behind.push_tag(TAG);
    behind.commit("The second commit");
    behind.push_main();
    behind.work_git(&["update-ref", "refs/remotes/origin/main", &behind.first]);
    behind
        .run(&["tag", TAG, &behind.first])
        .assert_refused(1, &stale_main());

    // No local origin/main at all.
    let missing = Repos::new();
    missing.annotated(TAG, &missing.first, MESSAGE);
    missing.push_tag(TAG);
    missing.work_git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    missing
        .run(&["tag", TAG, &missing.first])
        .assert_refused(1, &stale_main());

    let unknown = Repos::new();
    unknown.annotated(TAG, &unknown.first, MESSAGE);
    unknown.push_tag(TAG);
    let origin = unknown.origin();
    unknown.git(
        unknown.dir.path(),
        &["clone", "-q", origin.to_str().expect("utf-8 path"), "other"],
    );
    let other = unknown.dir.path().join("other");
    unknown.git(
        &other,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "A commit made elsewhere",
        ],
    );
    unknown.git(&other, &["push", "-q", "origin", "main"]);
    unknown
        .run(&["tag", TAG, &unknown.first])
        .assert_refused(1, &stale_main());
}

/// A tag on a commit of another branch, which main does not hold, is refused.
///
/// Proves: SPEC decisions 171 and 193: a tag on a commit of another branch, neither the commit origin reports for main nor one of its ancestors, is refused in the three labelled lines naming main, exit 1
#[test]
fn a_commit_main_does_not_hold_is_refused() {
    let repos = Repos::new();
    repos.work_git(&["checkout", "-q", "-b", "side"]);
    let side = repos.commit("A commit of another branch");
    repos.work_git(&["push", "-q", "origin", "side"]);
    // A branch whose name ends like main's ref never stands for main.
    repos.work_git(&["push", "-q", "origin", "side:refs/heads/x/refs/heads/main"]);
    repos.annotated(TAG, &side, MESSAGE);
    repos.push_tag(TAG);
    repos
        .run(&["tag", TAG, &side])
        .assert_refused(1, &not_on_main(TAG, &side));

    // A tagged commit pushed from elsewhere, which the local repository does not hold while its
    // copy of main is current: it cannot be judged here (git merge-base ends with 128), so the
    // advice to fetch first, never git's own text.
    let elsewhere = Repos::new();
    let origin = elsewhere.origin();
    elsewhere.git(
        elsewhere.dir.path(),
        &["clone", "-q", origin.to_str().expect("utf-8 path"), "other"],
    );
    let other = elsewhere.dir.path().join("other");
    elsewhere.git(&other, &["checkout", "-q", "-b", "side"]);
    elsewhere.git(
        &other,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "A commit made elsewhere",
        ],
    );
    let unknown = elsewhere.git(&other, &["rev-parse", "HEAD"]);
    elsewhere.git(&other, &["tag", "-a", TAG, "-m", MESSAGE, &unknown]);
    let refspec = format!("refs/tags/{TAG}:refs/tags/{TAG}");
    elsewhere.git(&other, &["push", "-q", "origin", "side", &refspec]);
    elsewhere
        .run(&["tag", TAG, &unknown])
        .assert_refused(1, &stale_main());

    // A shallow clone that does not hold the tagged commit, an ancestor of main: never "not on
    // main" with the advice of the next version number, but the advice to fetch first.
    let shallow = Repos::new();
    shallow.annotated(TAG, &shallow.first, MESSAGE);
    shallow.push_tag(TAG);
    shallow.commit("The second commit");
    shallow.push_main();
    let address = format!("file://{}", shallow.origin().display());
    shallow.git(
        shallow.dir.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            &address,
            "shallow",
        ],
    );
    let clone = shallow.dir.path().join("shallow");
    assert_eq!(
        shallow.git(&clone, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    shallow
        .run_in(&clone, &["tag", TAG, &shallow.first])
        .assert_refused(1, &stale_main());
    shallow.assert_tmp_empty();
}

/// A workspace version with a suffix is refused for its own tag and for the whole version's
/// alike, while `version` still prints it.
///
/// Proves: SPEC decisions 165 and 193: with the workspace version 0.2.0-rc.1, tag refuses v0.2.0-rc.1 and v0.2.0 alike in three lines of their own saying Nage publishes whole versions only, exit 1, while version prints 0.2.0-rc.1
#[test]
fn a_version_that_is_not_three_whole_numbers_is_refused() {
    let repos = Repos::new();
    repos.write_cargo_toml(&cargo_toml("0.2.0-rc.1"));
    repos.work_git(&["commit", "-q", "-a", "-m", "A version with a suffix"]);
    repos.push_main();
    let commit = repos.work_git(&["rev-parse", "HEAD"]);
    for tag in ["v0.2.0-rc.1", "v0.2.0"] {
        repos.annotated(tag, &commit, MESSAGE);
        repos.push_tag(tag);
        repos
            .run(&["tag", tag, &commit])
            .assert_refused(1, &not_whole("0.2.0-rc.1"));
    }
    repos.run(&["version"]).assert_success("0.2.0-rc.1\n");
}

/// A tag that breaks several rules is refused for the first in the script's order only; the
/// script's header lists every refusal word for word, in that order.
///
/// Proves: SPEC decisions 157 and 193: a tag breaking several rules is refused in exactly three lines, Error, Why and What now, the Why naming the first rule in the script's order
#[test]
fn only_the_first_broken_rule_is_named() {
    // A version with a suffix, a wrong tag and no origin: the version comes first.
    let suffix = Repos::new();
    suffix.write_cargo_toml(&cargo_toml("0.2.0-rc.1"));
    suffix.work_git(&["remote", "remove", "origin"]);
    let out = suffix.run(&["tag", "v9.9.9", &suffix.first]);
    assert_three_lines(
        &out,
        "The version in Cargo.toml, 0.2.0-rc.1, is not three whole numbers such as 0.2.0; Nage publishes whole versions only.",
    );

    // A wrong tag and no origin: the tag comes first.
    let no_remote = Repos::new();
    no_remote.work_git(&["remote", "remove", "origin"]);
    let out = no_remote.run(&["tag", "v0.3.0", &no_remote.first]);
    assert_three_lines(
        &out,
        "The tag v0.3.0 is not v followed by this commit's version, 0.2.0 (Cargo.toml).",
    );

    // Lightweight on origin, naming another commit, off main: lightweight comes first.
    let several = Repos::new();
    several.work_git(&["checkout", "-q", "-b", "side"]);
    let side = several.commit("A commit of another branch");
    several.work_git(&["push", "-q", "origin", "side"]);
    several.light(TAG, &side);
    several.push_tag(TAG);
    let out = several.run(&["tag", TAG, &several.first]);
    assert_three_lines(
        &out,
        "The tag v0.2.0 on origin is a lightweight tag; a release needs an annotated tag, whose message becomes the Release page's text.",
    );

    // Annotated, naming another commit, off main: the other commit comes first.
    let annotated = Repos::new();
    annotated.work_git(&["checkout", "-q", "-b", "side"]);
    let side = annotated.commit("A commit of another branch");
    annotated.work_git(&["push", "-q", "origin", "side"]);
    annotated.annotated(TAG, &side, MESSAGE);
    annotated.push_tag(TAG);
    let other = annotated.commit("Another commit of that branch");
    let out = annotated.run(&["tag", TAG, &other]);
    assert_three_lines(
        &out,
        &format!(
            "The tag v0.2.0 on origin names the commit {side}, but this run was given {other}."
        ),
    );

    // The script's order is its header's: the header lists every refusal word for word, the
    // tag's rules in the order the script asks them.
    assert_in_header(&[
        no_version(VERSION_ERROR),
        accepted("<tag>", "<version>", "<commit>"),
        no_version(TAG_ERROR),
        not_whole("<version>"),
        not_v_and_version("<tag>", "<version>"),
        no_origin(TAG_ERROR, "the tag"),
        no_answer(TAG_ERROR, "the tag could not be checked"),
        not_held("<tag>"),
        lightweight("<tag>"),
        another_commit("<tag>", "<tagged>", "<commit>"),
        stale_main(),
        not_on_main("<tag>", "<commit>"),
        empty_message("<tag>"),
        no_version(NOTES_ERROR),
        notes_no_origin(),
        no_answer(NOTES_ERROR, "the tag's message could not be read"),
        notes_not_held("<tag>"),
        notes_lightweight("<tag>"),
        usage(),
    ]);
}

/// Without a remote named origin, `tag` and `notes` each refuse in lines of their own.
///
/// Proves: SPEC decision 193: in a repository with no remote named origin, tag and notes each refuse in three lines naming the missing remote, exit 1, never git's own text and never the advice to push the tag
#[test]
fn a_repository_without_origin_is_refused_in_lines_of_its_own() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    repos.work_git(&["remote", "remove", "origin"]);

    let out = repos.run(&["tag", TAG, &repos.first]);
    out.assert_refused(1, &no_origin(TAG_ERROR, "the tag"));
    assert_no_git_text(&out);

    let out = repos.run(&["notes", TAG, IMAGE, GUIDE]);
    out.assert_refused(1, &notes_no_origin());
    assert_no_git_text(&out);
    repos.assert_tmp_empty();
}

/// With origin pointing at a folder that is no repository, `tag` and `notes` each refuse in
/// lines of their own.
///
/// Proves: SPEC decision 193: with origin pointing at a folder that is no repository, tag and notes each refuse in three lines saying origin did not answer, exit 1, never git's own text and never the advice to push the tag
#[test]
fn an_origin_that_does_not_answer_is_refused_in_lines_of_its_own() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    let nowhere = repos.dir.path().join("not-a-repository");
    fs::create_dir(&nowhere).expect("create the folder");
    repos.work_git(&[
        "remote",
        "set-url",
        "origin",
        nowhere.to_str().expect("utf-8 path"),
    ]);

    let out = repos.run(&["tag", TAG, &repos.first]);
    out.assert_refused(1, &no_answer(TAG_ERROR, "the tag could not be checked"));
    assert_no_git_text(&out);

    let out = repos.run(&["notes", TAG, IMAGE, GUIDE]);
    out.assert_refused(
        1,
        &no_answer(NOTES_ERROR, "the tag's message could not be read"),
    );
    assert_no_git_text(&out);
    repos.assert_tmp_empty();
}

/// A Cargo.toml whose workspace table holds no version, though a package of it does, stops all
/// three subcommands.
///
/// Proves: SPEC decision 193: when the workspace.package table of Cargo.toml holds no version, version, tag and notes each refuse in three lines naming Cargo.toml, exit 1, nothing on standard output
#[test]
fn a_cargo_toml_without_a_workspace_version_is_refused() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    repos.write_cargo_toml(
        "[package]\nname = \"other\"\nversion = \"0.2.0\"\n\n[workspace]\nmembers = []\n\n[workspace.package]\nedition = \"2024\"\n",
    );
    repos
        .run(&["version"])
        .assert_refused(1, &no_version(VERSION_ERROR));
    repos
        .run(&["tag", TAG, &repos.first])
        .assert_refused(1, &no_version(TAG_ERROR));
    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_refused(1, &no_version(NOTES_ERROR));

    // A rust-version key is no version.
    repos.write_cargo_toml(
        "[workspace]\nmembers = []\n\n[workspace.package]\nrust-version = \"1.99.1\"\nedition = \"2024\"\n",
    );
    repos
        .run(&["version"])
        .assert_refused(1, &no_version(VERSION_ERROR));

    // No Cargo.toml at all: the same three lines, never the shell's own text.
    fs::remove_file(repos.work().join("Cargo.toml")).expect("remove Cargo.toml");
    repos
        .run(&["version"])
        .assert_refused(1, &no_version(VERSION_ERROR));
    repos
        .run(&["tag", TAG, &repos.first])
        .assert_refused(1, &no_version(TAG_ERROR));
    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_refused(1, &no_version(NOTES_ERROR));
}

/// `notes` prints origin's message, not the local tag's, then a blank line and the four fixed
/// lines.
///
/// Proves: SPEC decision 167: notes prints the annotated tag's message as origin holds it, even when the local tag of that name carries another, a blank line, then exactly Image with the image and its digest, Pull it as with the image's name and the version, Deployment guide, and To update naming the version and the guide's Update to a new version, exit 0
#[test]
fn notes_print_the_tags_message_and_four_fixed_lines() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    repos.delete_local_tag(TAG);
    repos.annotated(TAG, &repos.first, "A message origin never saw");
    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_success(&notes_text(MESSAGE));
    repos.assert_tmp_empty();

    // An origin given by a relative address is read relative to the repository.
    repos.work_git(&["remote", "set-url", "origin", "../origin.git"]);
    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_success(&notes_text(MESSAGE));
    repos.assert_tmp_empty();

    // A tag whose message is empty: an empty message, never the tag's header with its tagger.
    let empty = Repos::new();
    empty.annotated(TAG, &empty.first, "");
    empty.push_tag(TAG);
    empty
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_success(&notes_text(""));
    empty.assert_tmp_empty();

    // A signed tag: the message without its signature block, whichever of git's four marks
    // begins it.
    for (mark, block) in SIGNATURES {
        let signed = Repos::new();
        signed.origin_tag_object(TAG, &signed.first, &format!("{MESSAGE}\n{block}"));
        let out = signed.run(&["notes", TAG, IMAGE, GUIDE]);
        assert_eq!(out.code, Some(0), "signed with {mark}: {out}");
        assert_eq!(out.stdout, notes_text(MESSAGE), "signed with {mark}: {out}");
        assert_eq!(out.stderr, "", "signed with {mark}: {out}");
        signed.assert_tmp_empty();
    }
}

/// `notes` works in a clone of depth 1 that holds no tag.
///
/// Proves: SPEC decisions 167 and 193: notes run in a clone of depth 1 that holds no tag prints the message origin holds, exit 0
#[test]
fn notes_read_the_message_from_a_shallow_checkout() {
    let repos = Repos::new();
    repos.commit("The second commit");
    repos.push_main();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    let address = format!("file://{}", repos.origin().display());
    repos.git(
        repos.dir.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            &address,
            "shallow",
        ],
    );
    let shallow = repos.dir.path().join("shallow");
    assert_eq!(
        repos.git(&shallow, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    assert_eq!(repos.git(&shallow, &["tag", "--list"]), "");
    let before = repos.state(&shallow);
    repos
        .run_in(&shallow, &["notes", TAG, IMAGE, GUIDE])
        .assert_success(&notes_text(MESSAGE));
    assert_eq!(repos.state(&shallow), before);
    repos.assert_tmp_empty();
}

/// `notes` refuses a lightweight tag on origin: it has no message.
///
/// Proves: SPEC decisions 167 and 193: notes for a lightweight tag on origin prints three lines saying it has no message to show and nothing on standard output, exit 1
#[test]
fn notes_refuse_a_lightweight_tag_on_origin() {
    let repos = Repos::new();
    repos.light(TAG, &repos.first);
    repos.push_tag(TAG);
    repos.delete_local_tag(TAG);
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_refused(1, &notes_lightweight(TAG));
    repos.assert_tmp_empty();
}

/// `notes` refuses a tag origin does not hold in lines of their own, apart from a lightweight
/// tag's.
///
/// Proves: SPEC decisions 167 and 193: notes for a tag origin does not hold prints three lines of their own saying origin holds no such tag, apart from the refusal of a lightweight tag, and nothing on standard output, exit 1
#[test]
fn notes_refuse_a_tag_origin_does_not_hold() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    let out = repos.run(&["notes", TAG, IMAGE, GUIDE]);
    out.assert_refused(1, &notes_not_held(TAG));
    assert_ne!(out.stderr, notes_lightweight(TAG), "{out}");
    repos.assert_tmp_empty();
}

/// No subcommand, an unknown one and each subcommand with too few or too many arguments exit 2.
///
/// Proves: SPEC decision 193: no subcommand, an unknown one and a wrong number of arguments for each subcommand are refused in the three labelled lines, exit 2
#[test]
fn any_other_use_is_refused_with_exit_2() {
    let repos = Repos::new();
    let first = repos.first.as_str();
    let uses: [&[&str]; 9] = [
        &[],
        &["publish"],
        &["version", "extra"],
        &["tag"],
        &["tag", TAG],
        &["tag", TAG, first, "extra"],
        &["notes", TAG],
        &["notes", TAG, IMAGE],
        &["notes", TAG, IMAGE, GUIDE, "extra"],
    ];
    for args in uses {
        repos.run(args).assert_refused(2, &usage());
    }
}

/// `version` leaves the repository as it was.
///
/// Proves: SPEC decision 193: the repository's refs as git for-each-ref lists them, its FETCH_HEAD and the names of its loose and packed objects are the same before and after version
#[test]
fn version_changes_nothing_in_the_repository() {
    let repos = Repos::new();
    let work = repos.work();
    let before = repos.state(&work);
    assert!(before.fetch_head.is_some(), "the fixture fetched");
    repos.run(&["version"]).assert_success("0.2.0\n");
    assert_eq!(repos.state(&work), before);
}

/// `tag` leaves the repository as it was, whether it accepts or refuses.
///
/// Proves: SPEC decision 193: the repository's refs as git for-each-ref lists them, its FETCH_HEAD and the names of its loose and packed objects are the same before and after an accepted tag and a refused one
#[test]
fn tag_changes_nothing_in_the_repository() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    repos.delete_local_tag(TAG);
    let second = repos.commit("The second commit");
    repos.push_main();
    let work = repos.work();
    let before = repos.state(&work);
    assert!(before.fetch_head.is_some(), "the fixture fetched");

    // The accepted tag's message was fetched into the throwaway, which is gone again.
    repos
        .run(&["tag", TAG, &repos.first])
        .assert_success(&accepted(TAG, VERSION, &repos.first));
    assert_eq!(repos.state(&work), before);
    repos.assert_tmp_empty();

    repos
        .run(&["tag", TAG, &second])
        .assert_refused(1, &another_commit(TAG, &repos.first, &second));
    assert_eq!(repos.state(&work), before);

    // A tag whose message is empty, plain or signed, is refused (the Release page would have no
    // words), its message read in the throwaway as notes reads it.
    let empty = Repos::new();
    empty.annotated(TAG, &empty.first, "");
    empty.push_tag(TAG);
    let before = empty.state(&empty.work());
    empty
        .run(&["tag", TAG, &empty.first])
        .assert_refused(1, &empty_message(TAG));
    assert_eq!(empty.state(&empty.work()), before);
    empty.assert_tmp_empty();
    for (mark, block) in SIGNATURES {
        let signed = Repos::new();
        signed.origin_tag_object(TAG, &signed.first, block);
        let before = signed.state(&signed.work());
        let out = signed.run(&["tag", TAG, &signed.first]);
        assert_eq!(out.code, Some(1), "signed with {mark}: {out}");
        assert_eq!(out.stdout, "", "signed with {mark}: {out}");
        assert_eq!(out.stderr, empty_message(TAG), "signed with {mark}: {out}");
        assert_eq!(signed.state(&signed.work()), before);
        signed.assert_tmp_empty();
    }
}

/// `notes` leaves the repository as it was and removes its throwaway repository.
///
/// Proves: SPEC decision 193: the repository's refs as git for-each-ref lists them, its FETCH_HEAD and the names of its loose and packed objects are the same before and after notes, which reads the message in a throwaway bare repository it removes
#[test]
fn notes_change_nothing_in_the_repository() {
    let repos = Repos::new();
    repos.annotated(TAG, &repos.first, MESSAGE);
    repos.push_tag(TAG);
    repos.delete_local_tag(TAG);
    let work = repos.work();
    let before = repos.state(&work);
    assert!(before.fetch_head.is_some(), "the fixture fetched");

    repos
        .run(&["notes", TAG, IMAGE, GUIDE])
        .assert_success(&notes_text(MESSAGE));
    assert_eq!(repos.state(&work), before);
    repos.assert_tmp_empty();

    // With GIT_DIR exported, as in a hook, the fetch still goes into the throwaway, never into the
    // repository GIT_DIR names.
    let git_dir = work.join(".git");
    repos
        .run_with(
            &work,
            &work.join("scripts/release-check.sh"),
            &["notes", TAG, IMAGE, GUIDE],
            &[("GIT_DIR", &git_dir)],
        )
        .assert_success(&notes_text(MESSAGE));
    assert_eq!(repos.state(&work), before);
    repos.assert_tmp_empty();
}
