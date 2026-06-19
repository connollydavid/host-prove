#!/bin/sh
# install-kani.sh — install the pinned Kani via cargo (its native channel), locked.
# `--locked --version` is Kani's reproducibility pin (the cargo analog of a binary
# SHA256); `cargo kani setup` fetches the matching backend. Linux/macOS. No Docker.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$here/_common.sh"
ver=$(pin kani version)
echo "host-prove: installing kani-verifier $ver (cargo, locked) ..." >&2
cargo install --locked kani-verifier --version "$ver"
cargo kani setup
echo "host-prove: kani $ver ready (cargo kani)" >&2
