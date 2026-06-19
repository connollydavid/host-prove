#!/bin/sh
# install-tlaps.sh [dest] — fetch the pinned TLAPS prebuilt installer, verify its
# SHA256, and run it to expose `tlapm`. Linux CI-matrix leg only (the official
# prebuilt installer; no Docker, no OCaml build). macOS/Windows TLAPS is out of scope.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$here/_common.sh"
os=$(uname -s)
[ "$os" = "Linux" ] || { echo "host-prove: install-tlaps.sh runs on Linux only (got $os)" >&2; exit 2; }
ver=$(pin tlaps version); asset=$(pin tlaps asset); sha=$(pin tlaps sha256)
dest=${1:-"$HP_ROOT/.tools/tlaps"}
url="https://github.com/tlaplus/tlapm/releases/download/v$ver/$asset"
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
echo "host-prove: fetching tlaps $ver ..." >&2
curl -fsSL "$url" -o "$tmp"
verify_sha "$tmp" "$sha"
mkdir -p "$dest"
sh "$tmp" -d "$dest" >/dev/null    # self-extracting installer; -d sets the prefix
bin="$dest/bin"
echo "host-prove: tlaps $ver ready ($bin/tlapm)" >&2
[ -n "${GITHUB_PATH:-}" ] && echo "$bin" >> "$GITHUB_PATH"   # CI matrix
echo "$bin"
