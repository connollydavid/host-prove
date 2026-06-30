//! host-prove — run ONE verifier and print ONE machine-matchable verdict line.
//!
//! A weak/small model issues a single command — `host-prove kani --harness X` — and the tool
//! **runs the verifier itself** (`cargo kani` / `apalache-mc` / `tlapm` via `std::process`) and
//! parses the output in the same process, printing exactly one line from a tiny fixed vocabulary
//! and setting the exit code to match. No shell wrapper, no pipe, no multi-step orchestration the
//! agent can fumble — the tool carries the process (the host-prove thesis). It is an agent-UX +
//! lane-runner aid, **off the discharge trust path**: discharge is the verifier passing on
//! re-derivation (`call/0018`), which is exactly what this runs. Pure std (the verifier itself is
//! the only subprocess — no Rust API exists for it). Deterministic parser, fixture-tested.
//! (Replaces the former `verdict.py` + `*_check.sh` shell wrappers — no unpinned runtime, no glue.)
//!
//! Run a verifier (the default — tool must be on PATH):
//!   host-prove kani     --harness NAME [--dir CRATE] [--bound unwind=K]
//!   host-prove apalache --mode typecheck --spec FILE
//!   host-prove apalache --mode check     --spec FILE --inv NAME [--bound length=N]
//!   host-prove tlaps    --module FILE.tla
//! Parse already-captured output instead of running (testing / piping): add `--stdin`.
//!
//! Vocabulary / exit (0 = proved/clean, 1 = a real negative verdict, 2 = the tool could not run):
//!   kani      SUCCESSFUL <h> [bound=..] | FAILED <h> (replay: ..) | ERROR <h>: <msg>
//!   apalache  TYPECHECK-OK <spec> | TYPE-ERROR <loc>: <msg>
//!             PROVEN <inv> [bound=..] | VIOLATED <inv> (counterexample: ..) | ERROR: <msg>
//!   tlaps     ALL-PROVED <module> (<n> obligations) [unbounded]
//!             FAILED <module>: <k>/<n> (first: <loc>) | ERROR <module>: <msg>

use std::io::Read;
use std::process::{self, Command, Output};

/// The first error-ish line, or a fallback — what to show when no verdict is recognized.
fn first_error(lines: &[&str]) -> String {
    for ln in lines {
        let s = ln.trim();
        let low = s.to_lowercase();
        if low.starts_with("error") || low.contains("error:") || low.contains("error[") {
            return s.to_string();
        }
    }
    "no recognizable verdict in output".to_string()
}

/// The soundness bound (`#9`) carried on a PASS verdict for a bounded tool: required, so an
/// absent one is recorded as `unspecified` for the consumer (`obligations --prove`) to flag.
fn bound_suffix(bound: Option<&str>) -> String {
    match bound {
        Some(b) => format!(" [bound={b}]"),
        None => " [bound=unspecified]".to_string(),
    }
}

/// `path/Mod.tla` or `Mod` -> `Mod` (the display name the verdict carries).
fn module_name(spec: &str) -> &str {
    let base = spec.rsplit(['/', '\\']).next().unwrap_or(spec);
    base.strip_suffix(".tla").unwrap_or(base)
}

fn parse_kani(lines: &[&str], harness: &str, bound: Option<&str>) -> (String, i32) {
    // The authoritative result is the summary line "Complete - <a> ... <b> failures, <c> total":
    // a non-zero failure count is a refutation. The per-harness "VERIFICATION:- SUCCESSFUL/FAILED"
    // lines are the fallback when no summary was captured.
    let summary_failures = lines.iter().find_map(|l| {
        let before = l.get(..l.find(" failures")?)?;
        before
            .rsplit(|c: char| !c.is_ascii_digit())
            .find(|s| !s.is_empty())
            .and_then(|n| n.parse::<u64>().ok())
    });
    let verdict_line = |needle: &str| lines.iter().any(|l| l.contains("VERIFICATION") && l.contains(needle));
    // Fail closed: any FAILED line, or any reported failure, dominates a SUCCESSFUL line. The old
    // SUCCESSFUL-first order reported a clean proof when a refuted harness was also present.
    if verdict_line("FAILED") || summary_failures.is_some_and(|n| n > 0) {
        (
            format!("FAILED {harness} (replay: cargo kani --harness {harness} --concrete-playback=print)"),
            1,
        )
    } else if verdict_line("SUCCESSFUL") {
        (format!("SUCCESSFUL {harness}{}", bound_suffix(bound)), 0)
    } else {
        (format!("ERROR {harness}: {}", first_error(lines)), 2)
    }
}

