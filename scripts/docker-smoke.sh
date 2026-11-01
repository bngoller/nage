#!/usr/bin/env bash
# Container smoke for the nage image (SPEC 5.2 and 10.3 "Deployment"; SPEC decision 71). The
# docker CI job runs it after `docker build --tag nage:ci .`, and a developer runs the same file
# after `docker build -t nage:dev .`:
#
#     scripts/docker-smoke.sh nage:dev          # publishes the container on 127.0.0.1:8787
#     scripts/docker-smoke.sh nage:dev 18787    # another port when 8787 is busy
#
# It needs docker with the compose plugin, curl, tar, jq and file on the host. Every check says
# what it proves and is numbered in the output. The first failure names the script line and its
# exit status, prints the container's logs and health log when the container exists, and exits 1;
# the container, its volume and the scratch directory are removed on every exit (success, failure
# or Ctrl-C); the image stays.
set -Eeuo pipefail

image=${1:?usage: scripts/docker-smoke.sh <image> [port]}
port=${2:-8787}
repo=$(cd "$(dirname "$0")/.." && pwd)
name=nage-smoke-$$
volume=$name-data
scratch=$(mktemp -d)
step=start
n=0

cleanup() {
    docker rm -f -v "$name" "$name-image" >/dev/null 2>&1 || true
    docker volume rm "$volume" >/dev/null 2>&1 || true
    rm -rf "$scratch"
}
trap cleanup EXIT
fail() {
    local status=$? line=${BASH_LINENO[0]}
    # With `set -E` the ERR trap also fires inside command substitutions and subshells; only the
    # main shell reports (and the EXIT trap cleans up once).
    [ "$BASHPID" = "$$" ] || exit 1
    echo "docker-smoke: FAILED at: $step (script line $line, exit status $status)" >&2
    # Checks 1 to 5 run before the container exists.
    if docker container inspect "$name" >/dev/null 2>&1; then
        echo "--- docker logs $name" >&2
        docker logs "$name" >&2 2>&1 || true
        echo "--- health log" >&2
        docker inspect --format '{{json .State.Health}}' "$name" >&2 2>/dev/null || true
    fi
    exit 1
}
trap fail ERR
check() { n=$((n + 1)); step="[$n] $1"; echo "docker-smoke: [$n] $1"; }
# A negated grep (`! grep`) is ignored by `set -e`, so absence is asserted through a function;
# only grep's "no match" status (1) counts as absent, an error (2) fails.
absent() {
    local status=0
    grep -qE "$1" "$2" || status=$?
    if [ "$status" -ne 1 ]; then
        echo "docker-smoke: '$1' must not match in $(basename "$2") (grep status $status)" >&2
        return 1
    fi
}

wait_healthy() {
    local state=none
    for _ in $(seq 1 60); do
        state=$(docker inspect --format '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$name")
        case "$state" in
            "running healthy") return 0 ;;
            "running starting") sleep 1 ;;
            *) echo "docker-smoke: container state: $state" >&2; return 1 ;;
        esac
    done
    echo "docker-smoke: not healthy within 60 s: $state" >&2
    return 1
}

run_container() {
    docker run --detach --name "$name" --volume "$volume:/data" --publish "127.0.0.1:$port:8787" \
        --env TZ=America/Toronto "$image" >/dev/null
}

check "the FROM rust tag equals rust-toolchain.toml's channel (decision 68)"
channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$repo/rust-toolchain.toml")
from_tag=$(sed -n 's/^FROM .*rust:\([0-9][0-9.]*\)-.*$/\1/p' "$repo/Dockerfile")
[ -n "$channel" ]
[ "$channel" = "$from_tag" ]

check "compose.yaml is a valid Compose file (decision 70)"
# -f reads the committed file alone: a local compose.override.yaml or a COMPOSE_FILE is not merged.
(cd "$repo" && docker compose -f compose.yaml config --quiet)

check "the image configuration is SPEC 5.2's: user 1000:1000, NAGE_DATA_DIR and NAGE_BIND, the exec-form health check on nage health, the entrypoint and command, no VOLUME, the source label (decision 69)"
config=$(docker image inspect --format '{{json .Config}}' "$image")
[ "$(jq -r '.User' <<<"$config")" = "1000:1000" ]
jq -e '(.Env | any(. == "NAGE_DATA_DIR=/data")) and (.Env | any(. == "NAGE_BIND=0.0.0.0:8787"))' <<<"$config" >/dev/null
[ "$(jq -c '.Healthcheck.Test' <<<"$config")" = '["CMD","/usr/local/bin/nage","health"]' ]
[ "$(jq -c '.Entrypoint' <<<"$config")" = '["/usr/local/bin/nage"]' ]
[ "$(jq -c '.Cmd' <<<"$config")" = '["serve"]' ]
[ "$(jq -r '.Labels["org.opencontainers.image.source"]' <<<"$config")" = "https://github.com/bngoller/nage" ]
[ "$(jq -c '.Volumes' <<<"$config")" = null ]
echo "  image size: $(docker image inspect --format '{{.Size}}' "$image") bytes"

