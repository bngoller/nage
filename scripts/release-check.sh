#!/usr/bin/env bash
# The release's checks of its tag and the Release page's text (PLAN milestone 2 slice 9; SPEC
# decisions 165, 167, 168, 171 and 193). .github/workflows/release.yml calls it; its tests run it in
# temporary repositories (crates/nage/tests/release_check.rs).
#
#     scripts/release-check.sh version
#     scripts/release-check.sh tag <tag> <commit>
#     scripts/release-check.sh notes <tag> <image-with-digest> <guide-address>
#
# The repository is the one holding this script (its parent folder, found with CDPATH cleared);
# every git command runs there and asks the remote `origin`. Before the first git command the
# script clears git's own variables (git rev-parse --local-env-vars), so that a GIT_DIR set by a
# hook cannot send a command to another repository. It changes nothing in that repository: no
# ref, no FETCH_HEAD, no object, loose or packed; nothing is fetched into it and nothing is pushed.
#
# The tag's message is read the same way by tag and notes, without touching the checkout:
# refs/tags/<tag> is fetched from origin's address (git remote get-url origin) into a throwaway
# bare repository in a temporary folder, read there with git cat-file, and the folder is removed on
# every exit. It works from a shallow checkout. The message is what follows the tag's header and
# its blank line, up to the first of git's signature marks (BEGIN PGP SIGNATURE, BEGIN PGP MESSAGE,
# BEGIN SSH SIGNATURE, BEGIN SIGNED MESSAGE, each between five dashes), its last line ends dropped.
#
# Every question to origin names full refs, as in
#     git ls-remote origin refs/tags/<tag> 'refs/tags/<tag>^{}' refs/heads/main
# and only a line whose ref is exactly one of these counts: a branch named like the tag, or a nested
# tag such as refs/tags/x/<tag>, never does. The tag is annotated exactly when origin reports the
# line refs/tags/<tag>^{}; the commit it names is that line's id.
#
# Every refusal is three labelled lines on stderr, Error, Why and What now, in everyday words, never
# git's own text. The Error line names the subcommand's outcome:
#     version: Error: The version was not read.
#     tag:     Error: Nothing was published.
#     notes:   Error: The Release page's text was not written.
#
# version: prints the workspace version, the `version` of Cargo.toml's [workspace.package] table
# beside this script, as it stands (a suffix such as 0.2.0-rc.1 included), and nothing else; exit 0.
#   V. Cargo.toml holds no version in its [workspace.package] table: exit 1 with
#       Error: The version was not read.
#       Why: Cargo.toml holds no version in its [workspace.package] table.
#       What now: Put the version in the [workspace.package] table of Cargo.toml on main, then run the release again.
#
# tag <tag> <commit>: exit 0 with one line on stdout when every rule holds,
#     Success: The tag <tag> may be released: annotated on origin, version <version>, on main at <commit>.
# otherwise exit 1 with the three labelled lines on stderr, naming the first broken rule only, in
# this order:
#   V. Cargo.toml holds no version in its [workspace.package] table
#       Error: Nothing was published.
#       Why: Cargo.toml holds no version in its [workspace.package] table.
#       What now: Put the version in the [workspace.package] table of Cargo.toml on main, then run the release again.
#   1. the workspace version is not three whole numbers separated by dots (0.2.0-rc.1 is refused)
#       Error: Nothing was published.
#       Why: The version in Cargo.toml, <version>, is not three whole numbers such as 0.2.0; Nage publishes whole versions only.
#       What now: Land a version of three whole numbers in Cargo.toml on main, then tag that commit.
#   2. the tag is not v followed by exactly the workspace version
#       Error: Nothing was published.
#       Why: The tag <tag> is not v followed by this commit's version, <version> (Cargo.toml).
#       What now: Release this commit as v<version>, or land the version you meant in Cargo.toml on main and tag that commit.
#   3. origin cannot be asked: there is no remote named origin
#       Error: Nothing was published.
#       Why: This repository has no remote named origin, so the tag could not be checked.
#       What now: Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to.
#   4. origin cannot be asked: it does not answer (git ls-remote fails, or, at rule 10, the tag's
#      message cannot be fetched and read)
#       Error: Nothing was published.
#       Why: The remote origin did not answer, so the tag could not be checked.
#       What now: Run the release again; if origin still does not answer, check its address with git remote get-url origin.
#   5. origin holds no line refs/tags/<tag>
#       Error: Nothing was published.
#       Why: The remote origin holds no tag <tag>.
#       What now: Push the tag with git push origin <tag>, then run the release again.
#   6. the tag on origin is not annotated (no line refs/tags/<tag>^{}); a checkout may have made the
#      local tag lightweight, so the local tag is never asked
#       Error: Nothing was published.
#       Why: The tag <tag> on origin is a lightweight tag; a release needs an annotated tag, whose message becomes the Release page's text.
#       What now: Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC).
#   7. the tag on origin names another commit than <commit>
#       Error: Nothing was published.
#       Why: The tag <tag> on origin names the commit <tagged>, but this run was given <commit>.
#       What now: Run the release from the tag itself, so that it builds the commit the tag names.
#   8. the local copy of main is not what origin holds: origin reports no refs/heads/main, the
#      local refs/remotes/origin/main is not the commit origin reports for refs/heads/main, or
#      that commit is not in the local repository; or <commit> cannot be judged against it:
#      git merge-base --is-ancestor ends with neither 0 nor 1, as when <commit> is not in the
#      local repository (a shallow clone that does not hold it)
#       Error: Nothing was published.
#       Why: The local copy of main (origin/main) is not the main that origin holds, so the commit could not be looked for on it.
#       What now: Fetch first with git fetch origin main, then run the release again.
#   9. <commit> is neither the commit origin reports for refs/heads/main nor one of its ancestors,
#      judged in the local repository (git merge-base --is-ancestor ends with 1)
#       Error: Nothing was published.
#       Why: The commit <commit> of the tag <tag> is not on main: it is neither the commit origin holds for main nor one of its ancestors.
#       What now: Release under the next version number, tagging a commit that main holds.
#  10. the annotated tag's message on origin is empty, signed or not (read as notes reads it)
#       Error: Nothing was published.
#       Why: The tag <tag> on origin has an empty message; its message becomes the Release page's text.
#       What now: Release under the next version number with a tag whose message says what the release holds (git tag -a, made with TZ=UTC).
#
# notes <tag> <image-with-digest> <guide-address>: prints the Release page's text on stdout, exit
# 0: the annotated tag's own message as origin holds it, a blank line, then exactly these four
# lines, where <image> is the image's name without its @ and digest and <version> the workspace
# version:
#     Image: <image-with-digest>
#     Pull it as: <image>:<version>
#     Deployment guide: <guide-address>
#     To update: put <version> in your Compose file's image line, then pull and start (the guide's "Update to a new version").
# The message is read as described above; an empty message prints as an empty line. Otherwise
# exit 1, nothing on stdout, and on stderr, the first that holds of:
#   V. Cargo.toml holds no version in its [workspace.package] table
#       Error: The Release page's text was not written.
#       Why: Cargo.toml holds no version in its [workspace.package] table.
#       What now: Put the version in the [workspace.package] table of Cargo.toml on main, then run the release again.
#   3. there is no remote named origin
#       Error: The Release page's text was not written.
#       Why: This repository has no remote named origin, so the tag's message could not be read.
#       What now: Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to.
#   4. origin does not answer (git ls-remote or the fetch fails)
#       Error: The Release page's text was not written.
#       Why: The remote origin did not answer, so the tag's message could not be read.
#       What now: Run the release again; if origin still does not answer, check its address with git remote get-url origin.
#   5. origin holds no line refs/tags/<tag>
#       Error: The Release page's text was not written.
#       Why: The remote origin holds no tag <tag>, so there is no message to show.
#       What now: Push the annotated tag with git push origin <tag>, then run the release again.
#   6. the tag on origin is not annotated (no line refs/tags/<tag>^{})
#       Error: The Release page's text was not written.
#       Why: The tag <tag> on origin is a lightweight tag, so it has no message to show.
#       What now: Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC).
#
# Any other use (no subcommand, an unknown one, a wrong number of arguments): exit 2 with
#     Error: The command was not run.
#     Why: release-check takes version, tag <tag> <commit> or notes <tag> <image-with-digest> <guide-address>.
#     What now: Run scripts/release-check.sh with one of them.
set -euo pipefail

