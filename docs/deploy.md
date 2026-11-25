# Nage deployment guide

How to run Nage 0.2.0 with Docker Compose on your own machine. Steps first, look-up tables at the
end.

## What you need

- A Linux machine, x86_64 (amd64).
- Docker with the Compose plugin. `docker compose version` should answer.
- `sudo`.
- `sqlite3`, for backups while Nage runs.

The image is `ghcr.io/bngoller/nage:0.2.0` and anyone can pull it. No Docker? The release page
https://github.com/bngoller/nage/releases/tag/v0.2.0 has the program by itself, with `SHA256SUMS`.

## First start

### Make the data folder

```sh
mkdir nage && cd nage
mkdir -m 0700 data
sudo chown 1000:1000 data
```

Do this before the first start. Docker won't make the folder for you.

### The Compose file

```sh
curl -fsSL -o compose.yaml https://github.com/bngoller/nage/raw/v0.2.0/deploy/compose.example.yaml
```

Open it and set `TZ` to your time zone, written Area/City (`timedatectl list-timezones` lists them).
Leave the quotes in `NAGE_TRUST_PROXY: "true"`. The example publishes Nage's port for this machine
only. To open it straight from your home network, over plain HTTP, swap `- "127.0.0.1:8787:8787"`
under `ports:` for the commented line `- "8787:8787"`. Never on a machine the internet can reach.

### Set the book up at the terminal

```text
$ docker compose run --rm nage init --book-name Household --currency CAD --username sam --display-name Sam
Success: The book "Household" was set up.
  Currency: CAD
  Starter accounts: added
  Admin: sam (Sam)
  Database: /data/nage.sqlite
What now: Start the server if it is not running; sam can then sign in.
```

It asks for sam's password twice first. Choose the currency with care: it can't be changed later.

### Start the server

```sh
docker compose up -d
```

```text
$ docker compose logs --no-log-prefix nage
2026-12-01T20:15:03Z  INFO starting version=0.2.0 bind=0.0.0.0:8787 data_dir=/data tz=Etc/UTC tz_source=TZ log=info trust_proxy=true
2026-12-01T20:15:03Z  INFO database ready path=/data/nage.sqlite applied=0 schema_version=2
2026-12-01T20:15:03Z  INFO check ok rules=10
2026-12-01T20:15:04Z  INFO listening on http://0.0.0.0:8787
```

Open http://127.0.0.1:8787 on the machine itself and sign in. To reach it from your other devices, go
on to [HTTPS in front of Nage](#https-in-front-of-nage).
`docker compose ps` says `healthy` after a few seconds.

### Or set it up in the browser

Skip `nage init`, start the server and open http://127.0.0.1:8787/setup on the machine itself. Until
someone fills it in, anyone who can reach Nage can make themselves the admin, so do it before you put
a proxy in front or open the port.

## HTTPS in front of Nage

The proxy has to pass on the site's name (the browser's `Host`, or `X-Forwarded-Host`) and say that
the visitor came over HTTPS (`X-Forwarded-Proto`). Nage only believes the `X-Forwarded-` headers with
`NAGE_TRUST_PROXY: "true"`. With it off, sign-in still works but the cookie isn't marked `Secure`.
Caddy is tried with every release. nginx was tried once, with a real browser. Tailscale Serve isn't
tried by us.
In the examples, put your machine's name in place of `nage.example`.

### Tailscale Serve

```sh
sudo tailscale serve --bg http://127.0.0.1:8787
```

It prints the HTTPS address on your tailnet. `sudo tailscale serve reset` turns it off again.
Its certificate puts the machine's Tailscale name in a public list of certificates.

### Caddy

```caddyfile
nage.example {
	reverse_proxy 127.0.0.1:8787
}
```

This is for Caddy on the same machine; it sends the right headers by itself. For a name only your
network knows, add `tls internal` inside the braces: Caddy then makes its own certificate, and browsers
warn once.

### nginx

```nginx
server {
    listen 443 ssl;
    server_name nage.example;
    # ssl_certificate and ssl_certificate_key as your certificate needs

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_set_header Host $http_host;
        proxy_set_header X-Forwarded-Host $http_host;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Keep all three `proxy_set_header` lines. Without the first two every form is refused. Without the
third the sign-in cookie isn't marked `Secure`.

### When a form is refused

The page says "This form was not accepted" and the log shows `status=403`. Check that the first log
line ends with `trust_proxy=true` and that the proxy passes on the name your browser shows.

### When the page doesn't load

The machine's firewall may be stopping the proxy's port. Let it in.

## Add another user

```text
$ docker compose exec nage nage user add --by sam --username alex --display-name Alex
Success: alex (Alex) was added.
  Added by: sam
  Admin: no