fn parse_apalache(lines: &[&str], mode: &str, inv: &str, spec: &str, bound: Option<&str>) -> (String, i32) {
    let text = lines.join("\n");
    if mode == "typecheck" {
        if text.contains("Type checker [OK]") || text.contains("Typechecking ... succeeded") {
            return (format!("TYPECHECK-OK {spec}"), 0);
        }
        // Snowcat type errors carry a bracketed source location: [file:L:C-L:C]: msg
        for ln in lines {
            if let (Some(lb), Some(rbm)) = (ln.find('['), ln.find("]:")) {
                if lb < rbm {
                    let inside = &ln[lb + 1..rbm]; // file:L:C-L:C
                    if let Some((_file, loc)) = inside.split_once(':') {
                        let msg = ln[rbm + 2..].trim();
                        return (format!("TYPE-ERROR {loc}: {msg}"), 2);
                    }
                }
            }
        }
        for ln in lines {
            let low = ln.to_lowercase();
            if low.contains("error") || low.contains("typingexception") {
                return (format!("TYPE-ERROR ?: {}", ln.trim()), 2);
            }
        }
        return (format!("ERROR: {}", first_error(lines)), 2);
    }
    // mode == check. The verdict is the "The outcome is: <X>" line. Collect every outcome and fail
    // closed when there is none or more than one distinct value (a concatenated or stale log), so a
    // stray NoError can never override a real Error.
    let outcomes: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.split("The outcome is:").nth(1))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    if outcomes.is_empty() {
        return (format!("ERROR: {}", first_error(lines)), 2);
    }
    if outcomes.iter().any(|o| *o != outcomes[0]) {
        return (format!("ERROR: ambiguous apalache outcome ({})", outcomes.join(", ")), 2);
    }
    match outcomes[0] {
        "NoError" => (format!("PROVEN {inv}{}", bound_suffix(bound)), 0),
        "Error" => {
            let cex = cex_path(&text).unwrap_or_else(|| "counterexample.tla".to_string());
            (format!("VIOLATED {inv} (counterexample: {cex})"), 1)
        }
        // A deadlock is a real negative result (a state with no enabled action), not a tool failure.
        "Deadlock" => (format!("VIOLATED {inv} (deadlock: no enabled action)"), 1),
        other => (format!("ERROR: apalache outcome {other} not recognized"), 2),
    }
}

/// The counterexample path from Apalache's "Check the trace ... : <path>" line.
fn cex_path(text: &str) -> Option<String> {
    let i = text.find("Check the trace")?;
    let rest = &text[i..];
    let c = rest.find(':')?;
    rest[c + 1..].split_whitespace().next().map(str::to_string)
}

fn parse_tlaps(lines: &[&str], module: &str) -> (String, i32) {
    // Anchor to the toolbox protocol prefix `@!!`, so echoed spec or source content carrying the
    // substring `status:`/`loc:` cannot fabricate or inflate obligations.
    let mut total = 0usize;
    let mut not_proved = 0usize;
    let mut last_loc = "?";
    let mut first_bad: Option<(String, String)> = None;
    for ln in lines {
        let t = ln.trim_start();
        if let Some(loc) = t.strip_prefix("@!!loc:") {
            last_loc = loc.trim();
        }
        if let Some(st) = t.strip_prefix("@!!status:") {
            let st = st.trim();
            total += 1;
            // Allowlist the genuinely-discharged statuses. Anything else (`failed`, `omitted`,
            // `missing`, `interrupted`, or a decorated form such as `failed (smt: timeout)`) is not a
            // proof, so the run is not all-proved. The old exact `== "failed"` denylist let those pass.
            if !matches!(st, "proved" | "trivial") {
                not_proved += 1;
                if first_bad.is_none() {
                    first_bad = Some((last_loc.to_string(), st.to_string()));
                }
            }
        }
    }
    if total == 0 {
        return (format!("ERROR {module}: {}", first_error(lines)), 2);
    }
    if not_proved == 0 {
        // TLAPS is a proof system: unbounded, the top rung of the ladder.
        (format!("ALL-PROVED {module} ({total} obligations) [bound=unbounded]"), 0)
    } else {
        let (loc, st) = first_bad.unwrap();
        (
            format!("FAILED {module}: {not_proved}/{total} not proved (first: {loc} [{st}])"),
            1,
        )
    }
}

