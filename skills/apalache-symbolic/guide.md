# Apalache Symbolic-Checking Workflow

Check a TLA+ invariant **symbolically** with Apalache (it compiles the spec to SMT and
reasons over whole data domains with Z3), rather than enumerating finite states like TLC.
This is the solver rung of the ladder: it discharges *parametric* invariants — properties
quantified over all N — that bounded TLC can only sample.

Built to run with a **small/weak model**: you never read raw Apalache output and never
redesign the spec. You annotate types mechanically, run one wrapper command per step, and
match its one-line verdict. Type-check first — Apalache refuses a spec it cannot type.

## Input / Output

- **Input:** a `.tla` spec, the invariant name to check (usually from an obligation
  dispositioned `apalache:<inv>`), and the `Init`/`Next` names (default `Init`/`Next`).
- **Output:** a typed spec that Apalache proves `PROVEN` for the invariant up to the
  configured length, plus the wired obligation + CI lane — OR a classified counterexample.

## When this lane applies

The invariant must quantify over a **parameter** — `\A i \in Server`, `\A t \in Thread`,
all queue lengths, all configs — where you want it to hold for *every* value, not just the
`CONSTANTS` TLC was pinned to. If a fixed small instance is all you need, plain specula/TLC
is enough; do not reach for Apalache. Liveness/temporal properties and unbounded message
buffers may exceed Apalache — if `--length` cannot be made meaningful, say so and stop.

## The only verdict vocabulary

`host-prove apalache` prints exactly one line. Match on the first word:

| Verdict | Meaning | Exit |
|---|---|---|
| `TYPECHECK-OK <spec>` | Snowcat types check; ready to `check` | 0 |
| `TYPE-ERROR <loc>: <msg>` | A `@type:` annotation is missing/wrong at `<loc>` | 2 |
| `PROVEN <inv> [bound=…] [apalache=<v> pinned]` | No violation up to `--length`, by the pinned apalache — symbolic, so holds for all parameter values | 0 |
| `VIOLATED <inv> (counterexample: <file>)` | Apalache found a violating run (or a deadlock) | 1 |
| `ERROR: <msg>` | Apalache ran but produced no verdict | 2 |
| `BLOCKED apalache: <reason>` | Apalache is absent or not the pinned version: run `host-prove install apalache` | 2 |

## Procedure

### Step 1 — Type-check (gate)

```sh
host-prove apalache --mode typecheck --spec <spec.tla>
```

| Verdict | Do |
|---|---|
| `TYPECHECK-OK` | Go to Step 2. |
| `TYPE-ERROR <loc>: …` | Add/fix the `@type:` annotation at `<loc>` using the recipe table, then re-run Step 1. Loop until OK. |
| `ERROR: …` | Fix what the message names (parse error, missing file). If unclear, STOP and report. |

Apalache needs a **Snowcat type annotation** on every `CONSTANT`, `VARIABLE`, and operator
it cannot infer. Annotations are TLA+ comments of the form `\* @type: <T>;` placed on the
line **above** the declaration. This is mechanical — do not change the spec's logic.

### Step 2 — Symbolic check

```sh
host-prove apalache --mode check --spec <spec.tla> --inv <Inv> --bound length=<N>
```

Pick `<N>` = the longest run that could first violate the invariant (often the spec's
diameter; start at 10 and raise if the property is deep). `--cinit` sets symbolic bounds on
`CONSTANTS` (e.g. `N \in 1..MAX`) so the proof covers the parametric family, not one size.

### Step 3 — Act on the verdict

| Verdict | Do |
|---|---|
| `PROVEN` | The invariant holds symbolically up to `--length`. If the verdict shows `[bound=unspecified]`, supply `--bound length=<N>` and re-run, or record it as flagged. Then go to Step 4 (wire it). |
| `VIOLATED (counterexample: <file>)` | Classify it (table below). A `deadlock` counterexample is a real negative too. |
| `BLOCKED apalache: <reason>` | Apalache is not installed at the pinned version. Run `host-prove install apalache`, then re-run Step 2. Never a pass or fail; do not touch the spec. |
| `ERROR: …` | Fix what the message names, or the property is out of scope. If unsure, STOP. |

**Counterexample classification** (the specula convention — every counterexample is exactly one):

| Case | Meaning | Who is wrong | Action |
|---|---|---|---|
| A | Invariant too strong | the invariant | Weaken/correct the invariant; re-run. |
| B | Spec models the code wrong | the spec | Fix the spec to match the implementation; re-run. |
| C | Real bug | the implementation | **STOP. Report to the user immediately.** Do not edit the spec to hide it. |

Read the counterexample file to decide; cross-reference the implementation as ground truth.
If you cannot confidently place it in A or B, treat it as C and report.

### Step 4 — Wire the obligation + CI lane

1. Disposition the obligation `<id> => apalache:<Inv>` in the `<spec>.obligations` manifest.
2. Ensure CI runs `apalache-mc` on the spec (a declared `apalache:` disposition with no
   Apalache lane is a HAZARD under `host-lifecycle software --check`).

## Fix recipes — `@type:` annotations (the common friction)

Place each on the line **above** the declaration. Common shapes:

| TLA+ thing | Annotation |
|---|---|
| `CONSTANT N` (a number) | `\* @type: Int;` |
| `CONSTANT Server` (a set) | `\* @type: Set(SERVER);` (an uninterpreted sort) |
| `VARIABLE log` (function/seq) | `\* @type: Int -> Seq(Int);` |
| `VARIABLE state` (record) | `\* @type: Str -> { term: Int, role: Str };` |
| operator `Foo(x) == …` | `\* @type: (Int) => Bool;` (arg types `=>` result) |
| a set of records | `\* @type: Set({ id: Int, ok: Bool });` |

If `TYPE-ERROR` persists after annotating, the spec mixes types in one expression (e.g. a
function used at two types) — report that to the user; do not contort the logic to satisfy
the checker.

## Hard rules

- Type-check passes before you `check`. Never skip Step 1.
- `--cinit` with symbolic constant bounds is what makes the result *parametric*; a check
  with `CONSTANTS` pinned to one value is just slower TLC — note it if you must do that.
- A Case-C counterexample stops the workflow and goes to the user. Never edit the spec to
  make a real bug disappear.
- One wrapper command per step; never hand-parse `apalache-mc` output.
