#!/usr/bin/env bash
# Fixture guard (PLAN milestone 1 slice 8 and section 6; SPEC decisions 76, 77 and 78). The
# fixture-guard CI job runs it on every pull request; a developer runs it before a commit that
# touches fixtures/ or either guard script, and before a push whose commits touch fixtures/:
#
#     scripts/fixture-guard.sh                     # the repository's fixtures/, layout checked
#     scripts/fixture-guard.sh --fixtures-dir DIR  # another fixtures directory, layout checked
#     scripts/fixture-guard.sh PATH...             # candidate files or directories, by type
#
# Rules (decision 77): every path under fixtures/ fits the layout fixtures/README.md describes; a
# CSV fixture holds no UTF-16 byte-order mark and no NUL byte; no CSV line, JSON string or key, or
# TEXT value of a seeded book holds a run of seven or more digits unless it starts with 999, is an
# eight-digit date or is the integer part of a period-decimal amount; no file, path name or value
# holds an entry of the deny list, read from FIXTURE_DENY_LIST (one entry per line; in CI the
# repository secret of the same name). While fixtures/csv/ holds no file, a missing deny list only
# skips that scan with a notice; once it holds one, a missing list is a failure (decision 78).
# No matched entry, run or cell is printed outside a path name: a problem is its place and a hint.
# Exit 0 when nothing was found, 1 on any problem, 2 on a usage or tool error. Needs bash, GNU grep
# with PCRE, find, sort, od, tr, sed, iconv, jq and the sqlite3 command-line tool.
set -Eeuo pipefail
# ASCII-only digit classes and Unicode case folding, whatever the caller's locale (decision 77).
export LC_ALL=C.UTF-8

# A run of seven or more ASCII digits that is not exempt (decision 77): not preceded by a digit (so
# the run is maximal and the 999 test sees its start), not starting with 999, not an eight-digit
# date of 1900 to 2099 (month 01 to 12, day 01 to 31) and not followed by a period and one to
# three digits (the integer part of an amount). The possessive quantifier `{7,}+` keeps the engine
# from backtracking into a shorter run that would slip past the amount test.
DIGIT_RE='(?<![0-9])(?!999)(?!(?:19|20)[0-9]{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])(?![0-9]))[0-9]{7,}+(?!\.[0-9]{1,3}(?![0-9]))'

# One SELECT per column of every table but SQLite's own: the TEXT values that are not a hash (32 or
# more hexadecimal characters), not a password hash ($argon2...) and hold no NUL byte (those are
# reported by the NUL query), each on one line as `table.column|value`, its line breaks turned into
# spaces and nothing quoted, so an apostrophe or a word after a line break meets the deny list as
# written (decision 77).
# Virtual and shadow tables and generated columns are not read (a known limit of decision 77).
read -r -d '' VALUES_GENERATOR <<'EOF' || true
select printf(
    'select %s || replace(replace("%w", char(10), '' ''), char(13), '' '') from "%w" where typeof("%w") = ''text'' and instr(cast("%w" as blob), x''00'') = 0 and not (length("%w") >= 32 and "%w" not glob ''*[^0-9a-fA-F]*'') and "%w" not like ''$argon2%%'';',
    quote(l.name || '.' || i.name || '|'), i.name, l.name, i.name, i.name, i.name, i.name, i.name)
from pragma_table_list l, pragma_table_info(l.name) i
where l.schema = 'main' and l.type = 'table' and l.name not like 'sqlite\_%' escape '\'
order by l.name, i.cid;
EOF
# One SELECT per column counting the TEXT values that hold a NUL byte, which the tool would
# otherwise cut off silently (decision 77).
read -r -d '' NUL_GENERATOR <<'EOF' || true
select printf(
    'select %s || count(*) from "%w" where typeof("%w") = ''text'' and instr(cast("%w" as blob), x''00'') > 0;',
    quote(l.name || '.' || i.name || '|'), l.name, i.name, i.name)
from pragma_table_list l, pragma_table_info(l.name) i
where l.schema = 'main' and l.type = 'table' and l.name not like 'sqlite\_%' escape '\'
order by l.name, i.cid;
EOF

problems=0
files=0
entries=()
entry_lines=()
scratch=$(mktemp -d) || { echo "fixture-guard: error: cannot create a temporary directory (TMPDIR)" >&2; exit 2; }
cleanup() { rm -rf "$scratch"; }
trap cleanup EXIT
fail() {
    local status=$? line=${BASH_LINENO[0]}
    # With `set -E` the ERR trap also fires inside command substitutions and subshells; only the
    # main shell reports (and the EXIT trap cleans up once).
    [ "$BASHPID" = "$$" ] || exit 2
    echo "fixture-guard: internal error at script line $line (exit status $status)" >&2
    exit 2
}
trap fail ERR

