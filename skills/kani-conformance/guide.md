# Kani Code-Conformance Workflow

Prove that a Rust function meets its specified contract for **every** bounded input —
the code-side rung of the verification ladder (verify the *implementation*, beyond
tests + trace conformance). You write a small proof harness, run it through one
wrapper command, and act on a single verdict word.

This workflow is built to run with a **small/weak model**. You never read raw Kani
output and you never invent a proof: you fill a template, run `kani_check.sh`, and
match its one-line verdict in the table below. If a step is not on the path, STOP.

## Input / Output

- **Input:** a Rust crate, a target function, and the property to prove (usually from
  an obligation dispositioned `kani:<harness>`).
- **Output:** a `#[cfg(kani)]` proof harness that verifies SUCCESSFUL, plus the wired
  obligation disposition and CI lane — OR a recorded counterexample reported to the user.

## When this lane applies (and when it does NOT)

Use Kani only for a **pure-ish, bounded** property of code: a function's input→output
contract, an equivalence between two implementations, panic/overflow freedom, a parser
that must reject a class of inputs. If the property is about **behaviour/requirements**,
it belongs to allium; if about **ordering/timing**, to specula/TLC; if it needs *all
parameter values of a spec*, to apalache-symbolic. A property that needs the network,
the filesystem, threads, or unbounded loops is **out of scope** — say so and stop.

## The only verdict vocabulary

`kani_check.sh` prints exactly one line. Match on the first word:

| Verdict | Meaning | Exit |
|---|---|---|
| `SUCCESSFUL <harness>` | Proved for all inputs in the harness's bounds | 0 |
| `FAILED <harness> (replay: …)` | A counterexample exists | 1 |
| `ERROR <harness>: <msg>` | Kani could not run (build error, unwind bound, unsupported) | 2 |

## Procedure

### Step 1 — State the property in one sentence

Write the contract as one checkable sentence: *"for every input `x` satisfying P,
`f(x)` returns/satisfies Q."* If you cannot, STOP and ask the user. Do not guess.

### Step 2 — Fill the harness template

Add a `#[cfg(kani)]` module to the crate (a sibling of the function, or `src/proofs.rs`
behind `#[cfg(kani)] mod proofs;`). **`#[cfg(kani)]` is mandatory** — it keeps the
harness out of `cargo build`/`cargo test`, so the release artifact stays byte-identical
(reproducible build untouched). Fill exactly this shape — change only the marked parts:

```rust
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn <harness_name> {              // <-- the name you disposition as kani:<harness_name>
        let x: <Type> = kani::any(); // a symbolic input — Kani tries ALL values
        kani::assume(<precondition>);// restrict to the inputs the contract covers (P)
        assert!(<property of x>);    // the postcondition Q (or `f(x) == reference(x)`)
    }
}
```

Rules that keep a weak model on the rails:
- **Symbolic, not example.** Use `kani::any()` for inputs; do not hardcode one value
  (that is just a test). One symbolic input proves the whole class.
- **Bound the input** with `kani::assume(...)` to exactly the contract's domain — e.g.
  `kani::assume(s.len() <= 8)` for strings/slices (Kani needs a finite bound; pick the
  smallest bound that still covers the real cases).
- **One property per harness.** Split unrelated claims into separate harnesses.
- For `&str`/`String` inputs, build from a bounded `[u8; N]` of `kani::any()` bytes
  (e.g. with `kani::assume(bytes.iter().all(|b| b.is_ascii()))`), not an unbounded `String`.

### Step 3 — Run the wrapper (one command)

```sh
host-prove/scripts/kani_check.sh <harness_name> <crate-dir>
```

### Step 4 — Act on the verdict (do exactly this)

| Verdict | Do |
|---|---|
| `SUCCESSFUL` | Go to Step 5 (wire it). Done — the property holds. |
| `FAILED (replay: <cmd>)` | The code (or your contract) is wrong. Run `<cmd>` to print a replay unit test; add it to the crate's tests as a regression; report the counterexample to the user. **STOP. Do NOT weaken `assert!` or loosen `assume` to force a pass** — that hides the bug. |
| `ERROR: …unwind…` | Loop bound too low — see the fix table. Re-run Step 3. |
| `ERROR: …` (other) | Fix the build error the message names, or the property is out of scope (Step "When this lane applies"). If unsure, STOP and report. |

### Step 5 — Wire the obligation + CI lane

1. In the spec's `<spec>.obligations` manifest, disposition the obligation as
   `<id> => kani:<harness_name>` (replaces a `test:` line when a proof now discharges it).
2. Ensure the crate's CI runs Kani so the lane is live (see `references` / install). A
   declared `kani:` disposition with **no** Kani CI lane is a HAZARD under
   `host-lifecycle software --check`. Run `host-lifecycle obligations <spec> --kani <dir>`
   to confirm the harness name resolves.

## Fix recipes

| Symptom in `ERROR` | Fix |
|---|---|
| `Failed to unwind ... bound` / `unwinding assertion` | Add `#[kani::unwind(N)]` above the harness (start N = your `assume` length bound + 2), or pass a global `--default-unwind N`. Raise N until it clears; keep it as small as works. |
| `cannot find function/value` (build) | A normal Rust build error in the harness — fix the name/import. The harness sees the crate via `use super::*;`. |
| `unsupported construct` | The function uses something Kani cannot model (FFI, inline asm, certain intrinsics). This property is out of scope — disposition `waived: <reason>` and tell the user. |

## Hard rules

- The harness is `#[cfg(kani)]`-gated. No exceptions — an ungated harness pulls the
  `kani` crate into the normal build and breaks the reproducible artifact.
- A `FAILED` verdict is never "fixed" by editing the assertion. Fix the code or report.
- One command per run (`kani_check.sh`); never hand-parse `cargo kani` output.
