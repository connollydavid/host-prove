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

/// The text after the first occurrence of `key` on a line, trimmed. The TLAPS toolbox emits
/// one field per line (`@!!loc:18:1:18:42`, `@!!status:proved`), so rest-of-line is the value.
fn field_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.find(key).map(|i| line[i + key.len()..].trim())
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
    let verdict_line = |needle: &str| {
        lines
            .iter()
            .any(|l| l.contains("VERIFICATION") && l.contains(needle))
    };
    if verdict_line("SUCCESSFUL") {
        (format!("SUCCESSFUL {harness}{}", bound_suffix(bound)), 0)
    } else if verdict_line("FAILED") {
        (
            format!("FAILED {harness} (replay: cargo kani --harness {harness} --concrete-playback=print)"),
            1,
        )
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
    // mode == check
    if text.contains("The outcome is: NoError") {
        return (format!("PROVEN {inv}{}", bound_suffix(bound)), 0);
    }
    if text.contains("The outcome is: Error") {
        let cex = cex_path(&text).unwrap_or_else(|| "counterexample.tla".to_string());
        return (format!("VIOLATED {inv} (counterexample: {cex})"), 1);
    }
    (format!("ERROR: {}", first_error(lines)), 2)
}

/// The counterexample path from Apalache's "Check the trace ... : <path>" line.
fn cex_path(text: &str) -> Option<String> {
    let i = text.find("Check the trace")?;
    let rest = &text[i..];
    let c = rest.find(':')?;
    rest[c + 1..].split_whitespace().next().map(str::to_string)
}

fn parse_tlaps(lines: &[&str], module: &str) -> (String, i32) {
    let mut statuses: Vec<&str> = Vec::new();
    let mut last_loc = "?";
    let mut first_fail_loc: Option<&str> = None;
    for ln in lines {
        if let Some(loc) = field_after(ln, "loc:") {
            last_loc = loc;
        }
        if let Some(st) = field_after(ln, "status:") {
            statuses.push(st);
            if st == "failed" && first_fail_loc.is_none() {
                first_fail_loc = Some(last_loc);
            }
        }
    }
    let total = statuses.len();
    if total == 0 {
        return (format!("ERROR {module}: {}", first_error(lines)), 2);
    }
    let failed = statuses.iter().filter(|s| **s == "failed").count();
    if failed == 0 {
        // TLAPS is a proof system — unbounded, the top rung of the ladder.
        (format!("ALL-PROVED {module} ({total} obligations) [unbounded]"), 0)
    } else {
        (
            format!("FAILED {module}: {failed}/{total} (first: {})", first_fail_loc.unwrap_or("?")),
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

/// Run a command; on spawn failure return one `error:` line so the parser yields an exit-2 ERROR.
fn run(mut cmd: Command, tool: &str) -> Vec<String> {
    match cmd.output() {
        Ok(o) => combined_lines(o),
        Err(e) => vec![format!("error: {tool} could not run: {e}")],
    }
}

fn run_kani(harness: &str, dir: &str, bound: Option<&str>) -> Vec<String> {
    let mut c = Command::new("cargo");
    c.args(["kani", "--harness", harness, "--output-format", "terse"]);
    if let Some(n) = bound.and_then(|b| b.strip_prefix("unwind=")) {
        c.args(["--default-unwind", n]);
    }
    c.current_dir(dir);
    run(c, "cargo kani")
}

fn run_apalache(mode: &str, spec: &str, inv: &str, bound: Option<&str>) -> Vec<String> {
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

fn run_tlaps(module_path: &str) -> Vec<String> {
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
    let get = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
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

    let owned: Vec<String> = match args.get(1).map(String::as_str) {
        Some("kani") => {
            let h = get("--harness").unwrap_or_else(|| usage());
            if from_stdin { stdin_lines() } else { run_kani(&h, &get("--dir").unwrap_or_else(|| ".".into()), b) }
        }
        Some("apalache") => {
            let mode = get("--mode").unwrap_or_else(|| usage());
            let spec = get("--spec").unwrap_or_else(|| "spec".into());
            let inv = get("--inv").unwrap_or_else(|| "invariant".into());
            if from_stdin { stdin_lines() } else { run_apalache(&mode, &spec, &inv, b) }
        }
        Some("tlaps") => {
            let m = get("--module").unwrap_or_else(|| usage());
            if from_stdin { stdin_lines() } else { run_tlaps(&m) }
        }
        _ => usage(),
    };
    let lines: Vec<&str> = owned.iter().map(String::as_str).collect();

    let (line, code) = match args.get(1).map(String::as_str) {
        Some("kani") => parse_kani(&lines, &get("--harness").unwrap(), b),
        Some("apalache") => {
            let spec = get("--spec").unwrap_or_else(|| "spec".into());
            let base = spec.rsplit(['/', '\\']).next().unwrap_or(&spec).to_string();
            parse_apalache(&lines, &get("--mode").unwrap(), &get("--inv").unwrap_or_else(|| "invariant".into()), &base, b)
        }
        Some("tlaps") => parse_tlaps(&lines, module_name(&get("--module").unwrap())),
        _ => usage(),
    };
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
        assert_eq!(v, "ALL-PROVED ParallelScan (2 obligations) [unbounded]");
    }
    #[test]
    fn tlaps_failed_first_loc() {
        let (v, c) = parse_tlaps(&ls(include_str!("../tests/fixtures/tlaps_failed.txt")), "ParallelScan");
        assert_eq!(c, 1);
        assert_eq!(v, "FAILED ParallelScan: 1/2 (first: 21:1:21:30)");
    }
    #[test]
    fn module_name_from_path() {
        assert_eq!(module_name("spec/ParallelScan.tla"), "ParallelScan");
        assert_eq!(module_name("ParallelScan"), "ParallelScan");
    }
}
