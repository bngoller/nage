#!/usr/bin/env bash
# Self-test of the fixture guard (SPEC decisions 77 and 78): synthetic fixtures in a temporary
# directory prove that each rule its checks name fires, that every exemption holds, that the deny
# list is applied as decisions 77 and 78 say, and that nothing the guard prints names an entry, a
# run or a cell, a path name that holds an entry excepted. The fixture-guard CI job runs it before
# the scan, and a developer runs it before a commit that changes fixtures/ or either guard script:
#
#     scripts/fixture-guard-selftest.sh
#
# Each check prints a numbered line naming what it proves; the first failure names the check,
# prints the guard's output and exits 1. The guard's own exit codes are 0 clean, 1 problems,
# 2 usage or tool error. Needs the guard's tools (GNU grep, find, sort, od, tr, sed, iconv, jq
# and sqlite3) plus env and printf.
set -Eeuo pipefail
export LC_ALL=C.UTF-8

here=$(cd "$(dirname -- "$0")" && pwd)
guard=$here/fixture-guard.sh
scratch=$(mktemp -d)
out=$scratch/output
step=start
n=0

cleanup() { rm -rf "$scratch"; }
trap cleanup EXIT
fail() {
    local status=$? line=${BASH_LINENO[0]}
    # With `set -E` the ERR trap also fires inside command substitutions and subshells; only the
    # main shell reports (and the EXIT trap cleans up once).
    [ "$BASHPID" = "$$" ] || exit 1
    echo "fixture-guard-selftest: FAILED at: $step (script line $line, exit status $status)" >&2
    if [ -s "$out" ]; then
        echo "--- the guard's output" >&2
        cat -- "$out" >&2
    fi
    exit 1
}
trap fail ERR
check() {
    n=$((n + 1))
    step="[$n] $1"
    echo "fixture-guard-selftest: [$n] $1"
}

# run EXPECTED LIST ACTIONS ARGS...: runs the guard with FIXTURE_DENY_LIST=LIST (unset when LIST is
# "-") and GITHUB_ACTIONS=ACTIONS (unset when "-"), captures both streams and asserts the exit code.
run() {
    local expected=$1 list=$2 actions=$3 status=0
    local -a unsets=() sets=()
    shift 3
    if [ "$list" = - ]; then
        unsets+=(-u FIXTURE_DENY_LIST)
    else
        sets+=("FIXTURE_DENY_LIST=$list")
    fi
    if [ "$actions" = - ]; then
        unsets+=(-u GITHUB_ACTIONS)
    else
        sets+=("GITHUB_ACTIONS=$actions")
    fi
    # env's options come before its assignments; LC_ALL=C leaves the locale to the guard's own
    # export (decision 77).
    env "${unsets[@]}" "${sets[@]}" LC_ALL=C "$guard" "$@" >"$out" 2>&1 || status=$?
    if [ "$status" -ne "$expected" ]; then
        echo "fixture-guard-selftest: expected exit $expected, got $status" >&2
        return 1
    fi
}
# expect TEXT: the guard's output holds the line fragment TEXT.
expect() {
    if ! grep -q -F -e "$1" -- "$out"; then
        echo "fixture-guard-selftest: missing in the output: $1" >&2
        return 1
    fi
}
# absent TEXT: the guard's output holds TEXT nowhere, whatever its case (grep status 1 passes).
absent() {
    local status=0
    grep -q -i -F -e "$1" -- "$out" || status=$?
    if [ "$status" -ne 1 ]; then
        echo "fixture-guard-selftest: '$1' must not appear in the output (grep status $status)" >&2
        return 1
    fi
}
# fails N: the guard reported exactly N problems.
fails() {
    local found
    found=$(grep -c -F -e 'fixture-guard: FAIL ' -- "$out" || true)
    if [ "$found" -ne "$1" ]; then
        echo "fixture-guard-selftest: expected $1 FAIL line(s), found $found" >&2
        return 1
    fi
}
# layout NAME: a fresh, conforming fixtures directory with one CSV fixture and one clean book in
# WAL mode, like the committed one (decision 58).
layout() {
    local d=$scratch/$1
    mkdir -p "$d/books" "$d/csv/alpha-chequing"
    printf 'Fixtures of the self-test.\n' >"$d/README.md"
    printf 'Date,Description,Amount\n2026-01-02,Coffee,-4.50\n' >"$d/csv/alpha-chequing/sample.csv"
    printf '{"version": 1}\n' >"$d/csv/alpha-chequing/profile.json"
    printf '{"version": 1, "entries": []}\n' >"$d/csv/alpha-chequing/expected.json"
    sqlite3 "$d/books/v0.1.0.sqlite" "pragma journal_mode=wal; create table book(id integer primary key, name text); insert into book values (1, 'Household');" >/dev/null
    printf '%s\n' "$d"
}