usage() {
    cat <<'EOF'
usage: scripts/fixture-guard.sh                     scan the repository's fixtures/ (layout checked)
       scripts/fixture-guard.sh --fixtures-dir DIR  scan DIR as a fixtures directory (layout checked)
       scripts/fixture-guard.sh PATH...             scan candidate files or directories by type
The deny list is read from FIXTURE_DENY_LIST, one entry per line. Exit 0 clean, 1 problems, 2 error.
EOF
}
die() {
    echo "fixture-guard: error: $*" >&2
    exit 2
}
note() { echo "fixture-guard: $*"; }
# escape TEXT: a workflow-command property value (%, carriage return, line feed, colon and comma
# percent-encoded, as GitHub's runner expects).
escape() {
    local s=$1
    s=${s//%/%25}
    s=${s//$'\r'/%0D}
    s=${s//$'\n'/%0A}
    s=${s//:/%3A}
    s=${s//,/%2C}
    printf '%s' "$s"
}
# problem PLACE HINT [FILE [LINE]]: one FAIL line, plus an annotation under GitHub Actions.
problem() {
    local where="" message
    problems=$((problems + 1))
    echo "fixture-guard: FAIL $1: $2"
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        if [ -n "${3:-}" ]; then
            where="file=$(escape "$3")"
            if [ -n "${4:-}" ]; then
                where="$where,line=$4"
            fi
        fi
        message="$1: $2"
        message=${message//%/%25}
        if [ -n "$where" ]; then
            echo "::error $where::$message"
        else
            echo "::error::$message"
        fi
    fi
}

# The deny list: one entry per line of FIXTURE_DENY_LIST, trimmed; blank lines and lines starting
# with # are ignored; an entry under two characters is a configuration error (decisions 77 and 78).
load_deny_list() {
    local n=0 line
    while IFS= read -r line || [ -n "$line" ]; do
        n=$((n + 1))
        line=${line%$'\r'}
        line=${line#"${line%%[![:space:]]*}"}
        line=${line%"${line##*[![:space:]]}"}
        case $line in
            '' | '#'*) continue ;;
        esac
        if [ "${#line}" -lt 2 ]; then
            die "FIXTURE_DENY_LIST entry at line $n is shorter than two characters"
        fi
        entries+=("$line")
        entry_lines+=("$n")
    done <<<"${FIXTURE_DENY_LIST:-}"
}

# report MODE PATH LINE TEXT HINT: one problem at the place that MODE and LINE name (a CSV or text
# line, a JSON string or number, a book value, a path name).
report() {
    local mode=$1 path=$2 line=$3 text=$4 hint=$5 item
    case $mode in
        csv | text) problem "$path:$line" "$hint" "$path" "$line" ;;
        json) problem "$path: JSON string $line" "$hint" "$path" ;;
        number) problem "$path: JSON number $line" "$hint" "$path" ;;
        book)
            item=$(sed -n "${line}p" -- "$text")
            problem "$path: ${item%%|*} (value $line)" "$hint" "$path"
            ;;
        path)
            # No file= property: GitHub masks a secret in an annotation's message, not in its
            # properties, and a path that matches an entry holds the entry.
            item=$(sed -n "${line}p" -- "$text")
            problem "$path/$item" "$hint"
            ;;
    esac
}

# scan_digits TEXT MODE PATH: the digit rule over every line of TEXT.
scan_digits() {
    local text=$1 mode=$2 path=$3 hits line run
    hits=$({ grep -a -o -n -P -e "$DIGIT_RE" -- "$text" || [ $? -eq 1 ]; })
    [ -n "$hits" ] || return 0
    while IFS=: read -r line run; do
        report "$mode" "$path" "$line" "$text" "a run of ${#run} digits beginning ${run:0:3}"
    done <<<"$hits"
}

# scan_deny TEXT MODE PATH: every deny-list entry over TEXT, a fixed string matched without regard
# to case as a whole word, or at digit boundaries when the entry is all digits (so a number glued
# to letters is still found); the hint names the entry's line number in the list, never the entry.
scan_deny() {
    local text=$1 mode=$2 path=$3 i entry lines line
    for i in "${!entries[@]}"; do
        entry=${entries[i]}
        if [[ $entry =~ ^[0-9]+$ ]]; then
            lines=$({ grep -a -n -P -e "(?<![0-9])${entry}(?![0-9])" -- "$text" || [ $? -eq 1 ]; } | LC_ALL=C sed 's/:.*//' | sort -n -u)
        else
            lines=$({ grep -a -i -w -n -F -e "$entry" -- "$text" || [ $? -eq 1 ]; } | LC_ALL=C sed 's/:.*//' | sort -n -u)
        fi
        [ -n "$lines" ] || continue
        while IFS= read -r line; do
            report "$mode" "$path" "$line" "$text" "deny-list entry ${entry_lines[i]}"
        done <<<"$lines"
    done
}

# scan_book FILE PATH: a copy of the book is opened read-only (the committed file never is,
# decision 58); every TEXT value VALUES_GENERATOR reads goes through both rules.
scan_book() {
    local file=$1 path=$2 copy tables queries nuls values column count
    copy=$scratch/book-$files.sqlite
    cp -- "$file" "$copy"
    if ! tables=$(sqlite3 -readonly -batch -bail -noheader "$copy" "select count(*) from sqlite_master where type = 'table';" 2>/dev/null); then
        problem "$path" "cannot be opened as a SQLite database" "$path"
        return 0
    fi
    if [ "$tables" -eq 0 ]; then
        problem "$path" "holds no table (an empty database)" "$path"
        return 0
    fi
    queries=$scratch/nul-$files.sql
    sqlite3 -readonly -batch -bail -noheader "$copy" "$NUL_GENERATOR" >"$queries"
    nuls=$(sqlite3 -readonly -batch -bail -noheader "$copy" <"$queries")
    while IFS='|' read -r column count; do
        [ -n "$column" ] || continue
        if [ "$count" -gt 0 ]; then
            problem "$path: $column" "$count TEXT value(s) holding a NUL byte" "$path"
        fi
    done <<<"$nuls"
    queries=$scratch/values-$files.sql
    sqlite3 -readonly -batch -bail -noheader "$copy" "$VALUES_GENERATOR" >"$queries"
    values=$scratch/values-$files.txt
    sqlite3 -readonly -batch -bail -noheader "$copy" <"$queries" >"$values"
    scan_digits "$values" book "$path"
    scan_deny "$values" book "$path"
}

# scan_file FILE PATH: the rules that apply to the file's kind, by extension (any letter case);
# with paths given as arguments, a file that is neither JSON nor a book is a candidate CSV.
scan_file() {
    local file=$1 path=$2 kind text numbers bom
    files=$((files + 1))
    case ${file,,} in
        *.csv) kind=csv ;;
        *.json) kind=json ;;
        *.sqlite) kind=book ;;
        *)
            if [ "$mode" = args ]; then
                kind=csv
            else
                kind=text
            fi
            ;;
    esac
    note "scanned $path ($kind)"
    case $kind in
        csv)
            bom=$(od -An -tx1 -N2 -- "$file" | tr -d ' ')
            if [ "$bom" = fffe ] || [ "$bom" = feff ]; then
                problem "$path" "a UTF-16 byte-order mark (W3 refuses UTF-16; save the export as UTF-8 or Windows-1252)" "$path" 1
                return 0
            fi
            if [ "$(tr -d -c '\000' <"$file" | wc -c)" -ne 0 ]; then
                problem "$path" "a NUL byte (not a text file)" "$path"
                return 0
            fi
            scan_digits "$file" csv "$path"
            if iconv -f UTF-8 -t UTF-8 <"$file" >/dev/null 2>&1; then
                text=$file
            else
                # Not UTF-8: W3 reads such a file as Windows-1252 (unless it starts with a UTF-8
                # byte-order mark), so the deny list is matched against the decoded text (line
                # numbers unchanged; UTF-8 text in such a file is decoded too, a known limit).
                text=$scratch/decoded-$files
                if ! iconv -f WINDOWS-1252 -t UTF-8 <"$file" >"$text" 2>/dev/null; then
                    problem "$path" "neither UTF-8 nor Windows-1252" "$path"
                    return 0
                fi
            fi
            scan_deny "$text" csv "$path"
            ;;
        json)
            # Every string value in document order (a repeated key's values included), then every
            # object's keys in sorted order, one per line with line breaks read as spaces; the
            # numbers separately, for the deny list alone (minor-unit amounts have seven digits).
            # Keys come from the parsed document, so a key inside a value that a later repeated key
            # replaces is not scanned (a known limit of decision 77).
            text=$scratch/strings-$files
            numbers=$scratch/numbers-$files
            if ! {
                jq -r --stream 'select(length == 2) | .[1] | strings | gsub("[\r\n]"; " ")' -- "$file" &&
                    jq -r '[.. | objects | keys[]] | .[] | gsub("[\r\n]"; " ")' -- "$file"
            } >"$text" 2>/dev/null; then
                problem "$path" "not valid JSON" "$path"
                return 0
            fi
            jq -r --stream 'select(length == 2) | .[1] | numbers | tostring' -- "$file" >"$numbers" 2>/dev/null
            scan_digits "$text" json "$path"
            scan_deny "$text" json "$path"
            scan_deny "$numbers" number "$path"
            ;;
        book) scan_book "$file" "$path" ;;
        text) scan_deny "$file" text "$path" ;;
    esac
}

