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
//! Run a verifier (pure-local and pin-bound; the verifier must already be installed):
//!   host-prove kani     --harness NAME [--dir CRATE] [--bound unwind=K]
//!   host-prove apalache --mode typecheck --spec FILE
//!   host-prove apalache --mode check     --spec FILE --inv NAME [--bound length=N]
//!   host-prove tlaps    --module FILE.tla
//! Install the pinned, SHA-verified verifier (the one network/filesystem verb):
//!   host-prove install <kani|apalache|tlaps>
//! Report each verifier's installed-vs-pinned status:  host-prove doctor
//! Parse already-captured output instead of running (testing / piping): add `--stdin`.
//!
//! The run path never installs and binds the verifier to the embedded `tools.lock` pin: an absent or
//! wrong-version verifier is BLOCKED (exit 2), never a pass or fail (call/0036). A live PASS carries
//! the resolved `[<tool>=<version> pinned]`.
//!
//! Vocabulary / exit (0 = proved/clean, 1 = a real negative verdict, 2 = could not run / blocked):
//!   kani      SUCCESSFUL <h> [bound=..] | FAILED <h> (replay: ..) | ERROR <h>: <msg>
//!   apalache  TYPECHECK-OK <spec> | TYPE-ERROR <loc>: <msg>
//!             PROVEN <inv> [bound=..] | VIOLATED <inv> (counterexample: ..) | ERROR: <msg>
//!   tlaps     ALL-PROVED <module> (<n> obligations) [bound=unbounded]
//!             FAILED <module>: <k>/<n> not proved (first: <loc> [<status>]) | ERROR <module>: <msg>
//!   any       BLOCKED <tool>: <reason>; run: host-prove install <tool>   (verifier not pin-bound)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};

// === Verifier provenance: install verb + pure-local pin-bound resolver (call/0036) ===
//
// host-prove carries its verifier pins (the analog of .host-software's recorded toolchain). The run
// path is pure-local and NEVER installs: it resolves a verifier only from a version-stamped install
// bound to the embedded pin, and fails closed (a BLOCKED verdict) otherwise. `host-prove install` is
// the one verb that touches the network or filesystem. No verdict ever issues from an unbound tool.
const TOOLS_LOCK: &str = include_str!("../tools.lock");

/// The value of `<field>=` for `<tool>` in the embedded tools.lock, or None.
fn pin(tool: &str, field: &str) -> Option<String> {
    for line in TOOLS_LOCK.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        if it.next() != Some(tool) {
            continue;
        }
        for tok in it {
            if let Some((k, v)) = tok.split_once('=') {
                if k == field {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Where version-stamped verifier installs live (override with HOST_PROVE_TOOLS).
fn tools_root() -> PathBuf {
    if let Some(d) = std::env::var_os("HOST_PROVE_TOOLS") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/host-prove/tools")
}

/// `~/.cargo/bin`, where `cargo install` places the kani binaries.
fn cargo_bin() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bin")
}

/// Verify `file` hashes to `want` (a 64-hex sha256), shelling sha256sum. Fail-closed: a pin that is
/// not a full hex digest, or a mismatch, is an error and nothing is installed.
fn verify_sha(file: &Path, want: &str) -> Result<(), String> {
    if want.len() != 64 || !want.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("no valid pinned sha256 (got '{want}') — refusing to install"));
    }
    let out = Command::new("sha256sum").arg(file).output().map_err(|e| format!("sha256sum: {e}"))?;
    let got = String::from_utf8_lossy(&out.stdout);
    let got = got.split_whitespace().next().unwrap_or("");
    if got.eq_ignore_ascii_case(want) {
        Ok(())
    } else {
        Err(format!("sha256 mismatch: got {got}, want {want}"))
    }
}

/// The sha256 (lowercase hex) of a file, shelling sha256sum. Used to bind the extracted verifier
/// binary to its install: recorded at install, re-checked on every run, so a binary swapped in place
/// after install cannot answer for the pin (a per-run content bind, not an install-time stamp).
fn file_sha(path: &Path) -> Result<String, String> {
    let out = Command::new("sha256sum").arg(path).output().map_err(|e| format!("sha256sum: {e}"))?;
    if !out.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()));
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| "empty sha256sum output".to_string())
}

/// Run a helper command, mapping a non-zero exit or a spawn failure to an Err with its first stderr
/// line, so any failed install step aborts the install (fail-closed).
fn sh_ok(cmd: &mut Command, what: &str) -> Result<(), String> {
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("").trim()
        )),
        Err(e) => Err(format!("{what}: {e}")),
    }
}