# git's own variables, such as GIT_DIR in a hook, would send the git commands below elsewhere.
# shellcheck disable=SC2046 # one variable name per word, split on purpose
unset $(git rev-parse --local-env-vars)

# The repository: the parent of this script's folder (CDPATH cleared, so that cd prints nothing).
repo=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

# refuse <exit> <why> <what now>: the three labelled lines under the subcommand's Error line.
error="The command was not run."
refuse() {
    printf 'Error: %s\nWhy: %s\nWhat now: %s\n' "$error" "$2" "$3" >&2
    exit "$1"
}

usage() {
    error="The command was not run."
    refuse 2 \
        "release-check takes version, tag <tag> <commit> or notes <tag> <image-with-digest> <guide-address>." \
        "Run scripts/release-check.sh with one of them."
}

# git in the repository, which it only reads.
git_here() {
    git -C "$repo" "$@"
}

# The version of Cargo.toml's [workspace.package] table as it stands, or nothing.
workspace_version() {
    local line table="" found=""
    local header='^[[:space:]]*\[+([^]]*)\]+'
    local key="^[[:space:]]*version[[:space:]]*=[[:space:]]*[\"']([^\"']*)[\"']"
    [ -f "$repo/Cargo.toml" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        line=${line%$'\r'}
        if [[ $line =~ $header ]]; then
            table=${BASH_REMATCH[1]}
            table=${table//[[:space:]]/}
            continue
        fi
        if [ "$table" = "workspace.package" ] && [[ $line =~ $key ]]; then
            found=${BASH_REMATCH[1]}
        fi
    done <"$repo/Cargo.toml"
    printf '%s' "$found"
}

# Sets `version`, or refuses rule V.
read_version() {
    version=$(workspace_version)
    if [ -z "$version" ]; then
        refuse 1 \
            "Cargo.toml holds no version in its [workspace.package] table." \
            "Put the version in the [workspace.package] table of Cargo.toml on main, then run the release again."
    fi
}

# Sets `url` to origin's address, or refuses rule 3; <what> ends the Why line.
origin_url() {
    if ! url=$(git_here remote get-url origin 2>/dev/null) || [ -z "$url" ]; then
        refuse 1 \
            "This repository has no remote named origin, so $1 could not be checked." \
            "Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to."
    fi
}

# The refusal of rule 4; <what> ends the Why line.
no_answer() {
    refuse 1 \
        "The remote origin did not answer, so $1." \
        "Run the release again; if origin still does not answer, check its address with git remote get-url origin."
}

# Asks origin for exactly refs/tags/<tag>, its peeled line and refs/heads/main, and sets
# `tag_id`, `peeled` and `main_id` from the lines whose ref is exactly one of these (each empty
# when origin has no such line); refuses rule 4 when origin does not answer.
ask_origin() {
    local tag=$1 what=$2 listing id ref
    if ! listing=$(git_here ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}" refs/heads/main 2>/dev/null); then
        no_answer "$what"
    fi
    tag_id="" peeled="" main_id=""
    while IFS=$'\t' read -r id ref; do
        if [ "$ref" = "refs/tags/$tag" ]; then
            tag_id=$id
        elif [ "$ref" = "refs/tags/$tag^{}" ]; then
            peeled=$id
        elif [ "$ref" = "refs/heads/main" ]; then
            main_id=$id
        fi
    done <<<"$listing"
}

# version
version_command() {
    error="The version was not read."
    read_version
    printf '%s\n' "$version"
}

# The refusal of rule 8: the local copy of main cannot answer whether the commit is on main.
stale_main() {
    refuse 1 \
        "The local copy of main (origin/main) is not the main that origin holds, so the commit could not be looked for on it." \
        "Fetch first with git fetch origin main, then run the release again."
}

# tag <tag> <commit>
tag_command() {
    local tag=$1 commit=$2 local_main ancestry
    error="Nothing was published."
    read_version
    if ! [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        refuse 1 \
            "The version in Cargo.toml, $version, is not three whole numbers such as 0.2.0; Nage publishes whole versions only." \
            "Land a version of three whole numbers in Cargo.toml on main, then tag that commit."
    fi
    if [ "$tag" != "v$version" ]; then
        refuse 1 \
            "The tag $tag is not v followed by this commit's version, $version (Cargo.toml)." \
            "Release this commit as v$version, or land the version you meant in Cargo.toml on main and tag that commit."
    fi
    origin_url "the tag"
    ask_origin "$tag" "the tag could not be checked"
    if [ -z "$tag_id" ]; then
        refuse 1 \
            "The remote origin holds no tag $tag." \
            "Push the tag with git push origin $tag, then run the release again."
    fi
    if [ -z "$peeled" ]; then
        refuse 1 \
            "The tag $tag on origin is a lightweight tag; a release needs an annotated tag, whose message becomes the Release page's text." \
            "Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC)."
    fi
    if [ "$peeled" != "$commit" ]; then
        refuse 1 \
            "The tag $tag on origin names the commit $peeled, but this run was given $commit." \
            "Run the release from the tag itself, so that it builds the commit the tag names."
    fi
    local_main=$(git_here rev-parse --verify --quiet refs/remotes/origin/main 2>/dev/null || true)
    if [ -z "$main_id" ] || [ "$local_main" != "$main_id" ] ||
        ! git_here cat-file -e "$main_id^{commit}" 2>/dev/null; then
        stale_main
    fi
    # 0: an ancestor of main or main itself; 1: not on main; anything else, such as 128 for a
    # commit the local repository does not hold (a shallow clone), cannot be judged here.
    ancestry=0
    git_here merge-base --is-ancestor "$commit" "$main_id" 2>/dev/null || ancestry=$?
    if [ "$ancestry" -ne 0 ] && [ "$ancestry" -ne 1 ]; then
        stale_main
    fi
    if [ "$ancestry" -eq 1 ]; then
        refuse 1 \
            "The commit $commit of the tag $tag is not on main: it is neither the commit origin holds for main nor one of its ancestors." \
            "Release under the next version number, tagging a commit that main holds."
    fi
    read_message "$tag" "the tag could not be checked"
    if [ -z "$message" ]; then
        refuse 1 \
            "The tag $tag on origin has an empty message; its message becomes the Release page's text." \
            "Release under the next version number with a tag whose message says what the release holds (git tag -a, made with TZ=UTC)."
    fi
    printf 'Success: The tag %s may be released: annotated on origin, version %s, on main at %s.\n' \
        "$tag" "$version" "$commit"
}

# Sets `message` to the message of the annotated tag <tag> as origin holds it (origin's address in
# `url`), read in a throwaway bare repository in a temporary folder that is removed on every exit;
# refuses rule 4 when it cannot be fetched or read, <what> ending the Why line.
read_message() {
    local tag=$1 what=$2 raw mark
    # A relative local address is relative to the repository, not to the throwaway.
    case $url in
    /* | *:*) ;;
    *) url="$repo/$url" ;;
    esac
    # The throwaway bare repository, removed on every exit.
    scratch=$(mktemp -d)
    trap 'rm -rf -- "$scratch"' EXIT
    if ! git init -q --bare "$scratch/tag.git" >/dev/null 2>&1 ||
        ! git -C "$scratch/tag.git" fetch -q --no-tags "$url" "+refs/tags/$tag:refs/tags/$tag" >/dev/null 2>&1 ||
        ! raw=$(git -C "$scratch/tag.git" cat-file tag "refs/tags/$tag" 2>/dev/null); then
        no_answer "$what"
    fi
    # The message follows the tag's header and its blank line; a signature is not part of it.
    # The command substitution drops the object's last line ends, so a tag whose message is empty
    # leaves no blank line after its header: its message is then empty, never the header.
    case $raw in
    *$'\n\n'*) message=${raw#*$'\n\n'} ;;
    *) message="" ;;
    esac
    # git's four signature marks: OpenPGP's two, SSH's and X.509's.
    for mark in \
        '-----BEGIN PGP SIGNATURE-----' \
        '-----BEGIN PGP MESSAGE-----' \
        '-----BEGIN SSH SIGNATURE-----' \
        '-----BEGIN SIGNED MESSAGE-----'; do
        message=${message%%"$mark"*}
    done
    while [ "${message%$'\n'}" != "$message" ]; do
        message=${message%$'\n'}
    done
}

# notes <tag> <image-with-digest> <guide-address>
notes_command() {
    local tag=$1 image=$2 guide=$3
    error="The Release page's text was not written."
    read_version
    if ! url=$(git_here remote get-url origin 2>/dev/null) || [ -z "$url" ]; then
        refuse 1 \
            "This repository has no remote named origin, so the tag's message could not be read." \
            "Run the release in a clone of the repository, whose remote origin is the one the tag was pushed to."
    fi
    ask_origin "$tag" "the tag's message could not be read"
    if [ -z "$tag_id" ]; then
        refuse 1 \
            "The remote origin holds no tag $tag, so there is no message to show." \
            "Push the annotated tag with git push origin $tag, then run the release again."
    fi
    if [ -z "$peeled" ]; then
        refuse 1 \
            "The tag $tag on origin is a lightweight tag, so it has no message to show." \
            "Release under the next version number with an annotated tag (git tag -a, made with TZ=UTC)."
    fi
    read_message "$tag" "the tag's message could not be read"
    printf '%s\n\n' "$message"
    printf 'Image: %s\n' "$image"
    printf 'Pull it as: %s:%s\n' "${image%%@*}" "$version"
    printf 'Deployment guide: %s\n' "$guide"
    printf 'To update: put %s in your Compose file'\''s image line, then pull and start (the guide'\''s "Update to a new version").\n' \
        "$version"
}

case ${1-} in
version)
    [ $# -eq 1 ] || usage
    version_command
    ;;
tag)
    [ $# -eq 3 ] || usage
    tag_command "$2" "$3"
    ;;
notes)
    [ $# -eq 4 ] || usage
    notes_command "$2" "$3" "$4"
    ;;
*)
    usage
    ;;
esac
