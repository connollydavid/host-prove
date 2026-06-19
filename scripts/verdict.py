#!/usr/bin/env python3
"""verdict.py — turn a verifier's raw output into ONE machine-matchable line.

The host-prove skills are built so a small/weak model never has to interpret raw
tool noise. Every wrapper pipes the verifier's combined stdout+stderr through this
parser, which prints exactly one line from a tiny fixed vocabulary and sets the
exit code to match. The agent matches the line; it does not read the tool.

Usage (raw tool output on stdin):
    verdict.py kani     --harness NAME
    verdict.py apalache --mode typecheck --spec FILE
    verdict.py apalache --mode check     --inv NAME
    verdict.py tlaps    --module NAME

Vocabulary / exit code:
    kani      SUCCESSFUL <h>                      0
              FAILED <h> (replay: cargo kani ...) 1
              ERROR <h>: <msg>                    2
    apalache  TYPECHECK-OK <spec>                 0
              TYPE-ERROR <loc>: <msg>             2
              PROVEN <inv>                        0
              VIOLATED <inv> (counterexample)     1
              ERROR: <msg>                        2
    tlaps     ALL-PROVED <module> (<n> obligations)        0
              FAILED <module>: <k>/<n> (first: <loc>)      1
              ERROR <module>: <msg>                        2

Exit 0 = proved/clean, 1 = a real negative verdict, 2 = the tool could not run.
Pure stdlib; deterministic; unit-tested against fixtures in tests/.
"""
import argparse
import re
import sys


def first_error(lines):
    for ln in lines:
        s = ln.strip()
        low = s.lower()
        if low.startswith("error") or "error:" in low or "error[" in low:
            return s
    return "no recognizable verdict in output"


def parse_kani(lines, harness):
    text = "\n".join(lines)
    # The authoritative per-harness verdict line is "VERIFICATION:- SUCCESSFUL/FAILED".
    if re.search(r"VERIFICATION:?-?\s*SUCCESSFUL", text):
        return f"SUCCESSFUL {harness}", 0
    if re.search(r"VERIFICATION:?-?\s*FAILED", text):
        # The agent regenerates a replayable counterexample test deterministically.
        return f"FAILED {harness} (replay: cargo kani --harness {harness} --concrete-playback=print)", 1
    return f"ERROR {harness}: {first_error(lines)}", 2


def parse_apalache(lines, mode, inv, spec):
    text = "\n".join(lines)
    if mode == "typecheck":
        if re.search(r"Type checker\s*\[OK\]", text) or "Typechecking ... succeeded" in text:
            return f"TYPECHECK-OK {spec}", 0
        # Snowcat type errors carry a bracketed source location: [file:L:C-L:C]: msg
        for ln in lines:
            m = re.search(r"\[[^\]]*:(\d+:\d+(?:-\d+:\d+)?)\]:\s*(.*)", ln)
            if m:
                return f"TYPE-ERROR {m.group(1)}: {m.group(2).strip()}", 2
        for ln in lines:  # fallback: an error-ish line with no parseable location
            if "error" in ln.lower() or "typingexception" in ln.lower():
                return f"TYPE-ERROR ?: {ln.strip()}", 2
        return f"ERROR: {first_error(lines)}", 2
    # mode == check
    if "The outcome is: NoError" in text:
        return f"PROVEN {inv}", 0
    if "The outcome is: Error" in text:
        m = re.search(r"Check the trace.*?:\s*(\S+)", text) or re.search(r"(\S*counterexample\S*\.tla)", text)
        cex = m.group(1) if m else "counterexample.tla"
        return f"VIOLATED {inv} (counterexample: {cex})", 1
    return f"ERROR: {first_error(lines)}", 2


def parse_tlaps(lines, module):
    # tlapm --toolbox emits per-obligation messages with `status:` fields; we tolerate
    # both the `@!!status:proved` toolbox form and a bare `status:proved`.
    statuses = []
    last_loc = "?"
    first_fail_loc = None
    for ln in lines:
        m = re.search(r"loc:(\d+:\d+:\d+:\d+)", ln)
        if m:
            last_loc = m.group(1)
        m = re.search(r"status:(\w+)", ln)
        if m:
            st = m.group(1)
            statuses.append(st)
            if st == "failed" and first_fail_loc is None:
                first_fail_loc = last_loc
    total = len(statuses)
    if total == 0:
        return f"ERROR {module}: {first_error(lines)}", 2
    failed = sum(1 for s in statuses if s == "failed")
    if failed == 0:
        return f"ALL-PROVED {module} ({total} obligations)", 0
    return f"FAILED {module}: {failed}/{total} (first: {first_fail_loc or '?'})", 1


def main():
    ap = argparse.ArgumentParser(prog="verdict.py")
    sub = ap.add_subparsers(dest="tool", required=True)
    k = sub.add_parser("kani"); k.add_argument("--harness", required=True)
    a = sub.add_parser("apalache")
    a.add_argument("--mode", choices=["typecheck", "check"], required=True)
    a.add_argument("--inv", default="invariant"); a.add_argument("--spec", default="spec")
    t = sub.add_parser("tlaps"); t.add_argument("--module", required=True)
    args = ap.parse_args()

    lines = sys.stdin.read().splitlines()
    if args.tool == "kani":
        line, code = parse_kani(lines, args.harness)
    elif args.tool == "apalache":
        line, code = parse_apalache(lines, args.mode, args.inv, args.spec)
    else:
        line, code = parse_tlaps(lines, args.module)
    print(line)
    sys.exit(code)


if __name__ == "__main__":
    main()