/// `host-prove install <tool>` — the only network/filesystem verb. Fetch the pinned asset, verify its
/// SHA256 BEFORE extracting or executing it, install into a version-stamped directory, and record the
/// verified pin in a marker the resolver checks every run. Returns the install's bin directory. Kani
/// is a cargo-locked source build (no asset sha); its `cargo kani setup` backend is not sha-pinned.
fn install(tool: &str) -> Result<PathBuf, String> {
    match tool {
        "kani" => {
            let ver = pin("kani", "version").ok_or("no kani version pinned")?;
            eprintln!("host-prove: installing kani-verifier {ver} (cargo, locked) ...");
            sh_ok(
                Command::new("cargo").args(["install", "--locked", "kani-verifier", "--version", &ver]),
                "cargo install kani-verifier",
            )?;
            sh_ok(Command::new("cargo").args(["kani", "setup"]), "cargo kani setup")?;
            Ok(cargo_bin())
        }
        "apalache" | "tlaps" => {
            let ver = pin(tool, "version").ok_or("no version pinned")?;
            let asset = pin(tool, "asset").ok_or("no asset pinned")?;
            let sha = pin(tool, "sha256").ok_or("no sha256 pinned")?;
            let repo = if tool == "apalache" { "apalache-mc/apalache" } else { "tlaplus/tlapm" };
            let url = format!("https://github.com/{repo}/releases/download/v{ver}/{asset}");
            let dir = tools_root().join(tool).join(&ver);
            std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
            let tmp = dir.join(".download");
            eprintln!("host-prove: fetching {tool} {ver} ...");
            sh_ok(Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&tmp).arg(&url), "curl")?;
            if let Err(e) = verify_sha(&tmp, &sha) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            if tool == "apalache" {
                sh_ok(Command::new("tar").arg("-xzf").arg(&tmp).arg("-C").arg(&dir).arg("--strip-components=1"), "tar")?;
            } else {
                if !cfg!(target_os = "linux") {
                    let _ = std::fs::remove_file(&tmp);
                    return Err("tlaps installs on Linux only".into());
                }
                sh_ok(Command::new("chmod").arg("+x").arg(&tmp), "chmod")?;
                sh_ok(Command::new(&tmp).arg("-d").arg(&dir), "tlaps installer")?;
            }
            let _ = std::fs::remove_file(&tmp);
            // Record the sha of the EXTRACTED binary host-prove will execute, not the asset's — so the
            // per-run resolve re-hashes the bytes that actually run (the asset sha was verified above,
            // before extraction; this binds the run).
            let exe = if tool == "apalache" { "apalache-mc" } else { "tlapm" };
            let binsha = file_sha(&dir.join("bin").join(exe))?;
            std::fs::write(dir.join(".host-prove-pin"), &binsha).map_err(|e| format!("marker: {e}"))?;
            Ok(dir.join("bin"))
        }
        other => Err(format!("unknown verifier {other}")),
    }
}