# The synthetic deny list: entries at lines 2 to 5 and 7 (line 1 is a comment, line 6 is blank;
# entry 2 is padded with spaces, which the guard trims).
LIST=$'# synthetic entries of the self-test\n  smith \nexample.com\n\xc3\x89mile\n456789\n\no\'brien\n'

check "a conforming layout with a deny list passes, every file scanned, through a symbolic link too, and no side file is left beside the WAL-mode book (decisions 58 and 77)"
d=$(layout conforming)
run 0 "$LIST" - --fixtures-dir "$d"
expect 'deny-list entries: 5'
expect 'OK, 5 file(s) scanned'
fails 0
ls -A "$d/books" >"$out"
absent 'sqlite-'
ln -s "$d" "$scratch/conforming-link"
run 0 "$LIST" - --fixtures-dir "$scratch/conforming-link"
expect 'OK, 5 file(s) scanned'

check "without a list and without a CSV fixture the deny-list scan is skipped with a notice and the run passes, while the layout check and the digit scan still run (decision 78)"
d=$(layout no-csv)
rm -r "$d/csv"
run 0 - - --fixtures-dir "$d"
expect 'deny list: none'
expect 'OK, 2 file(s) scanned'
run 0 - true --fixtures-dir "$d"
expect '::notice title=fixture-guard::'
printf 'stray\n' >"$d/notes.txt"
sqlite3 "$d/books/v0.2.0.sqlite" "create table t(v text); insert into t values ('ref 1234567');"
run 1 - - --fixtures-dir "$d"
expect 'deny list: none'
expect 'notes.txt: outside the layout'
expect 'v0.2.0.sqlite: t.v (value 1): a run of 7 digits beginning 123'
fails 2

check "without a list but with a CSV fixture the run fails, while the digit scan still runs (decision 78)"
d=$(layout list-missing)
printf 'Date,Description\n2026-01-02,ref 1234567\n' >"$d/csv/alpha-chequing/sample.csv"
run 1 - - --fixtures-dir "$d"
expect 'FAIL deny list: FIXTURE_DENY_LIST is unset, empty or holds no entry'
expect 'list-missing/csv/alpha-chequing/sample.csv:2: a run of 7 digits beginning 123'
fails 2

check "a list of blank lines and comments counts as missing; an entry under two characters is a configuration error, and a two-character entry is accepted (decisions 77 and 78)"
d=$(layout list-blank)
run 1 $'\n# only a comment\n   \n' - --fixtures-dir "$d"
expect 'FAIL deny list'
run 2 $'a\n' - --fixtures-dir "$d"
expect 'entry at line 1 is shorter than two characters'
run 0 $'ab\n' - --fixtures-dir "$d"
expect 'deny-list entries: 1'

check "the digit rule: a run of seven digits fails, six pass, and runs starting with 999, eight-digit dates (and only those: month 01-12, day 01-31, exactly eight digits) and the integer part of a period-decimal amount are exempt; the summary names the problem count (decision 77)"
d=$(layout digits)
printf '%s\n' 'Date,Description,Amount' \
    'ref 1234567 fails' \
    'ref 123456 passes' \
    '9991234567 passes' \
    '20260102 passes' \
    '20261345 fails' \
    '18991231 fails' \
    '1234567.89 passes' \
    '123456789.5 passes' \
    '1234567,89 fails' \
    'A1234567 fails' \
    '12345678.1234 fails' \
    '1234567.123 passes' \
    '$ 1,234,567.89 passes' \
    '20261301 fails' \
    '20260132 fails' \
    '202601021 fails' \
    '9981234567 fails' \
    '20260002 fails' \
    '21000101 fails' \
    '20260100 fails' \
    'ref 1234567. fails' \
    '19991231 passes' \
    '9990123456 passes' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv:2: a run of 7 digits beginning 123'
expect 'sample.csv:6: a run of 8 digits beginning 202'
expect 'sample.csv:7: a run of 8 digits beginning 189'
expect 'sample.csv:10: a run of 7 digits beginning 123'
expect 'sample.csv:11: a run of 7 digits beginning 123'
expect 'sample.csv:12: a run of 8 digits beginning 123'
expect 'sample.csv:15: a run of 8 digits beginning 202'
expect 'sample.csv:16: a run of 8 digits beginning 202'
expect 'sample.csv:17: a run of 9 digits beginning 202'
expect 'sample.csv:18: a run of 10 digits beginning 998'
expect 'sample.csv:19: a run of 8 digits beginning 202'
expect 'sample.csv:20: a run of 8 digits beginning 210'
expect 'sample.csv:21: a run of 8 digits beginning 202'
expect 'sample.csv:22: a run of 7 digits beginning 123'
expect 'FAILED, 14 problem(s) in 5 file(s) scanned'
fails 14
absent '1234567'

