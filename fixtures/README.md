# Fixtures

The test fixtures of Nage: anonymized bank exports under `csv/`, one seeded book per released version under `books/`, and this file. The layout, the anonymization checklist, the hand-over procedure and the guard that enforces them are SPEC decisions 76, 77 and 78; the checklist's origin is PLAN section 6. Nothing here names a bank, a host or a household member, and nothing real is ever committed.

## Layout

```
fixtures/
  README.md                      this file
  books/
    v<version>.sqlite            a seeded book per released version, written by `cargo xtask seed`
  csv/
    <alias>-<kind>/              one export format
      sample.csv                 the anonymized export, byte for byte except the replacements below
      profile.json               the import profile that parses it (milestone 3's profile format)
      expected.json              the expected parse and first-import counts (schema below)
      rules.json                 optional: rules the fixture's golden tests apply
```

`<alias>` is a neutral word in lowercase letters, digits and hyphens that is not derived from the bank's name (`maple`, `cedar`); which alias stands for which bank is written down by the maintainer outside the repository, nowhere in it. `<kind>` names the account kind (`chequing`, `savings`, `card`). The guard checks the shape of a directory name (lowercase words of letters and digits joined by hyphens), not the words themselves, so an alias or kind that names the bank or one of its products passes it unless the word is on the deny list. The guard fails on any other path under `fixtures/`, on a directory name outside that shape, and on a `csv/` directory missing one of its three required files.

## Anonymization checklist (every export, before it enters the repository)

Keep verbatim: header rows and their spelling, column order, delimiter, quoting, encoding and byte-order mark, line endings, date format, sign conventions, number formatting, trailer or summary lines, blank rows, pending rows if the bank exports them, and a few descriptions with accents, commas and quotes.

Replace:

- The bank's own name, abbreviation or domain, and the name of any product, account package or rewards programme that belongs to it, wherever it appears (a title line, a trailer, a column header, a description): write the alias, or a neutral word of the same shape, in its place, keeping the letter case and the punctuation around it. This is the one exception to "keep verbatim"; such names belong on the deny list, so the guard enforces it.
- Account numbers, cheque and reference numbers, card digits and bank ids: a number of the same length that starts with `999`. Alphanumeric identifiers and e-transfer references: a value of the same shape, a letter where a letter was and a digit where a digit was, any run of seven or more digits starting with `999`. Distinct numbers stay distinct: a duplicate bank id in one file is a parse error.
- Names and e-transfer counterparties: generic ones. Descriptions become generic but keep their shape; an e-transfer line stays an e-transfer line.

Amounts may be altered; keep signs, magnitudes and any running-balance column arithmetically consistent. An amount whose digits form a run of seven or more passes the guard only when the run is followed by a period and one to three decimals (`1234567.89`); otherwise reduce it (an export that writes whole cents reaches seven digits at 10,000.00).

Include at least: a transfer pair visible from both accounts; two rows with the same amount and date and different descriptions; a refund; a month boundary; for a multi-account export, rows from at least two accounts, and any USD rows the bank emits, which the profile must filter.

Edit in a plain-text editor, never a spreadsheet (a spreadsheet rewrites quoting, dates, encodings and numbers on save), set to keep line endings and trailing spaces and not to add a missing final newline (in Neovim, `:setlocal nofixendofline`). Before copying, check that the anonymized copy kept the original's encoding, byte-order mark and line endings: `file -e csv original.csv copy.csv` reports the same kind (`-e csv` skips file's CSV test, whose report leaves out the line terminators), `head -c 3 <file> | od -An -tx1` shows the same byte-order mark (the UTF-8 mark `0xEF 0xBB 0xBF` in both or in neither), and `tail -c 2 <file> | od -An -tx1` the same ending (`0d 0a`, a final `0a`, or no line break). Copy the file into the tree byte for byte (`cp`); the repository's `.gitattributes` keeps git from converting their line endings, and its `.editorconfig` lifts the repository's own editor rules (UTF-8, LF, a final newline, trimmed trailing spaces, the indentation) for the CSV files under `fixtures/csv/`; an editor's own defaults still apply (Neovim adds a missing final newline on write), inside the tree as outside it, so re-edit a fixture only with the editor set as above.

## Hand-over

A real export never enters the working tree, a branch or a Claude Code session: the history is public and permanent. The maintainer anonymizes a copy outside the repository, runs the guard on it with the deny list (`FIXTURE_DENY_LIST="$(cat ~/.config/nage/fixture-deny-list)" scripts/fixture-guard.sh /path/to/candidate.csv`), fixes what it reports, then copies the file into `fixtures/csv/<alias>-<kind>/sample.csv` and adds the profile and the expected parse. The fixture is committed in milestone 3's pull request, with the guard live.