/// The installed kani version via `cargo kani --version` (a local subprocess, no network).
fn installed_kani_version() -> Option<String> {
    let out = Command::new("cargo").args(["kani", "--version"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

/// Resolve the pinned verifier — PURE-LOCAL, never installs, never reaches the network. Returns the
/// (program, version) bound to the embedded pin, or Err(reason) for a BLOCKED verdict. Kani is bound
/// by version (sha=n/a, a cargo-locked build); apalache/tlaps by the version-stamped dir + its pin
/// marker. A PATH binary of unverifiable provenance is not consulted: it counts as not-pinned.
fn resolve(tool: &str) -> Result<(String, String), String> {
    match tool {
        "kani" => {
            let pinned = pin("kani", "version").ok_or("no kani version pinned")?;
            match installed_kani_version() {
                None => Err("not installed".into()),
                Some(v) if v == pinned => Ok(("cargo".into(), pinned)),
                Some(v) => Err(format!("pinned {pinned}, found {v}")),
            }
        }
        "apalache" | "tlaps" => {
            let exe = if tool == "apalache" { "apalache-mc" } else { "tlapm" };
            let pinned = pin(tool, "version").ok_or("no version pinned")?;
            let dir = tools_root().join(tool).join(&pinned);
            let bin = dir.join("bin").join(exe);
            if !bin.is_file() {
                return Err(format!("not installed at pinned version {pinned}"));
            }
            // Per-run content bind: the executed binary must still hash to what install recorded.
            let recorded = std::fs::read_to_string(dir.join(".host-prove-pin")).unwrap_or_default();
            let got = file_sha(&bin).map_err(|e| format!("cannot hash the installed binary: {e}"))?;
            if got != recorded.trim() {
                return Err(format!("installed binary does not match its recorded sha at version {pinned}"));
            }
            Ok((bin.to_string_lossy().into_owned(), pinned))
        }
        other => Err(format!("unknown verifier {other}")),
    }
}

/// The BLOCKED verdict line: a verifier absent or not bound to the pin. Exit 2, never a pass/fail.
fn blocked_line(tool: &str, reason: &str) -> String {
    format!("BLOCKED {tool}: {reason}; run: host-prove install {tool}")
}

/// `host-prove doctor` — report each declared verifier's installed-vs-pinned status without proving.
fn doctor() -> i32 {
    let mut all_ready = true;
    for tool in ["kani", "apalache", "tlaps"] {
        match resolve(tool) {
            Ok((_, v)) => {
                // kani's bind is version-only (sha n/a) with an unverified setup backend; say so.
                let note = if tool == "kani" { format!("version {v}, backend unverified") } else { format!("pinned {v}") };
                println!("{tool}: ready ({note})");
            }
            Err(why) => {
                all_ready = false;
                let pinned = pin(tool, "version").unwrap_or_default();
                println!("{tool}: NOT READY (pinned {pinned}): {why}; run: host-prove install {tool}");
            }
        }
    }
    if all_ready { 0 } else { 2 }
}

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

// The verifier argv (pure, so the flag assembly is testable without running the tool). The bound is
// validated against the tool before these run, so a bad prefix is rejected, not silently dropped.
fn kani_args(harness: &str, bound: Option<&str>) -> Vec<String> {
    let mut a = vec![
        "kani".to_string(), "--harness".to_string(), harness.to_string(),
        "--output-format".to_string(), "terse".to_string(),
    ];
    if let Some(n) = bound.and_then(|b| b.strip_prefix("unwind=")) {
        a.push("--default-unwind".to_string());
        a.push(n.to_string());
    }
    a
}

fn apalache_args(mode: &str, spec: &str, inv: &str, bound: Option<&str>) -> Vec<String> {
    if mode == "typecheck" {
        return vec!["typecheck".to_string(), spec.to_string()];
    }
    let mut a = vec!["check".to_string(), format!("--inv={inv}")];
    if let Some(n) = bound.and_then(|b| b.strip_prefix("length=")) {
        a.push(format!("--length={n}"));
    }
    a.push(spec.to_string());
    a
}

fn tlaps_args(module_path: &str) -> Vec<String> {
    vec!["--toolbox".to_string(), "0".to_string(), "0".to_string(), module_path.to_string()]
}

// `prog` is the pin-bound program the resolver returned (the program is never chosen here, and this
// path never installs — the hard split: a run is pure-local).
fn run_kani(prog: &str, harness: &str, dir: &str, bound: Option<&str>) -> (Vec<String>, bool) {
    let mut c = Command::new(prog);
    c.args(kani_args(harness, bound));
    c.current_dir(dir);
    run(c, "cargo kani")
}

fn run_apalache(prog: &str, mode: &str, spec: &str, inv: &str, bound: Option<&str>) -> (Vec<String>, bool) {
    let mut c = Command::new(prog);
    c.args(apalache_args(mode, spec, inv, bound));
    run(c, "apalache-mc")
}

fn run_tlaps(prog: &str, module_path: &str) -> (Vec<String>, bool) {
    let mut c = Command::new(prog);
    c.args(tlaps_args(module_path));
    run(c, "tlapm")
}

fn usage() -> ! {
    eprintln!(
        "usage: host-prove <kani --harness NAME [--dir D] | apalache --mode typecheck|check --spec F [--inv N] | tlaps --module F.tla> [--bound B] [--stdin]\n       host-prove install <kani|apalache|tlaps>   fetch + SHA-verify + install the pinned verifier (the only network step)\n       host-prove doctor                           report each verifier's installed-vs-pinned status\n       a run is pure-local and pin-bound: an absent or wrong-version verifier is BLOCKED (exit 2), never a verdict; --stdin parses already-captured output"
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
    let sub = args.get(1).map(String::as_str);

    // The only network/filesystem verbs. `install` is the sole path that fetches and writes; a CI
    // provisioning lane or the operator runs it deliberately. `doctor` only reads status.
    if sub == Some("install") {
        let tool = args.get(2).map(String::as_str).unwrap_or("");
        if !matches!(tool, "kani" | "apalache" | "tlaps") {
            eprintln!("usage: host-prove install <kani|apalache|tlaps>");
            process::exit(2);
        }
        match install(tool) {
            Ok(bin) => {
                eprintln!("host-prove: {tool} ready ({})", bin.display());
                if let Some(gp) = std::env::var_os("GITHUB_PATH") {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(gp) {
                        let _ = writeln!(f, "{}", bin.display());
                    }
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("host-prove: install {tool} failed: {e}");
                process::exit(2);
            }
        }
    }
    if sub == Some("doctor") {
        process::exit(doctor());
    }

    let from_stdin = has("--stdin");
    let stdin_lines = || -> Vec<String> {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).ok();
        input.lines().map(str::to_string).collect()
    };
    let bound = get("--bound");
    let b = bound.as_deref();

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

    // Pure-local pin-binding: resolve the verifier against the embedded pin before running it. An
    // absent or unbound verifier fails closed to a BLOCKED verdict (exit 2), never a pass or fail,
    // and this path never installs. A --stdin run parses captured output, so it resolves nothing.
    let resolved: Option<(String, String)> = if from_stdin {
        None
    } else {
        match sub {
            Some("kani") | Some("apalache") | Some("tlaps") => {
                let tool = sub.unwrap();
                match resolve(tool) {
                    Ok(rv) => Some(rv),
                    Err(why) => {
                        println!("{}", blocked_line(tool, &why));
                        process::exit(2);
                    }
                }
            }
            _ => None,
        }
    };
    let prog = resolved.as_ref().map(|(p, _)| p.as_str());

    let (owned, live_clean): (Vec<String>, Option<bool>) = match sub {
        Some("kani") => {
            let h = get("--harness").unwrap_or_else(|| usage());
            if from_stdin {
                (stdin_lines(), None)
            } else {
                let (l, ok) = run_kani(prog.unwrap(), &h, &get("--dir").unwrap_or_else(|| ".".into()), b);
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
                let (l, ok) = run_apalache(prog.unwrap(), &mode, &spec, &inv, b);
                (l, Some(ok))
            }
        }
        Some("tlaps") => {
            let m = get("--module").unwrap_or_else(|| usage());
            if from_stdin {
                (stdin_lines(), None)
            } else {
                let (l, ok) = run_tlaps(prog.unwrap(), &m);
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

    // Provenance: a live PASS carries the resolved, pin-bound verifier version, so a glance or a cold
    // read confirms the pinned tool produced the verdict.
    if code == 0 {
        if let (Some(t), Some((_, v))) = (sub, resolved.as_ref()) {
            // kani is bound by version only (cargo-locked, sha n/a) and its setup-fetched backend is
            // not hash-pinned, so its provenance claims strictly less than apalache/tlaps' version+hash bind.
            let bind = if t == "kani" { "version-pinned, backend unverified" } else { "pinned" };
            line = format!("{line} [{t}={v} {bind}]");
        }
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

    #[test]
    fn pin_reads_the_embedded_tools_lock() {
        assert_eq!(pin("apalache", "version").as_deref(), Some("0.58.0"));
        assert!(pin("apalache", "sha256").is_some_and(|s| s.len() == 64));
        assert_eq!(pin("kani", "version").as_deref(), Some("0.67.0"));
        assert_eq!(pin("kani", "sha256").as_deref(), Some("n/a"));
        assert!(pin("nope", "version").is_none());
    }

    #[test]
    fn argv_builders_assemble_the_right_flags() {
        let join = |v: Vec<String>| v.join(" ");
        assert_eq!(join(kani_args("h", Some("unwind=20"))), "kani --harness h --output-format terse --default-unwind 20");
        assert_eq!(join(kani_args("h", None)), "kani --harness h --output-format terse");
        assert_eq!(join(apalache_args("typecheck", "S.tla", "I", None)), "typecheck S.tla");
        assert_eq!(join(apalache_args("check", "S.tla", "Inv", Some("length=12"))), "check --inv=Inv --length=12 S.tla");
        assert_eq!(join(tlaps_args("M.tla")), "--toolbox 0 0 M.tla");
    }

    #[test]
    fn verify_sha_requires_a_full_hex_digest() {
        // not a 64-hex digest -> refused (fail-closed); also covers the kani `n/a` sentinel.
        assert!(verify_sha(Path::new("/etc/hostname"), "n/a").is_err());
        assert!(verify_sha(Path::new("/etc/hostname"), "").is_err());
        assert!(verify_sha(Path::new("/etc/hostname"), "abc").is_err());
    }

    #[test]
    fn resolve_blocks_an_uninstalled_pinned_verifier() {
        // Point the install root at a path with no install: resolve must fail closed (no network).
        std::env::set_var("HOST_PROVE_TOOLS", "/nonexistent-host-prove-tools-9e1");
        assert!(resolve("apalache").is_err());
        assert!(resolve("tlaps").is_err());
        std::env::remove_var("HOST_PROVE_TOOLS");
    }

    #[test]
    fn blocked_line_names_the_install_command() {
        let l = blocked_line("apalache", "not installed at pinned version 0.58.0");
        assert!(l.starts_with("BLOCKED apalache:"), "{l}");
        assert!(l.contains("run: host-prove install apalache"), "{l}");
    }
}