What now: Tell alex the password; alex can sign in now and change it afterwards.
```

Nage asks for alex's password twice first. Alex changes it after signing in (Menu, Change password).

## Reset a password

```text
$ docker compose exec nage nage user reset-password --by sam --username alex
Success: The password of alex was reset.
  Reset by: sam
  Signed out: alex, on every device
What now: Tell alex the new password; alex can change it after signing in.
```

Forgot your own? Name yourself twice: `--by sam --username sam`.

## Back up

```sh
mkdir -m 0700 copies
sudo chown 1000:1000 copies
```

Copies go here. Never copy `nage.sqlite` alone while Nage runs.

### A safe copy without stopping

```sh
sudo -u '#1000' sqlite3 data/nage.sqlite ".backup 'copies/nage-2026-12-01.sqlite'"
```

Run it as uid 1000, never as root. If `id -u` prints 1000 that's you already, so drop
`sudo -u '#1000'`.

### A copy with Nage stopped

```sh
docker compose stop
sudo cp -a data copies/data-2026-12-01
docker compose start
```

Once stopped, Nage leaves no `-wal` or `-shm` file behind, so the copy is whole.

## Update to a new version

Change the version at the end of the `image:` line in `compose.yaml`. To see what it would do first:

```text
$ docker compose run --rm nage migrate --dry-run
Success: 1 change is waiting; nothing was changed.
  Waiting: 0002
  Database version: 1 (this Nage expects 2)
  Backup: would be written to /data/backups/
What now: Run nage migrate without --dry-run, or start the server.
```

When nothing waits:

```text
$ docker compose run --rm nage migrate --dry-run
Success: Nothing is waiting; nothing was changed.
  Database version: 2
```

Then:

```sh
docker compose pull
docker compose up -d
```

```text
$ docker compose logs --no-log-prefix nage
2026-12-01T20:15:03Z  INFO starting version=0.2.0 bind=0.0.0.0:8787 data_dir=/data tz=Etc/UTC tz_source=TZ log=info trust_proxy=true
2026-12-01T20:15:03Z  INFO backup written path=/data/backups/2026-12-01T20-15-03Z-pre-migrate-0001-to-0.2.0.sqlite
2026-12-01T20:15:03Z  INFO database ready path=/data/nage.sqlite applied=1 schema_version=2
2026-12-01T20:15:03Z  INFO check skipped: no book is set up yet
2026-12-01T20:15:04Z  INFO listening on http://0.0.0.0:8787
```

Nage copies the database into `data/backups/` before it touches it, and the update goes through
whole or not at all. This log is an update from 0.1.0, which had no book yet.

### Old copies in the backups folder

Keep the newest two copies in `data/backups/`. Delete older ones whenever you like.

## Go back to the version before

Put the old version back in the `image:` line and run `docker compose up -d`. If the newer version
already updated the database, the old one won't start. After its starting line the log says:

```text
$ docker compose logs --no-log-prefix nage
Error: Nage 0.2.0 did not start.
Why: A newer Nage wrote this database: it carries the change 0003, and this version knows up to 0002.
What now: Start the newer Nage again. To stay on 0.2.0, restore the backup /data/backups/2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite as the deployment guide shows; entries made since it was written are not in it.
```

To restore the backup it names:

```sh
docker compose stop
mkdir -p set-aside
sudo mv data/nage.sqlite data/nage.sqlite-wal data/nage.sqlite-shm set-aside/
sudo cp data/backups/2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite data/nage.sqlite
sudo chown 1000:1000 data/nage.sqlite
sudo chmod 0600 data/nage.sqlite
docker compose up -d
```

The `-wal` and `-shm` files have to go aside too. `mv` complains when one is missing, and that's
fine. Anything entered after the backup was made isn't in it.

## When something is wrong

### A check reports a broken rule

```text
$ docker compose exec nage nage check
Success: The book passed every check.
  Checked: 10 rules and the links between rows
  Database: /data/nage.sqlite
```

```text
$ docker compose exec nage nage check
Error: The book did not pass every check: 2 problems were found.
Why:
  INV-13 transaction 3: a second opening balance for account 9 (Assets:Checking)
  FK postings row 2 references a missing accounts row