check "the binary is statically linked and compiled by the rustc rust-toolchain.toml pins, and /data is an empty directory owned by 1000:1000 with mode 0700 (SPEC 5.2; decision 68)"
docker create --name "$name-image" "$image" >/dev/null
docker cp "$name-image:/usr/local/bin/nage" "$scratch/nage"
file "$scratch/nage" | tee "$scratch/file.txt"
grep -E 'static(ally|-pie) linked' "$scratch/file.txt" >/dev/null
grep -aqF "rustc version $channel (" "$scratch/nage"
docker cp "$name-image:/data" - | tar -tv --numeric-owner | tee "$scratch/data.txt"
grep -E '^drwx------ +1000/1000 .* data/$' "$scratch/data.txt" >/dev/null
[ "$(wc -l <"$scratch/data.txt")" = 1 ]
docker rm -v "$name-image" >/dev/null

check "the binary runs in the runtime image: --version prints the version (SPEC 5.2; PLAN M1 slice 2)"
docker run --rm "$image" --version | grep -E '^nage [0-9]+\.[0-9]+\.[0-9]+'

check "the container starts on an empty named volume with the port published on loopback and TZ=America/Toronto (PLAN M1 slice 6)"
docker volume create "$volume" >/dev/null
run_container

check "the HEALTHCHECK reports healthy within 60 s (decision 69)"
wait_healthy
docker inspect --format '{{json .State.Health}}' "$name"

check "nage health inside the container exits 0 (PLAN M1 slice 6; SPEC 10.3)"
docker exec "$name" /usr/local/bin/nage health

check "/health and / answer through the published port, so NAGE_BIND=0.0.0.0:8787 reaches the host (SPEC 5.2)"
curl --fail --silent --show-error --max-time 10 --output "$scratch/health.json" "http://127.0.0.1:$port/health"
cat "$scratch/health.json"; echo
grep -E '"ok":true' "$scratch/health.json" >/dev/null
[ "$(curl --silent --max-time 10 --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:$port/")" = 200 ]

check "the start-up log names the zone, the bind address, /data and every migration applied, and no /health request appears at the default level (decisions 43, 47)"
migrations=$(find "$repo/migrations" -name '*.sql' | wc -l)
docker logs "$name" >"$scratch/logs1.txt" 2>&1
grep -E 'America/Toronto' "$scratch/logs1.txt" >/dev/null
grep -E 'listening on http://0\.0\.0\.0:8787' "$scratch/logs1.txt" >/dev/null
grep -E "applied=$migrations( |\$)" "$scratch/logs1.txt" >/dev/null
grep -E 'data_dir=/data( |$)' "$scratch/logs1.txt" >/dev/null
absent '/health' "$scratch/logs1.txt"

check "docker stop ends the server cleanly as PID 1: exit 0, the stopped line, within the 10 s grace period (decisions 45, 69)"
started=$(date +%s)
docker stop "$name" >/dev/null
elapsed=$(( $(date +%s) - started ))
echo "  stopped in $elapsed s"
[ "$(docker inspect --format '{{.State.ExitCode}}' "$name")" = 0 ]
[ "$elapsed" -lt 10 ]
docker logs "$name" >"$scratch/logs2.txt" 2>&1
grep -E 'shutdown signal received' "$scratch/logs2.txt" >/dev/null
grep -E 'stopped' "$scratch/logs2.txt" >/dev/null

check "the data volume holds nage.sqlite owned by 1000:1000 and no -wal or -shm file after the clean stop (SPEC 5.2)"
docker cp "$name:/data" - | tar -tv --numeric-owner | tee "$scratch/data2.txt"
grep -E ' 1000/1000 .* data/nage\.sqlite$' "$scratch/data2.txt" >/dev/null
absent 'nage\.sqlite-(wal|shm)' "$scratch/data2.txt"

check "a second start on the same volume applies no migration and is healthy again (SPEC 5.5)"
docker rm -v "$name" >/dev/null
run_container
wait_healthy
docker logs "$name" 2>&1 | grep -E 'applied=0( |$)' >/dev/null
docker stop "$name" >/dev/null

trap - ERR
echo "docker-smoke: OK, $n checks ($image)"