check "a CSV with a UTF-16 byte-order mark fails, and so does one holding a NUL byte (decision 77)"
d=$(layout encodings)
printf '\xff\xfeD\x00a\x00t\x00e\x00\n\x00' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv: a UTF-16 byte-order mark'
fails 1
printf 'Date,Description\n2026-01-02,abc\x00def 1234567\n' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv: a NUL byte'
fails 1

check "a Windows-1252 export: the deny list runs on the decoded text, so an accented entry matches its single-byte form, and a digit run on the same line is found; a byte that is neither UTF-8 nor Windows-1252 fails (decision 77)"
d=$(layout cp1252)
printf 'Date,Description\n2026-01-02,CAF\xc9 \xc9MILE 20260102 \xc91234567\n' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv:2: a run of 7 digits beginning 123'
expect 'sample.csv:2: deny-list entry 4'
fails 2
printf 'Date,Description\n2026-01-02,PAID \x81\n' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv: neither UTF-8 nor Windows-1252'
fails 1

check "deny-list entries match as fixed strings and whole words, whatever the case, an all-digit entry at digit boundaries even right after a letter and never with a digit after it, README.md by the deny list alone even on a line holding an encoding error, and a hit names the entry's line number, never the entry (decision 77)"
d=$(layout deny)
printf 'Fixtures of the self-test.\nCaf\xe9 paid to Smith, tel 456789, ref 1234567.\n' >"$d/README.md"
printf '%s\n' 'Date,Description' \
    'John SMITH paid' \
    'smithfield foods' \
    'mail a@example.com' \
    'exampleXcom is not the domain' \
    'C456789 is a number glued to a letter' \
    'tel 456789 x' \
    "$(printf '\xc3\xa9mile wrote')" \
    '14567890 is a longer number' \
    'tel 4567890 x' >"$d/csv/alpha-chequing/sample.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'sample.csv:2: deny-list entry 2'
expect 'sample.csv:4: deny-list entry 3'
expect 'sample.csv:6: deny-list entry 5'
expect 'sample.csv:7: deny-list entry 5'
expect 'sample.csv:8: deny-list entry 4'
expect 'sample.csv:9: a run of 8 digits beginning 145'
expect 'sample.csv:10: a run of 7 digits beginning 456'
absent 'sample.csv:10: deny-list'
expect 'deny/README.md:2: deny-list entry 2'
expect 'deny/README.md:2: deny-list entry 5'
fails 9
absent 'smith'
absent 'example.com'
absent '456789'
absent 'mile'

check "JSON: every string value (a repeated key's values and nested values included, a line break read as a space) and every key of the parsed document (nested keys included) is scanned by both rules, numbers by the deny list alone, a hit never prints the string, and a file jq cannot parse fails (decision 77)"
d=$(layout json)
printf '%s\n' '{"note": "ref 1234567", "note": "fine", "9991234": 1, "amount": 1234567, "acct": 456789, "k 7654321": true, "who": "John Smith"}' >"$d/csv/alpha-chequing/profile.json"
printf '%s\n' '{"rules": [{"memo": "Paid to\nSmith family", "example.com": 1}]}' >"$d/csv/alpha-chequing/rules.json"
printf 'not json\n' >"$d/csv/alpha-chequing/expected.json"
run 1 "$LIST" - --fixtures-dir "$d"
# profile.json, strings in document order: 1 'ref 1234567', 2 'fine', 3 'John Smith'; then the keys
# in sorted order: 4 '9991234', 5 'acct', 6 'amount', 7 'k 7654321', 8 'note', 9 'who'. Numbers: 1,
# 1234567, 456789. rules.json: string 1 'Paid to Smith family' (its escaped line break read as a
# space); keys 2 'rules', 3 'example.com', 4 'memo'.
expect 'profile.json: JSON string 1: a run of 7 digits beginning 123'
expect 'profile.json: JSON string 7: a run of 7 digits beginning 765'
expect 'profile.json: JSON string 3: deny-list entry 2'
expect 'profile.json: JSON number 3: deny-list entry 5'
expect 'rules.json: JSON string 1: deny-list entry 2'
expect 'rules.json: JSON string 3: deny-list entry 3'
expect 'expected.json: not valid JSON'
fails 7
absent 'smith'
absent '1234567'
absent '7654321'
absent '456789'
absent 'example.com'