/// Combined stdout+stderr of a finished verifier, split into lines (owned).
fn combined_lines(out: Output) -> Vec<String> {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push('\n');
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s.lines().map(str::to_string).collect()
}

/// Run a command; returns (combined output lines, exited-cleanly). `exited-cleanly` is true only on
/// a normal exit with status 0 (a signal-kill or non-zero exit is false). On spawn failure, one
/// `error:` line and `false`, so the parser yields ERROR and the exit-status backstop also fires.
fn run(mut cmd: Command, tool: &str) -> (Vec<String>, bool) {
    match cmd.output() {
        Ok(o) => {
            let clean = o.status.success();
            (combined_lines(o), clean)
        }
        Err(e) => (vec![format!("error: {tool} could not run: {e}")], false),
    }
}

fn run_kani(harness: &str, dir: &str, bound: Option<&str>) -> (Vec<String>, bool) {
    let mut c = Command::new("cargo");
    c.args(["kani", "--harness", harness, "--output-format", "terse"]);
    if let Some(n) = bound.and_then(|b| b.strip_prefix("unwind=")) {
        c.args(["--default-unwind", n]);
    }
    c.current_dir(dir);
    run(c, "cargo kani")
}

fn run_apalache(mode: &str, spec: &str, inv: &str, bound: Option<&str>) -> (Vec<String>, bool) {
    let mut c = Command::new("apalache-mc");
    if mode == "typecheck" {
        c.args(["typecheck", spec]);
    } else {
        c.arg("check").arg(format!("--inv={inv}"));
        if let Some(n) = bound.and_then(|b| b.strip_prefix("length=")) {
            c.arg(format!("--length={n}"));
        }
        c.arg(spec);
    }
    run(c, "apalache-mc")
}

fn run_tlaps(module_path: &str) -> (Vec<String>, bool) {
    let mut c = Command::new("tlapm");
    c.args(["--toolbox", "0", "0", module_path]);
    run(c, "tlapm")
}

