#!/usr/bin/env bash
# Packs the program for a GitHub Release (SPEC 5.2; PLAN milestone 2 slice 9). It copies
# /usr/local/bin/nage out of the given image, puts LICENSE and README.md beside it, and writes
# <dir>/nage-<version>-linux-x86_64.tar.gz and <dir>/SHA256SUMS. The release workflow runs it
# twice: as a rehearsal on the image it has just built, and for the Release page on the image it
# published, pulled by its digest, so that the packed program is the published one.
#
#     scripts/release-pack.sh <image> <version> <dir>
#     scripts/release-pack.sh nage:dev 0.2.0 dist     # by hand, after `docker build -t nage:dev .`
#
# It needs docker, tar, gzip and sha256sum on the host, and fails when the program in the image
# does not print the version it was asked to pack. The scratch directory and the stopped container
# it makes are removed on every exit; the image stays.
set -euo pipefail

usage="usage: scripts/release-pack.sh <image> <version> <dir>"
image=${1:?$usage}
version=${2:?$usage}
out=${3:?$usage}

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
name="nage-$version-linux-x86_64"
work=$(mktemp -d)
container=""

cleanup() {
    if [ -n "$container" ]; then
        docker rm "$container" > /dev/null 2>&1 || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

mkdir -p "$work/$name" "$out"
container=$(docker create "$image")
docker cp "$container:/usr/local/bin/nage" "$work/$name/nage"
cp "$repo/LICENSE" "$repo/README.md" "$work/$name/"
chmod 0755 "$work/$name" "$work/$name/nage"
chmod 0644 "$work/$name/LICENSE" "$work/$name/README.md"

# The program is static (decision 68), so it runs here as it does in the image.
printed=$("$work/$name/nage" --version)
if [ "$printed" != "nage $version" ]; then
    echo "scripts/release-pack.sh: the program in $image prints \"$printed\", not \"nage $version\"" >&2
    exit 1
fi

# The same bytes whoever packs it: fixed modes (the folder's too, whatever the umask), a fixed
# order, no owner names, the commit's own time (UTC),
# and no time of packing in the gzip header (-n).
stamp=$(git -C "$repo" log -1 --format=%cI)
TZ=UTC tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="$stamp" \
    -C "$work" -cf - "$name" | gzip -n > "$out/$name.tar.gz"
(cd "$out" && sha256sum "$name.tar.gz" > SHA256SUMS)
echo "packed $out/$name.tar.gz and $out/SHA256SUMS"