check "a book: TEXT values are scanned and integers and blobs are not; hashes and argon2 strings are exempt, a short all-digit value and a value merely starting with \$ are not; a NUL byte in a value fails; a WITHOUT ROWID table is read; SQLite's own tables are skipped; a place never shows the value, even one holding | (decision 77)"
d=$(layout book)
sqlite3 "$d/books/v0.1.0.sqlite" \
    "create table t(id integer primary key autoincrement, a text, b integer, c blob, h text);
     insert into t(a, b, c, h) values ('Ref 1234567 | here', 1234567, x'4142', 'a3f5e9b2c1d4f6a7b8c9d0e1f2a3b4c5d6e7f8091234567abcdef0123456789ab');
     insert into t(a, b, c, h) values ('\$argon2id\$v=19\$m=19456,t=2,p=1\$abc1234567def', 2, x'00', 'John Smith');
     insert into t(a, b, c, h) values (cast(x'610062003132333435363738' as text), 3, null, 'fine');
     create table wr(k text primary key, v text) without rowid;
     insert into wr values ('k', 'Visit example.com on 20260102');
     insert into wr values ('o', 'Note from O''Brien');
     create table zz(v text);
     insert into zz values ('Paid to' || char(10) || 'Smith family');
     insert into zz values ('A long memo of more than 32 characters, ref 1234567');
     create table zzz(v text);
     insert into zzz values ('9876543210');
     insert into zzz values ('\$ ref 1234567');
     insert into zzz values ('0123456789abcdef0123456789abcdef');
     insert into sqlite_sequence(name, seq) values ('ref 1234567 in a system table', 9);"
run 1 "$LIST" - --fixtures-dir "$d"
# The values in table, column and row order: book.name is 1; t.a's hash-free, NUL-free, non-argon2
# value is 2; t.h's 'John Smith' is 3 and its 'fine' 4; wr.k's 'k' and 'o' are 5 and 6; wr.v's two
# values 7 and 8 (an apostrophe stays an apostrophe); zz.v's two values 9 (its line break read as a
# space) and 10 (a long value that is not a hash); zzz.v's 11 (ten digits, too short for a hash) and
# 12 (a dollar sign without argon2); its 32-hex value is exempt and not counted.
expect 'v0.1.0.sqlite: t.a (value 2): a run of 7 digits beginning 123'
expect 'v0.1.0.sqlite: t.h (value 3): deny-list entry 2'
expect 'v0.1.0.sqlite: t.a: 1 TEXT value(s) holding a NUL byte'
expect 'v0.1.0.sqlite: wr.v (value 7): deny-list entry 3'
expect 'v0.1.0.sqlite: wr.v (value 8): deny-list entry 7'
expect 'v0.1.0.sqlite: zz.v (value 9): deny-list entry 2'
expect 'v0.1.0.sqlite: zz.v (value 10): a run of 7 digits beginning 123'
expect 'v0.1.0.sqlite: zzz.v (value 11): a run of 10 digits beginning 987'
expect 'v0.1.0.sqlite: zzz.v (value 12): a run of 7 digits beginning 123'
fails 9
absent 't.b'
absent 't.c'
absent 'sqlite_sequence'
absent '1234567'
absent "o'brien"

check "an empty book and a file that is not a database fail (decision 77)"
d=$(layout bad-books)
: >"$d/books/v0.2.0.sqlite"
printf 'not a database at all\n' >"$d/books/v0.3.0.sqlite"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'v0.2.0.sqlite: holds no table'
expect 'v0.3.0.sqlite: cannot be opened as a SQLite database'
fails 2

