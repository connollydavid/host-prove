#!/bin/sh
# kani_check.sh <harness> [crate-dir] — run ONE Kani proof harness and print a
# single verdict line (see scripts/verdict.py): SUCCESSFUL / FAILED / ERROR.
# Exit 0 = proved, 1 = a real counterexample, 2 = the tool could not run.
set -u
harness=${1:?usage: kani_check.sh <harness> [crate-dir]}
dir=${2:-.}
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
( cd "$dir" && cargo kani --harness "$harness" --output-format terse 2>&1 ) \
  | python3 "$here/verdict.py" kani --harness "$harness"