What now: Nothing was changed. Keep a copy of the data folder as it is, and keep the lines above: the deployment guide says where to report them.
```

A start logs the same at ERROR and keeps serving:

```log
2026-12-01T20:15:03Z ERROR check failed problems=2
2026-12-01T20:15:03Z ERROR check: INV-13 transaction 3: a second opening balance for account 9 (Assets:Checking)
2026-12-01T20:15:03Z ERROR check: FK postings row 2 references a missing accounts row
```

Make [a copy with Nage stopped](#a-copy-with-nage-stopped), then [report the lines](#report-a-problem).
This is the check a page means when it sends you to whoever looks after Nage. To restore a good copy,
see [Go back to the version before](#go-back-to-the-version-before).

### Nage health and the answer 503

```text
$ docker compose exec nage nage health
Error: The server's health was not confirmed.
Why: http://127.0.0.1:8787/health answered 503 Service Unavailable.
What now: Read the server's log to see why; it answers 503 while it cannot read its database.
```

The log says why: `docker compose logs --no-log-prefix nage`. When all is well, `nage health` prints
nothing.

### The in-use mark

Start a second server on the same data folder and, after its starting line, you get:

```text
$ docker compose run --rm nage serve
Error: Nage did not start.
Why: A Nage server is running on this data folder (/data).
What now: Stop the other server first, or give this one another data folder with --data-dir or NAGE_DATA_DIR.
```

Don't delete `data/nage.lock` while a server runs. It stays there after a stop, and that's normal.

### A forgotten password

An admin resets it: [Reset a password](#reset-a-password).

### A container that restarts again and again

```text
$ docker compose logs --no-log-prefix nage
Error: Nage did not start.
Why: NAGE_TRUST_PROXY gives "yes", and it takes only true or false.
What now: Set NAGE_TRUST_PROXY to true or false, or unset it, then start Nage again.
```

Fix the setting the log names in `compose.yaml`, then run `docker compose up -d`.

## Report a problem

The form: https://github.com/bngoller/nage/issues/new?template=problem.yml

`nage --help` prints it with your version filled in:

```text
$ docker compose run --rm nage --help
The deployment guide for this version:
  https://github.com/bngoller/nage/blob/v0.2.0/docs/deploy.md
Report a problem:
  https://github.com/bngoller/nage/issues/new?template=problem.yml&version=0.2.0