fn usage() -> ! {
    eprintln!(
        "usage: host-prove <kani --harness NAME [--dir D] | apalache --mode typecheck|check --spec F [--inv N] | tlaps --module F.tla> [--bound B] [--stdin]\n       runs the verifier and prints one verdict line; --stdin parses already-captured output instead"
    );
    process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // A flag's value is the next token, unless that token is itself a flag (then the value is
    // missing) — so `--harness --dir D` does not silently make the harness name `--dir`.
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .filter(|v| !v.starts_with("--"))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);

    let from_stdin = has("--stdin");
    let stdin_lines = || -> Vec<String> {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).ok();
        input.lines().map(str::to_string).collect()
    };
    let bound = get("--bound");
    let b = bound.as_deref();
    let sub = args.get(1).map(String::as_str);

    // Validate --bound against the verifier, so a malformed or misplaced bound is rejected rather
    // than dropped at the tool yet recorded verbatim on the PASS (which would over-claim coverage).
    if let Some(bv) = b {
        let ok = match sub {
            Some("kani") => bv.starts_with("unwind="),
            Some("apalache") => get("--mode").as_deref() == Some("check") && bv.starts_with("length="),
            _ => false,
        };
        if !ok {
            eprintln!("host-prove: --bound '{bv}' is not valid here (expected unwind=<K> for kani, length=<N> for apalache check)");
            usage();
        }
    }

    let (owned, live_clean): (Vec<String>, Option<bool>) = match sub {
        Some("kani") => {
            let h = get("--harness").unwrap_or_else(|| usage());
            if from_stdin {
                (stdin_lines(), None)
            } else {
                let (l, ok) = run_kani(&h, &get("--dir").unwrap_or_else(|| ".".into()), b);
                (l, Some(ok))
            }
        }
        Some("apalache") => {
            let mode = get("--mode").unwrap_or_else(|| usage());
            if mode != "typecheck" && mode != "check" {
                eprintln!("host-prove: --mode '{mode}' is not typecheck or check");
                usage();
            }
            let spec = get("--spec").unwrap_or_else(|| "spec".into());
            let inv = get("--inv").unwrap_or_else(|| "invariant".into());
            if from_stdin {
                (stdin_lines(), None)
            } else {
                let (l, ok) = run_apalache(&mode, &spec, &inv, b);
                (l, Some(ok))
            }
        }
        Some("tlaps") => {
            let m = get("--module").unwrap_or_else(|| usage());
            if from_stdin {
                (stdin_lines(), None)
            } else {
                let (l, ok) = run_tlaps(&m);
                (l, Some(ok))
            }
        }
        _ => usage(),
    };
    let lines: Vec<&str> = owned.iter().map(String::as_str).collect();

    let (mut line, mut code) = match sub {
        Some("kani") => parse_kani(&lines, &get("--harness").unwrap(), b),
        Some("apalache") => {
            let spec = get("--spec").unwrap_or_else(|| "spec".into());
            let base = spec.rsplit(['/', '\\']).next().unwrap_or(&spec).to_string();
            parse_apalache(&lines, &get("--mode").unwrap(), &get("--inv").unwrap_or_else(|| "invariant".into()), &base, b)
        }
        Some("tlaps") => parse_tlaps(&lines, module_name(&get("--module").unwrap())),
        _ => usage(),
    };

    // Fail-closed exit-status backstop: a live verifier that exited abnormally (a non-zero exit or a
    // signal-kill) must never yield a PASS, even if its partial or stale output parsed as one.
    if live_clean == Some(false) && code == 0 {
        line = "ERROR: the verifier exited abnormally; its output parsed as a pass and is not trusted".to_string();
        code = 2;
    }

    println!("{line}");
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ls(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn kani_success_carries_bound() {
        let (v, c) = parse_kani(&ls(include_str!("../tests/fixtures/kani_success.txt")), "verify_is_dotted_code", Some("unwind=20"));
        assert_eq!(c, 0);
        assert!(v.starts_with("SUCCESSFUL verify_is_dotted_code"), "{v}");
        assert!(v.contains("[bound=unwind=20]"), "{v}");
    }
    #[test]
    fn kani_success_unspecified_bound() {
        let (v, c) = parse_kani(&ls(include_str!("../tests/fixtures/kani_success.txt")), "verify_is_dotted_code", None);
        assert_eq!(c, 0);
        assert!(v.contains("[bound=unspecified]"), "{v}");
    }
    #[test]
    fn kani_failed() {
        let (v, c) = parse_kani(&ls(include_str!("../tests/fixtures/kani_failed.txt")), "verify_seg_glob", None);
        assert_eq!(c, 1);
        assert!(v.starts_with("FAILED verify_seg_glob (replay:"), "{v}");
        // invariant NonPassHasNoBound: a non-PASS verdict carries no bound footing.
        assert!(!v.contains("[bound"), "{v}");
    }
    #[test]
    fn kani_error() {
        let (v, c) = parse_kani(&ls(include_str!("../tests/fixtures/kani_error.txt")), "verify_x", None);
        assert_eq!(c, 2);
        assert!(v.starts_with("ERROR verify_x:"), "{v}");
    }
    #[test]
    fn apalache_typecheck_ok() {
        let (v, c) = parse_apalache(&ls(include_str!("../tests/fixtures/apalache_typecheck_ok.txt")), "typecheck", "x", "ParallelScan.tla", None);
        assert_eq!(c, 0);
        assert_eq!(v, "TYPECHECK-OK ParallelScan.tla");
    }
    #[test]
    fn apalache_typecheck_err_location() {
        let (v, c) = parse_apalache(&ls(include_str!("../tests/fixtures/apalache_typecheck_err.txt")), "typecheck", "x", "ParallelScan.tla", None);
        assert_eq!(c, 2);
        assert!(v.starts_with("TYPE-ERROR 42:10-42:24:"), "{v}");
    }
    #[test]
    fn apalache_check_proven_carries_bound() {
        let (v, c) = parse_apalache(&ls(include_str!("../tests/fixtures/apalache_check_noerror.txt")), "check", "ScanEquiv", "s", Some("length=12"));
        assert_eq!(c, 0);
        assert!(v.starts_with("PROVEN ScanEquiv"), "{v}");
        assert!(v.contains("[bound=length=12]"), "{v}");
        // invariant BoundedToolsNeverUnbounded: a non-TLAPS PASS never claims unbounded soundness.
        assert!(!v.contains("unbounded"), "{v}");
    }
    #[test]
    fn apalache_check_violated() {
        let (v, c) = parse_apalache(&ls(include_str!("../tests/fixtures/apalache_check_error.txt")), "check", "ScanEquiv", "s", None);
        assert_eq!(c, 1);
        assert!(v.starts_with("VIOLATED ScanEquiv (counterexample:"), "{v}");
    }
    #[test]
    fn tlaps_allproved_unbounded() {
        let (v, c) = parse_tlaps(&ls(include_str!("../tests/fixtures/tlaps_allproved.txt")), "ParallelScan");
        assert_eq!(c, 0);
        assert_eq!(v, "ALL-PROVED ParallelScan (2 obligations) [bound=unbounded]");
    }
    #[test]
    fn tlaps_failed_first_loc() {
        let (v, c) = parse_tlaps(&ls(include_str!("../tests/fixtures/tlaps_failed.txt")), "ParallelScan");
        assert_eq!(c, 1);
        assert_eq!(v, "FAILED ParallelScan: 1/2 not proved (first: 21:1:21:30 [failed])");
    }
    #[test]
    fn module_name_from_path() {
        assert_eq!(module_name("spec/ParallelScan.tla"), "ParallelScan");
        assert_eq!(module_name("ParallelScan"), "ParallelScan");
    }

    #[test]
    fn kani_failed_dominates_a_stray_successful() {
        // A FAILED line, or any non-zero failure count, must win over a SUCCESSFUL line.
        let out = "VERIFICATION:- FAILED\nVERIFICATION:- SUCCESSFUL\n\
                   Complete - 1 successfully verified harnesses, 1 failures, 2 total.";
        let (v, c) = parse_kani(&ls(out), "h", None);
        assert_eq!(c, 1, "{v}");
        assert!(v.starts_with("FAILED h"), "{v}");
    }

    #[test]
    fn tlaps_omitted_or_decorated_is_not_all_proved() {
        // An omitted leaf, and a decorated `failed (...)`, must each block ALL-PROVED.
        let omitted = "@!!loc:1:1:1:5\n@!!status:proved\n@!!loc:2:1:2:5\n@!!status:omitted";
        let (v, c) = parse_tlaps(&ls(omitted), "M");
        assert_eq!(c, 1, "{v}");
        assert!(v.contains("[omitted]"), "{v}");
        let decorated = "@!!loc:1:1:1:5\n@!!status:proved\n@!!loc:2:1:2:5\n@!!status:failed (smt: timeout)";
        let (v2, c2) = parse_tlaps(&ls(decorated), "M");
        assert_eq!(c2, 1, "{v2}");
    }

    #[test]
    fn tlaps_status_substring_does_not_fabricate_an_obligation() {
        // `status:` echoed in non-protocol content must not count as an obligation.
        let echoed = "@!!BEGINMSG\n@!!type:obligation\nASSUME msg.status: proved\n@!!ENDMSG";
        let (v, c) = parse_tlaps(&ls(echoed), "M");
        assert_eq!(c, 2, "{v}");
        assert!(v.starts_with("ERROR M:"), "{v}");
    }

    #[test]
    fn tlaps_zero_status_is_error() {
        let (v, c) = parse_tlaps(&ls("just text\nno protocol lines"), "M");
        assert_eq!(c, 2, "{v}");
        assert!(v.starts_with("ERROR M:"), "{v}");
    }

    #[test]
    fn apalache_ambiguous_outcome_is_error() {
        let mixed = "The outcome is: Error\nprev run said The outcome is: NoError";
        let (v, c) = parse_apalache(&ls(mixed), "check", "Inv", "s", None);
        assert_eq!(c, 2, "{v}");
        assert!(v.contains("ambiguous"), "{v}");
    }

    #[test]
    fn apalache_deadlock_is_a_negative() {
        let dl = "The outcome is: Deadlock\nEXITCODE: ERROR (12)";
        let (v, c) = parse_apalache(&ls(dl), "check", "Inv", "s", None);
        assert_eq!(c, 1, "{v}");
        assert!(v.contains("deadlock"), "{v}");
    }

    #[test]
    fn apalache_check_no_outcome_is_error() {
        let (v, c) = parse_apalache(&ls("PASS #1: SanyParser\nnoise"), "check", "Inv", "s", None);
        assert_eq!(c, 2, "{v}");
        assert!(v.starts_with("ERROR:"), "{v}");
    }

    #[test]
    fn nonpass_carries_no_bound() {
        // invariant NonPassHasNoBound across all three verifiers (not kani alone).
        let (kf, _) = parse_kani(&ls(include_str!("../tests/fixtures/kani_failed.txt")), "h", None);
        let (av, _) = parse_apalache(&ls(include_str!("../tests/fixtures/apalache_check_error.txt")), "check", "Inv", "s", None);
        let (tf, _) = parse_tlaps(&ls(include_str!("../tests/fixtures/tlaps_failed.txt")), "M");
        for v in [kf, av, tf] {
            assert!(!v.contains("[bound"), "{v}");
        }
    }
}
