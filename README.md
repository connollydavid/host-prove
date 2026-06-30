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
the `host-prove` binary (Rust, `cargo install`) is the single verdict+bound parser all
three wrappers pipe through; `cargo test` / `tests/run.sh` prove it maps real tool output
to the right verdict with no verifier installed. (Authoring a *new* TLAPS proof is the one genuinely reasoning-heavy task — the
`tlaps-proof` skill scopes a weak model to running and maintaining existing proofs and
flags authoring as strong-model work.)

## Install: a separate, pin-bound verb

`host-prove install <kani|apalache|tlaps>` is the one verb that touches the network or the filesystem.
It reads the embedded `tools.lock` pins, fetches the asset, SHA256-verifies it before extracting or
executing it, and installs into a version-stamped directory. Apalache and TLAPS are official prebuilt
binaries verified against the pin; Kani is a cargo-locked source build (`cargo install --locked`,
`sha256=n/a`), so its bind is the version, not a binary hash. No Docker, no OCaml build.

A run never installs. It is pure-local and binds the verifier to the embedded pin before running it:
an absent or wrong-version verifier is `BLOCKED` (exit 2) and names `host-prove install <tool>`, never
a pass or a fail (`call/0036`). A pin bump moves the version-stamped lookup path, so the old version is
never silently reused. `host-prove doctor` reports each verifier's installed-versus-pinned status
without running a proof. A sandbox or hermetic build runs the pure-local path; a connected operator or
a CI provisioning lane runs `install` deliberately (CI machine-verifies the install path against the
pin in the `install-smoke` job).

## Layout

```
skills/{apalache-symbolic,tlaps-proof,kani-conformance}/   # SKILL.md + guide.md (+ references)
src/main.rs                                                # the binary: install/resolve/run ONE verifier, emit one verdict + bound
tools.lock                                                 # the embedded pins (version, asset, sha256)
tests/                                                     # verdict fixtures + runner
```

The skills are wired into a host's `.claude/skills/` by `link-skills.sh`, exactly as the
allium/specula/host-lifecycle skills are. host-prove is referenced, never vendored.

## License

Unlicense (public domain) — see `LICENSE`. The verdicts and proofs host-prove produces
about your project belong to your project, not to this tool's license.
