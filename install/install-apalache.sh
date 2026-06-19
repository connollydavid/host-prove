#!/bin/sh
# install-apalache.sh [dest] — fetch the pinned Apalache (JVM), verify its SHA256,
# and expose `apalache-mc` on PATH. Cross-platform (needs a JRE 17+). No Docker.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$here/_common.sh"
ver=$(pin apalache version); asset=$(pin apalache asset); sha=$(pin apalache sha256)
dest=${1:-"$HP_ROOT/.tools/apalache"}
url="https://github.com/apalache-mc/apalache/releases/download/v$ver/$asset"
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
echo "host-prove: fetching apalache $ver ..." >&2
curl -fsSL "$url" -o "$tmp"
verify_sha "$tmp" "$sha"
mkdir -p "$dest"
tar -xzf "$tmp" -C "$dest" --strip-components=1
bin="$dest/bin"
echo "host-prove: apalache $ver ready ($bin/apalache-mc)" >&2
[ -n "${GITHUB_PATH:-}" ] && echo "$bin" >> "$GITHUB_PATH"   # CI matrix
echo "$bin"
