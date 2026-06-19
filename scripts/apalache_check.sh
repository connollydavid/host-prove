#!/bin/sh
# apalache_check.sh typecheck <spec.tla>
# apalache_check.sh check     <spec.tla> <Inv> [extra apalache-mc args...]
# Prints a single verdict line (see scripts/verdict.py).
#   typecheck -> TYPECHECK-OK / TYPE-ERROR <loc>: ...   (exit 0 / 2)
#   check     -> PROVEN / VIOLATED / ERROR              (exit 0 / 1 / 2)
# Always typecheck first: Apalache cannot check a spec whose Snowcat types fail.
set -u
mode=${1:?usage: apalache_check.sh typecheck SPEC.tla  --or--  apalache_check.sh check SPEC.tla INV [args]}
spec=${2:?usage: a spec path is required}
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
base=$(basename "$spec")
case "$mode" in
  typecheck)
    apalache-mc typecheck "$spec" 2>&1 \
      | python3 "$here/verdict.py" apalache --mode typecheck --spec "$base" ;;
  check)
    inv=${3:?invariant name required for check}; shift 3
    apalache-mc check --inv="$inv" "$@" "$spec" 2>&1 \
      | python3 "$here/verdict.py" apalache --mode check --inv "$inv" ;;
  *) echo "ERROR: unknown mode '$mode' (use typecheck|check)"; exit 2 ;;
esac