## `expected.json` (schema version 1)

A plain JSON document, written by `cargo xtask fixture-expected fixtures/csv/<alias>-<kind>` (milestone 3 slice 2) and compared by that slice's golden test; a change in parsing is regenerated with that command and reviewed in `git diff`. Pretty-printed with two-space indentation, keys in the order below, a final newline.

```json
{
  "version": 1,
  "minor_digits": 2,
  "rows_in_file": 3,
  "entries": [
    {
      "selector": null,
      "rows": [
        {
          "row_index": 0,
          "date": "2026-01-02",
          "amount": -4550,
          "description": "GROCER 9991234 MAPLE ST",
          "payee": null,
          "memo": null,
          "bank_id": "999123456"
        }
      ],
      "first_import": {
        "created": 1,
        "matched_transfer": 0,
        "linked_manual": 0,
        "skipped_duplicate": 0,
        "skipped_deleted": 0,
        "skipped_filtered": 2,
        "review": 0
      }
    }
  ]
}
```

- `version`: the schema version, 1; a later milestone that changes the shape bumps it.
- `minor_digits`: the book currency's minor digits, so the amounts can be read.
- `rows_in_file`: every data row of the file, filtered ones included (the import's `row_count`).
- `entries`: one per selector value, ordered by selector value; a single-account file has one entry with `"selector": null`.
- `rows`: the surviving rows in file order; `row_index` is the import engine's index over all data rows (0-based, filtered rows counted); `amount` is an integer in minor units, positive meaning money into the account; `payee`, `memo` and `bank_id` are strings or null.
- `first_import`: the outcome counts of a first import of this file into an empty book, every key of the outcome set present (W4; in an empty book only `created` and `skipped_filtered` can be nonzero).

Rule outcomes are not in this file: rule application, the summaries of imports other than a fixture's first, and report models are insta goldens beside their tests (SPEC 10.1, decision 60).

## The guard

`scripts/fixture-guard.sh` scans `fixtures/` (decision 77); `scripts/fixture-guard-selftest.sh` proves the rules its checks name on synthetic fixtures. CI runs both on every pull request (the `fixture-guard` job, decision 78); run both before a commit that changes anything under `fixtures/` or either script, and run the guard with the deny list before a push whose commits touch `fixtures/`, because CI runs after the push, when the file is already public.

What it checks:

- The layout above, for every path under `fixtures/`.
- A CSV fixture: no UTF-16 byte-order mark and no NUL byte (the import refuses UTF-16, and a byte scan cannot see through either); no run of seven or more digits unless it starts with `999`, is an eight-digit date between 19000101 and 20991231 (month 01 to 12, day 01 to 31) or is the integer part of an amount written with a period and one to three decimals (`1234567.89`); a decimal comma is never exempt. A file that is not valid UTF-8 is decoded from Windows-1252 for the deny list, as the import reads it when the file has no UTF-8 byte-order mark, and fails as `neither UTF-8 nor Windows-1252` when it holds one of the five bytes iconv leaves undefined (0x81, 0x8D, 0x8F, 0x90, 0x9D), which the import would read. Blind spots, which are the deny list's job: a number written in groups split by any non-digit (spaces, hyphens, periods, slashes), and anything shorter than seven digits.
- A JSON file: the same digit rule and the deny list over every string value (a repeated key's values included) and every key of the parsed document; numbers go through the deny list alone (minor-unit amounts have seven digits from 10,000.00); a file jq cannot parse fails (an empty file passes).
- A book: a copy is opened read-only (the committed file never is); every TEXT value of every ordinary table but SQLite's own goes through both rules, each value read on one line with its line breaks as spaces; integers and BLOBs are never scanned; a value of 32 or more hexadecimal characters (a hash) and a value starting with `$argon2` in any letter case (a password hash) are exempt; a TEXT value holding a NUL byte, an empty file and a file that is not a database fail.
- The deny list, over every file and every path name under `fixtures/` (a path name gets the deny list only: a number used as an alias passes unless it is listed).

Known limits: the CSV scan reads lines, not cells, so a quoted field that holds a line break is read as two lines and a phrase split across them is missed; the date exemption reads the shape only, so `20260230` passes and an eight-digit real number shaped like a date is the deny list's job, as is a date-shaped run inside an identifier (`AB20260102CD`); a run followed by a period and one to three digits passes even when it is an identifier (`1234567.001`), while a fractional part of seven or more digits fails (`0.1234567`); JSON numbers are never scanned for digit runs; an object key inside a value that a later repeated key replaces is not scanned; a phrase entry matches only with single spaces between its words, and a CR LF pair inside a JSON string or a book value is read as two spaces, so a phrase spanning it does not match; no Unicode normalization is applied (a composed and a decomposed accent are different strings); a TEXT value that is itself JSON (from milestone 3: `import_rows.raw`, `imports.header`, a profile's configuration, a rule's conditions) is scanned as stored, so an escaped line break or tab glues the next word and a JSON number of seven or more digits inside it fails; a virtual table, its shadow tables and a stored generated column are not read (the schema defines none; a migration that adds one amends decision 77 first); a CSV that is UTF-8 but for a stray byte is decoded from Windows-1252 throughout, so an accented entry is missed where the file spells it in UTF-8, while an ASCII entry is still found unless a non-ASCII character stands next to it; an entry that is also an ordinary word matches this file's prose and a book's account names too, so list such a first name with the surname, or as the phrases it appears in.

The deny list (`FIXTURE_DENY_LIST`): one entry per line, trimmed; blank lines and lines starting with `#` are ignored (so an entry cannot start with `#`); an entry shorter than two characters is an error, and a list that holds no entry at all counts as missing. Entries are matched as fixed strings, case-insensitively (accented letters included), as whole words: letters, digits and underscore are word characters, so `example.com` matches inside `a@example.com` while `cedar` does not match `cedarwood`; an entry that is all digits matches at digit boundaries wherever it stands, so `456789` is found inside `C456789` but not inside `14567890`; an entry should be a whole name, number, domain or phrase, never a fragment that begins or ends with punctuation (`@example.com` matches only after a space or another non-word character, so never inside `a@example.com`). What belongs in it: the real account, card, cheque and reference numbers, the bank's names and domains (a two-letter abbreviation included) and the names of its products, account packages and programmes, the household members' and counterparties' names, addresses and mailbox addresses, spelled as the exports spell them, accents included (an ASCII spelling matches only an ASCII occurrence). The list lives in two places and nowhere else: the maintainer's private file outside the repository (`~/.config/nage/fixture-deny-list`, mode 0600) for local runs, and the repository secret `FIXTURE_DENY_LIST` for CI, created with `gh secret set FIXTURE_DENY_LIST < ~/.config/nage/fixture-deny-list` before milestone 3's pull request. While `fixtures/csv/` holds no file, a missing list only skips the deny-list scan with a notice; once it holds one, a missing list fails the run. A pull request from a fork receives no repository secret, so it fails the job once fixtures exist.

Reading a failure: `fixture-guard: FAIL <place>: <hint>`. A digit run is reported as `<file>:<line>: a run of N digits beginning <three digits>`; a deny-list hit as `<file>:<line>: deny-list entry K`, where K is the entry's line number in your list (comments and blank lines count), and never the entry itself; a book value as `<file>: <table>.<column> (value N)`, N counting every TEXT value the guard read (the tables but SQLite's own in name order, each table's columns in order and its rows in the order SQLite reads them (storage order, or the order of an index that holds the column); exempt and NUL-bearing values are not counted); a JSON string as `<file>: JSON string N` (the Nth string value in document order, a repeated key's values included, then the keys, each object's keys in sorted order; a line break inside a string is read as a space) and a JSON number as `<file>: JSON number N`; a problem with a whole file (its encoding, a file that is not JSON or not a database) names the file alone, a layout problem names the path, and a missing list `deny list`. The run log is public: the guard never prints a matched entry, run or cell, though a path that matches an entry is necessarily named. Exit codes: 0 clean; 1 problems (a missing list with a fixture present and a path argument that does not exist included); 2 a usage error, a missing tool or a grep without PCRE, a `--fixtures-dir` that is not a directory, a temporary directory the guard cannot create, a CSV, a book or (with a list) a text file the guard cannot read (an unreadable JSON file is reported as not valid JSON), or a short entry. `scripts/fixture-guard.sh <path>...` scans files or directories outside the repository with the layout check off and without the path-name scan; there a file that is neither JSON nor a book is read as a CSV whatever its name, and the list is always required.

Tools: bash, GNU grep (with PCRE), find, sort, od, tr, sed, iconv, jq and the `sqlite3` command-line tool (Arch: `sudo pacman -S --needed sqlite jq`; Debian and Ubuntu: `sudo apt install sqlite3 jq`; the rest comes with the base system), all present on the CI runner.

## Seeded books

`fixtures/books/v<version>.sqlite` is written by `cargo xtask seed --out fixtures/books/v<version>.sqlite` through core operations and committed in each milestone's pull request (PLAN 1.3 item 5; decision 58). Tests copy a book before opening it, and so does the guard; never open a committed book in place (SQLite would create `-wal` and `-shm` files beside it, which the layout check rejects). `v0.1.0.sqlite` is a freshly migrated database with no book row.
