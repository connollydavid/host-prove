# host-prove

The **deep-verification lane driver** for the agentic-host methodology — the `host-*`
tool that lets an agent drive the upper rungs of the verification ladder, all the way
down to a small/weak model.

The methodology's ladder already has property-based testing (allium) and bounded model
checking (specula/TLC). host-prove adds three tiers on top, each as an **agentic skill**
plus a thin wrapper that turns the tool's output into one machine-readable verdict line:

| Skill | Tool | Rung | Discharges an obligation dispositioned |
|---|---|---|---|
| `apalache-symbolic` | [Apalache](https://apalache-mc.org) (TLA+ → SMT/Z3) | symbolic / **parametric** spec check | `apalache:<inv>` |
| `tlaps-proof` | [TLAPS](https://proofs.tlapl.us) (`tlapm`) | **unbounded** proof (prove-for-all) | `tlaps:<theorem>` |
| `kani-conformance` | [Kani](https://model-checking.github.io/kani) | **code** ↔ spec conformance (Rust) | `kani:<harness>` |

The first two are spec-side (issue #3 — the solver tier); the third is code-side (issue
#4 — verify the implementation against its spec). Together they complete the chain:
*spec proven correct → code proven to implement it*.

## Opt-in, inert until activated

These lanes are **optional and dormant by default**. A tier turns on only when a project
*declares* it, by dispositioning an obligation as `kani:` / `apalache:` / `tlaps:` in a
`<spec>.obligations` manifest. Until then nothing is installed, nothing runs, and
`host-lifecycle software --check` raises no tier HAZARD — exactly as a `.allium`/`.tla`
spec obliges its lane only when present. Declaring a tier obliges wiring its CI lane; not
declaring it costs nothing.

## Built for weak models

Each skill is procedural, not reasoning-heavy: one exact wrapper command per step, a
fixed verdict vocabulary the agent matches on (never raw tool output), decision tables
with explicit STOP conditions, and fill-in templates instead of open-ended authoring.
`scripts/verdict.py` is the single parser that all three wrappers pipe through;
`tests/run.sh` proves it maps real tool output to the right verdict with no verifier
installed. (Authoring a *new* TLAPS proof is the one genuinely reasoning-heavy task — the
`tlaps-proof` skill scopes a weak model to running and maintaining existing proofs and
flags authoring as strong-model work.)

## Reproducible install — like our Rust

`install/*.sh` fetch each tool's **official prebuilt binary**, pinned to an exact version
and verified against the SHA256 recorded in `tools.lock`, then expose it on PATH. No
Docker, no OCaml build. Re-running reproduces the identical verified binary — the analog
of `.host-software`'s digest-pinned toolchain + recorded artifact hash. The tools are
cross-platform enough to gate via a CI OS matrix (Apalache + Kani on ubuntu/macos;
Apalache also windows; TLAPS on its Linux prebuilt installer).

## Layout

```
skills/{apalache-symbolic,tlaps-proof,kani-conformance}/   # SKILL.md + guide.md (+ references)
scripts/{verdict.py, apalache_check.sh, tlapm_check.sh, kani_check.sh}
install/{install-apalache.sh, install-tlaps.sh, install-kani.sh, _common.sh}
tools.lock                                                 # pinned (version, asset, sha256)
tests/                                                     # verdict.py fixtures + runner
```

The skills are wired into a host's `.claude/skills/` by `link-skills.sh`, exactly as the
allium/specula/host-lifecycle skills are. host-prove is referenced, never vendored.

## License

Unlicense (public domain) — see `LICENSE`. The verdicts and proofs host-prove produces
about your project belong to your project, not to this tool's license.
