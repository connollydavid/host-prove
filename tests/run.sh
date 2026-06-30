#!/bin/sh
# Validate the host-prove binary against captured tool-output fixtures. Needs no
# verifier installed — it proves the parser maps real tool output to the right
# one-line verdict + exit code, which is what the skills rely on for weak models.
# Needs `host-prove` on PATH (CI puts target/release on PATH; or `cargo install`).
set -u
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
V=host-prove
pass=0; fail=0

run() {  # <fixture> <expect_code> <expect_prefix> -- <host-prove args...>
  fixture=$1; expect_code=$2; expect=$3; shift 3; [ "$1" = "--" ] && shift
  got=$("$V" "$@" --stdin < "$here/fixtures/$fixture"); code=$?
  case "$got" in
    "$expect"*) line_ok=1 ;;
    *) line_ok=0 ;;
  esac
  if [ "$code" = "$expect_code" ] && [ "$line_ok" = 1 ]; then
    pass=$((pass + 1)); printf 'ok   %-26s -> %s\n' "$fixture" "$got"
  else
    fail=$((fail + 1)); printf 'FAIL %-26s code=%s (want %s)\n     got:  %s\n     want: %s*\n' \
      "$fixture" "$code" "$expect_code" "$got" "$expect"
  fi
}

run kani_success.txt          0 "SUCCESSFUL verify_is_dotted_code"          -- kani --harness verify_is_dotted_code
run kani_failed.txt           1 "FAILED verify_seg_glob (replay:"          -- kani --harness verify_seg_glob
run kani_error.txt            2 "ERROR verify_x: error"                    -- kani --harness verify_x
run apalache_typecheck_ok.txt 0 "TYPECHECK-OK ParallelScan.tla"            -- apalache --mode typecheck --spec ParallelScan.tla
run apalache_typecheck_err.txt 2 "TYPE-ERROR 42:10-42:24:"                 -- apalache --mode typecheck --spec ParallelScan.tla
run apalache_check_noerror.txt 0 "PROVEN ScanEquiv"                        -- apalache --mode check --inv ScanEquiv
run apalache_check_error.txt  1 "VIOLATED ScanEquiv (counterexample:"      -- apalache --mode check --inv ScanEquiv
run tlaps_allproved.txt       0 "ALL-PROVED ParallelScan (2 obligations)"  -- tlaps --module ParallelScan
run tlaps_failed.txt          1 "FAILED ParallelScan: 1/2 not proved (first: 21:1:21:30" -- tlaps --module ParallelScan

echo "----"
printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
