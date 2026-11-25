#!/usr/bin/env bash
# Browser check of the nage image (PLAN M2 slice 9; SPEC decisions 71 and 121, the Deployment row
# of 10.3). The e2e suite runs the binary built on the machine; this script runs its fresh-server
# checks and its set-up against the image itself, so a file missing only from the image fails here:
# the container, on an empty named volume, leads `/` to its set-up page and answers `/health`, and
# the set-up page, filled in a real browser, sets the book up, signs its first user in and shows
# the home page with the starter accounts. A developer runs it after `docker build -t nage:dev .`:
#
#     scripts/container-e2e.sh nage:dev          # publishes the container on 127.0.0.1:8798
#     scripts/container-e2e.sh nage:dev 18798    # another port when 8798 is busy
#
# It needs docker on the host, the Node version e2e/mise.toml pins on PATH, `npm ci` run in e2e/
# and Chromium's headless shell installed for the pinned Playwright (the Commands table). It refuses
# with one line on stderr and exit status 2 when the image does not exist locally, when Node is not
# the pinned version or when e2e/node_modules is missing. Otherwise it starts the image with TZ=UTC
# and the port published on 127.0.0.1 only, waits up to 60 s for Docker to report the container
# healthy, and runs `npx playwright test --config container.config.ts` in e2e/ with
# NAGE_CONTAINER_URL set. It exits 0 only when every test passed; on any failure it prints the
# container's log and health log and exits 1. The container and its volume are removed on every
# exit (success, failure or Ctrl-C); the image stays.
set -euo pipefail

image=${1:?usage: scripts/container-e2e.sh <image> [port]}
port=${2:-8798}
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

refuse() {
    echo "container-e2e: $1" >&2
    exit 2
}

if ! [[ $port =~ ^[0-9]{1,5}$ ]] || [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    refuse "the port must be a number from 1 to 65535, not '$port'"
fi
command -v docker >/dev/null 2>&1 || refuse "docker is not installed"
docker info >/dev/null 2>&1 || refuse "docker is not reachable; start it, or join the docker group"
docker image inspect "$image" >/dev/null 2>&1 ||
    refuse "the image $image does not exist locally; build it first: docker build -t $image ."
pinned=$(sed -n 's/^node = "\(.*\)"$/\1/p' "$repo/e2e/mise.toml")
[ -n "$pinned" ] || refuse "cannot read the pinned Node version from e2e/mise.toml"
command -v node >/dev/null 2>&1 || refuse "node is not on PATH; e2e/mise.toml pins $pinned"
node_version=$(node --version)
[ "$node_version" = "v$pinned" ] ||
    refuse "Node is $node_version, but e2e/mise.toml pins v$pinned; put that version first on PATH (mise selects it inside e2e/)"
[ -d "$repo/e2e/node_modules" ] || refuse "e2e/node_modules is missing; run npm ci in e2e/ first"

# Names of their own, apart from the smoke's (nage-smoke-*) and from docker compose's; refused
# when taken, so that the cleanup below never removes what another run owns, and a volume is never
# reused (docker volume create succeeds silently on an existing name).
name=nage-container-e2e-$$-$RANDOM
volume=$name-data
url=http://127.0.0.1:$port
if docker container inspect "$name" >/dev/null 2>&1 || docker volume inspect "$volume" >/dev/null 2>&1; then
    refuse "a container or volume named $name already exists; run the script again"
fi

cleanup() {
    docker rm -f -v "$name" >/dev/null 2>&1 || true
    docker volume rm "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

fail() {
    echo "container-e2e: FAILED: $1" >&2
    if docker container inspect "$name" >/dev/null 2>&1; then
        echo "--- docker logs $name" >&2
        docker logs "$name" >&2 2>&1 || true
        # The status and each probe's exit code and output, without Docker's timestamps.
        echo "--- health log" >&2
        docker inspect --format '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{range .State.Health.Log}}
probe exit {{.ExitCode}}: {{.Output}}{{end}}{{else}}no health check{{end}}' "$name" >&2 2>/dev/null || true
    fi
    exit 1
}

wait_healthy() {
    local state=none
    for _ in $(seq 1 60); do
        state=$(docker inspect --format '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$name")
        case "$state" in
            "running healthy") return 0 ;;
            "running starting") sleep 1 ;;
            *) echo "container-e2e: container state: $state" >&2; return 1 ;;
        esac
    done
    echo "container-e2e: not healthy within 60 s: $state" >&2
    return 1
}

echo "container-e2e: starting $image on the empty volume $volume at $url with TZ=UTC"
docker volume create "$volume" >/dev/null || fail "cannot create the volume $volume"
docker run --detach --name "$name" --volume "$volume:/data" --publish "127.0.0.1:$port:8787" \
    --env TZ=UTC "$image" >/dev/null || fail "the container did not start"

echo "container-e2e: waiting for the container to report healthy (60 s at most)"
wait_healthy || fail "the container did not become healthy"

echo "container-e2e: running the browser check (container.config.ts) against $url"
status=0
(cd "$repo/e2e" && NAGE_CONTAINER_URL=$url npx playwright test --config container.config.ts) || status=$?
[ "$status" -eq 0 ] || fail "Playwright exited with status $status"

echo "container-e2e: OK ($image)"
