#!/bin/sh
# tlapm_check.sh <module.tla> [extra tlapm args...] — run TLAPS over a module and
# print a single verdict line (see scripts/verdict.py):
#   ALL-PROVED <module> (<n> obligations)        exit 0
#   FAILED <module>: <k>/<n> (first: <loc>)       exit 1
#   ERROR <module>: <msg>                         exit 2
# `--toolbox 0 0` makes tlapm emit machine-readable per-obligation status.
set -u
mod=${1:?usage: tlapm_check.sh <module.tla> [args...]}
shift
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
name=$(basename "$mod" .tla)
tlapm --toolbox 0 0 "$@" "$mod" 2>&1 \
  | python3 "$here/verdict.py" tlaps --module "$name"