check "the layout: a stray file (inside a fixture directory too), a side file, a misnamed book, a directory outside the naming rule (uppercase or hyphenless), a symbolic link and a missing required file (sample.csv included) fail, rules.json is allowed, and path names are matched against the deny list (decisions 76 and 77)"
d=$(layout layout)
printf 'stray\n' >"$d/notes.txt"
: >"$d/books/v0.1.0.sqlite-wal"
cp "$d/books/v0.1.0.sqlite" "$d/books/notes.sqlite"
mkdir "$d/csv/Bad-Name" "$d/csv/smith-visa" "$d/csv/beta" "$d/csv/gamma-savings"
printf 'Date\n' >"$d/csv/Bad-Name/sample.csv"
cp "$d/csv/alpha-chequing/sample.csv" "$d/csv/alpha-chequing/profile.json" "$d/csv/smith-visa/"
cp "$d/csv/alpha-chequing/sample.csv" "$d/csv/alpha-chequing/profile.json" "$d/csv/alpha-chequing/expected.json" "$d/csv/beta/"
cp "$d/csv/alpha-chequing/profile.json" "$d/csv/alpha-chequing/expected.json" "$d/csv/gamma-savings/"
printf '[]\n' >"$d/csv/alpha-chequing/rules.json"
printf 'Date\n' >"$d/csv/alpha-chequing/export.csv"
ln -s "$d/README.md" "$d/csv/alpha-chequing/link.csv"
run 1 "$LIST" - --fixtures-dir "$d"
expect 'notes.txt: outside the layout'
expect 'books/v0.1.0.sqlite-wal: outside the layout'
expect 'books/notes.sqlite: outside the layout'
expect 'csv/Bad-Name/: a directory outside the layout'
expect 'csv/Bad-Name/sample.csv: outside the layout'
expect 'csv/beta/: a directory outside the layout'
expect 'csv/alpha-chequing/export.csv: outside the layout'
expect 'csv/alpha-chequing/link.csv: not a regular file or directory'
expect 'csv/smith-visa/expected.json: required by the layout but missing'
expect 'csv/gamma-savings/sample.csv: required by the layout but missing'
expect 'csv/smith-visa: deny-list entry 2'
absent 'rules.json: outside'
fails 18

check "argument mode scans the given files and directories by type, reads a candidate of any other name as a CSV, and a missing list fails there too; a --fixtures-dir that is not a directory and a temporary directory the guard cannot create end with exit 2 (decision 77)"
d=$(layout arguments)
printf 'Date,Description\n2026-01-02,ref 1234567\n' >"$scratch/candidate.csv"
run 1 - - "$scratch/candidate.csv"
expect 'FAIL deny list'
expect 'candidate.csv:2: a run of 7 digits beginning 123'
fails 2
cp "$scratch/candidate.csv" "$scratch/Export.TXT"
run 1 "$LIST" - "$scratch/Export.TXT"
expect 'Export.TXT (csv)'
expect 'Export.TXT:2: a run of 7 digits beginning 123'
fails 1
printf 'not json 1234567\n' >"$scratch/Profile.JSON"
run 1 "$LIST" - "$scratch/Profile.JSON"
expect 'Profile.JSON (json)'
expect 'Profile.JSON: not valid JSON'
fails 1
run 0 "$LIST" - "$d/csv/alpha-chequing" "$d/books/v0.1.0.sqlite"
expect 'OK, 4 file(s) scanned'
run 1 "$LIST" - "$scratch/does-not-exist"
expect 'does-not-exist: does not exist'
run 1 "$LIST" - ""
expect ': does not exist'
run 2 "$LIST" - --fixtures-dir "$scratch/does-not-exist"
expect 'is not a directory'
status=0
env TMPDIR="$scratch/does-not-exist" "$guard" --help >"$out" 2>&1 || status=$?
[ "$status" -eq 2 ]
expect 'cannot create a temporary directory'

check "under GitHub Actions a text line's problem is also an error annotation naming the file (a comma in it percent-escaped) and the line, a JSON string hit's names the file and no line, and a path hit's carries no file property (decision 77)"
d=$(layout 'anno,tations')
printf 'Date,Description\n2026-01-02,ref 1234567\n' >"$d/csv/alpha-chequing/sample.csv"
printf '{"who": "John Smith"}\n' >"$d/csv/alpha-chequing/profile.json"
mkdir "$d/csv/smith-visa"
cp "$d/csv/alpha-chequing/expected.json" "$d/csv/smith-visa/"
printf '{"version": 1}\n' >"$d/csv/smith-visa/profile.json"
printf 'Date,Description\n2026-01-02,Coffee\n' >"$d/csv/smith-visa/sample.csv"
run 1 "$LIST" true --fixtures-dir "$d"
expect '::error file=anno%2Ctations/csv/alpha-chequing/sample.csv,line=2::anno,tations/csv/alpha-chequing/sample.csv:2: a run of 7 digits beginning 123'
expect '::error file=anno%2Ctations/csv/alpha-chequing/profile.json::anno,tations/csv/alpha-chequing/profile.json: JSON string 1: deny-list entry 2'
expect '::error::anno,tations/csv/smith-visa: deny-list entry 2'
absent '::error file=anno%2Ctations/csv/smith-visa'
absent 'profile.json,line='

trap - ERR
echo "fixture-guard-selftest: OK, $n checks"