# check_layout ROOT BASE: every entry under ROOT fits fixtures/README.md's layout (decision 77).
check_layout() {
    local root=$1 base=$2 entry type rel dir name required
    while IFS= read -r -d '' entry; do
        type=${entry%% *}
        rel=${entry#* }
        case $type in
            d)
                if ! [[ $rel =~ ^(books|csv|csv/[a-z0-9]+(-[a-z0-9]+)+)$ ]]; then
                    problem "$base/$rel/" "a directory outside the layout (see fixtures/README.md)" "$base/$rel"
                fi
                ;;
            f)
                if ! [[ $rel =~ ^(README\.md|books/v[0-9]+\.[0-9]+\.[0-9]+\.sqlite|csv/[a-z0-9]+(-[a-z0-9]+)+/(sample\.csv|profile\.json|expected\.json|rules\.json))$ ]]; then
                    problem "$base/$rel" "outside the layout (see fixtures/README.md)" "$base/$rel"
                fi
                ;;
            *) problem "$base/$rel" "not a regular file or directory" "$base/$rel" ;;
        esac
    done < <(find "$root" -mindepth 1 -printf '%y %P\0' | sort -z)
    for dir in "$root"/csv/*/; do
        [ -d "$dir" ] || continue
        name=$(basename -- "$dir")
        for required in sample.csv profile.json expected.json; do
            if [ ! -f "$dir$required" ]; then
                problem "$base/csv/$name/$required" "required by the layout but missing" "$base/csv/$name/$required"
            fi
        done
    done
}

mode=layout
root=""
targets=()
if [ $# -eq 0 ]; then
    root=$(cd "$(dirname -- "$0")/.." && pwd)/fixtures
else
    case $1 in
        --help | -h)
            usage
            exit 0
            ;;
        --fixtures-dir)
            [ $# -eq 2 ] || die "--fixtures-dir takes exactly one directory"
            root=$2
            ;;
        --*) die "unknown option $1 (try --help)" ;;
        *)
            mode=args
            targets=("$@")
            ;;
    esac
fi
for tool in grep find sort od tr sed iconv jq sqlite3; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is not installed"
done
printf 'a\n' | grep -q -P '^a$' 2>/dev/null || die "grep without PCRE support (-P) cannot run the digit rule"
load_deny_list

fixture_present=yes
base=""
if [ "$mode" = layout ]; then
    [ -d "$root" ] || die "$root is not a directory"
    # The physical path: find does not descend into a symbolic link given as its starting point.
    root=$(cd "$root" && pwd -P)
    base=$(basename -- "$root")
    fixture_present=no
    if [ -d "$root/csv" ] && [ -n "$(find "$root/csv" -type f -print -quit)" ]; then
        fixture_present=yes
    fi
fi
if [ "${#entries[@]}" -eq 0 ]; then
    if [ "$fixture_present" = yes ]; then
        problem "deny list" "FIXTURE_DENY_LIST is unset, empty or holds no entry while a fixture exists (decision 78)"
    else
        note "deny list: none; FIXTURE_DENY_LIST is unset, empty or holds no entry, and the deny-list scan is skipped while fixtures/csv/ holds no file (decision 78)"
        if [ -n "${GITHUB_ACTIONS:-}" ]; then
            echo "::notice title=fixture-guard::the deny-list scan was skipped: FIXTURE_DENY_LIST is unset, empty or holds no entry and fixtures/csv/ holds no file (decision 78)"
        fi
    fi
else
    note "deny-list entries: ${#entries[@]}"
fi

if [ "$mode" = layout ]; then
    check_layout "$root" "$base"
    if [ "${#entries[@]}" -gt 0 ]; then
        find "$root" -mindepth 1 -printf '%P\n' | sort >"$scratch/paths"
        scan_deny "$scratch/paths" path "$base"
    fi
    while IFS= read -r -d '' file; do
        scan_file "$file" "$base/${file#"$root/"}"
    done < <(find "$root" -type f -print0 | sort -z)
else
    for target in "${targets[@]}"; do
        if [ -f "$target" ]; then
            scan_file "$target" "$target"
        elif [ -d "$target" ]; then
            while IFS= read -r -d '' file; do
                scan_file "$file" "$file"
            done < <(find "$target" -type f -print0 | sort -z)
        else
            problem "$target" "does not exist"
        fi
    done
fi

if [ "$problems" -gt 0 ]; then
    note "FAILED, $problems problem(s) in $files file(s) scanned"
    exit 1
fi
note "OK, $files file(s) scanned"