```

**Strike out names first.** The lines can name your accounts, users and folders, and
the form is public.

- A question or an idea: the Discussions board, https://github.com/bngoller/nage/discussions
- A security problem: "Report a vulnerability" on the repository's Security page,
  https://github.com/bngoller/nage/security/advisories/new. Only the maintainer reads those.

## Look-up tables

### Settings

Environment variables, under `environment:` in `compose.yaml`. An empty value counts as not set.

| Setting | In the image | Without it | What it does |
|---|---|---|---|
| `NAGE_DATA_DIR` | `/data` | `./data` | The data folder |
| `NAGE_BIND` | `0.0.0.0:8787` | `127.0.0.1:8787` | The IP address and port Nage listens on; change it only together with the container port under `ports:` |
| `NAGE_TRUST_PROXY` | not set | off | `"true"` behind an HTTPS proxy: Nage believes `X-Forwarded-Host` and `X-Forwarded-Proto`; only `true` and `false` are taken |
| `TZ` | not set | UTC | The time zone Nage reads "today" in, as the tz database names it (Area/City) |
| `NAGE_TZ` | not set | `TZ`, then UTC | A time zone that wins over `TZ`; the example leaves it out, so that the two cannot disagree |
| `NAGE_LOG` | not set | `info` | What the log shows: a level such as `info` or `warn`, or a filter such as `warn,nage_web=debug` |
| `NO_COLOR` | not set | colour on a terminal | Any value turns colour off |

- Keep the quotes around `"true"`. Quoted, the value reaches Nage as written. On anything but `true`
  or `false` Nage stops, and `restart: unless-stopped` starts it again and again.
- `127.0.0.1:8787:8787` keeps Nage to the machine. Swap in `8787:8787` only on a home network you
  trust. Docker's published ports can go around the firewall, and the set-up page is open until someone
  uses it.

### The data folder

| File | Mode | What it is |
|---|---|---|
| `nage.sqlite` | 0600 | The book: the database |
| `nage.sqlite-wal`, `nage.sqlite-shm` | 0600 | SQLite's companions while Nage runs; a clean stop removes them |
| `nage.lock` | 0600 | The in-use mark; it holds `serve` or `migrate` and stays after a stop |
| `backups/` | 0700 | The copies written before each update, one per update step |

Everything in there belongs to uid 1000, the account Nage runs as.

- The folder has to exist first because the Compose file says `create_host_path: false`. A mistyped
  path then stops the start instead of quietly starting an empty book somewhere else.
- The safe copy runs as uid 1000 because `sqlite3` may create the `-wal` and `-shm` files, and files
  owned by root would keep Nage from opening its own database.
- uid 1000 has to be able to get to the folder. Inside another account's home it often can't, so put
  Nage's folder somewhere like `/srv/nage` then.

No account with uid 1000 on the machine? `sudo` refuses the bare number. This runs the same copy:

```sh
sudo setpriv --reuid 1000 --regid 1000 --clear-groups sqlite3 data/nage.sqlite ".backup 'copies/nage-2026-12-01.sqlite'"
```

- On a restore the `-wal` file goes aside because SQLite would play it back onto the restored copy.
- A backup's name says when it was written (UTC), which change it was taken at and which version did
  the update. Nage keeps one per update step and deletes nothing else.
- The in-use mark may do nothing on some network drives, and a Nage older than 0.2.0 takes none. Run
  one Nage per folder.
- `nage check`, `init`, `user add`, `user reset-password` and `migrate --dry-run` are fine beside a
  running server. `nage migrate` is not.

### The log

One event per line: the time (UTC), the level, the message, then `key=value` fields. These are
examples of each line, not one real start:

```log
2026-12-01T20:15:03Z  INFO starting version=0.2.0 bind=0.0.0.0:8787 data_dir=/data tz=Etc/UTC tz_source=TZ log=info trust_proxy=true
2026-12-01T20:15:03Z  INFO backup written path=/data/backups/2026-12-01T20-15-03Z-pre-migrate-0001-to-0.2.0.sqlite
2026-12-01T20:15:03Z  INFO older backup removed path=/data/backups/2026-01-01T00-00-00Z-pre-migrate-0001-to-0.2.0.sqlite
2026-12-01T20:15:03Z  INFO database ready path=/data/nage.sqlite applied=0 schema_version=2
2026-12-01T20:15:03Z  INFO check ok rules=10
2026-12-01T20:15:03Z  INFO check skipped: no book is set up yet
2026-12-01T20:15:03Z ERROR check failed problems=2
2026-12-01T20:15:03Z ERROR check: FK postings row 2 references a missing accounts row
2026-12-01T20:15:03Z ERROR check could not run error=error returned from database: (code: 1) no such table: postings
2026-12-01T20:15:03Z  INFO listening on http://0.0.0.0:8787
```

| Line | When |
|---|---|
| `starting` | First, with the version and every setting: `bind`, `data_dir`, `tz` and where it came from (`tz_source`), `log` and `trust_proxy` |
| `backup written` | When a change waits: the copy written before the update |
| `older backup removed` | After it, for an older copy of the same step it replaced |
| `database ready` | The database, the number of changes `applied` and its `schema_version` |
| `check ok`, `check skipped`, `check failed` and `check:` | The check of the book: passed, no book yet, or each broken rule at ERROR |
| `check could not run` | The check itself failed, at ERROR, with the database's own words |
| `listening on` | Nage answers from here on |
| `request` | Each request, with `method`, `path` (never what follows `?`), `status`, `latency_ms` and the signed-in `user_id` (a `status=403` line has none); a passing health check only at the level `debug` |
| `shutdown signal received`, then `stopped` | A stop: Nage finishes the requests under way, then ends |

### Commands

With the server running: `docker compose exec nage nage <command>`. With it stopped:
`docker compose run --rm nage <command>`.

| Command | What it does |
|---|---|
| `serve` | Runs the server; the image's own command |
| `health` | Asks the running server whether it is well; exit 0 only when it is |
| `init --book-name <name> --currency <code> --username <name> --display-name <name>` | Sets the book and its first user, an admin, up |
| `user add --by <admin> --username <name> --display-name <name>` | Adds a user who is not an admin |
| `user reset-password --by <admin> --username <name>` | Gives a user a new password and signs them out everywhere |
| `check` | Checks the book; changes nothing |
| `migrate` | Updates the database with the server stopped, after a backup |
| `migrate --dry-run` | Lists the changes that wait; changes nothing |
| `--help`, `--version` | The commands, and the version |

`init`, `user add` and `user reset-password` ask for the password twice. In a script, give
`--password-stdin` and add `-T` to `docker compose exec`, as in `docker compose exec -T nage nage user
add --by sam --username alex --display-name Alex --password-stdin < alex-password.txt`. Delete the
file afterwards.
