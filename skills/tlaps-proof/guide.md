# TLAPS Proof Workflow

Machine-check TLA+ proofs with TLAPS (`tlapm`) — the top rung of the ladder: a
**deductive, unbounded** proof that an invariant holds for *all* states and parameters,
which neither bounded TLC nor symbolic Apalache can fully close. Reserve it for the few
genuinely must-hold-for-all properties.

> **Two modes — know which you are in.**
>
> - **MAINTAIN** (this skill, weak model OK): *run* and *re-check* proofs that already
>   exist, and report failures. Every step is a wrapper command with a machine-readable
>   verdict. Safe for a small model.
> - **AUTHOR** (strong model only): *writing* a new proof — decomposing it into steps and
>   choosing the facts each step needs — is open-ended mathematical reasoning. A weak model
>   will not converge. If the task is to author a proof, **STOP and hand off to a strong
>   model**; do not improvise proof steps. (Specula's own README says the same about its
>   reasoning-heavy phases.)

## Input / Output

- **Input:** a `.tla` module that contains `THEOREM … PROOF … QED` blocks, and the theorem
  name (usually from an obligation dispositioned `tlaps:<theorem>`).
- **Output (MAINTAIN):** confirmation that every obligation discharges (`ALL-PROVED`), with
  the obligation wired + CI lane — OR a precise report of which obligation failed and where.

## The only verdict vocabulary

`host-prove tlaps` prints exactly one line. Match on the first word:

| Verdict | Meaning | Exit |
|---|---|---|
| `ALL-PROVED <module> (<n> obligations)` | Every proof obligation discharged | 0 |
| `FAILED <module>: <k>/<n> (first: <loc>)` | `<k>` obligations did not prove; first at `<loc>` | 1 |
| `ERROR <module>: <msg>` | `tlapm` could not run, or the module has no obligations | 2 |

## Procedure (MAINTAIN)

### Step 1 — Run the proof checker (one command)

```sh
host-prove tlaps --module <module.tla>
```

### Step 2 — Act on the verdict

| Verdict | Do |
|---|---|
| `ALL-PROVED` | Go to Step 3 (wire it). The theorem holds. |
| `FAILED … (first: <loc>)` | Report to the user: name the module, the failing count, and `<loc>`. If you arrived here **after editing the spec**, the edit broke a proof — say which edit and STOP; the proof must be repaired by a strong model. Do **not** add `OMITTED`/`ADMIT` to force a pass. |
| `ERROR: no obligations` | The module has no `PROOF` blocks — there is nothing to check. Authoring is needed → hand off (strong model). |
| `ERROR: …` (other) | Fix what the message names (a missing module on the path, a parse error). If unclear, STOP and report. |

### Step 3 — Wire the obligation + CI lane

1. Disposition the obligation `<id> => tlaps:<theorem>` in the `<spec>.obligations` manifest.
2. Ensure CI runs `tlapm` on the module (a declared `tlaps:` disposition with no TLAPS lane
   is a HAZARD under `host-lifecycle software --check`). TLAPS is verified in the **CI OS
   matrix** using the official prebuilt installer — not Docker, not a local OCaml build.

## Reading a proof (orientation only — authoring is strong-model work)

A TLAPS proof lives inline in the module. Minimal vocabulary, so MAINTAIN can recognise the
structure it is checking (it does **not** license a weak model to write new steps):

```tla
THEOREM Safety == Spec => []Inv
PROOF
<1>1. Init => Inv            BY DEF Init, Inv          \* base case
<1>2. Inv /\ [Next]_vars => Inv'  BY DEF Next, Inv     \* inductive step
<1>. QED  BY <1>1, <1>2, PTL DEF Spec                  \* combine
```

- `<1>1.`, `<1>2.` — proof steps at level 1; deeper levels (`<2>1.`) decompose a hard step.
- `BY f1, f2 DEF d1, d2` — discharge this step using facts `f1,f2` and unfolding definitions
  `d1,d2`. `OBVIOUS` = no facts needed. `PTL` = propositional temporal logic backend.
- `SUFFICES` reshapes the goal; `QED` closes the proof.
- TLAPS tries SMT (Z3), Zenon, and Isabelle backends per obligation; the wrapper reports the
  aggregate pass/fail — you do not choose backends.

If a step `FAILED`, the repair (more facts in `BY`, a finer decomposition, a helper lemma)
is exactly the open-ended reasoning reserved for a strong model. Report and hand off.

## Hard rules

- A weak model MAINTAINS; it does not AUTHOR. When authoring is required, STOP and hand off.
- Never make a `FAILED` go green with `OMITTED`, `ADMIT`, or by deleting the `PROOF` — that
  is a false claim of proof. Report the failure honestly.
- One wrapper command per run; never hand-parse `tlapm` output.
