# Apalache — CLI reference and CI lane

## Commands (subset host-prove drives)

- `apalache-mc typecheck SPEC.tla` — Snowcat type inference; the gate before `check`.
- `apalache-mc check --inv=Inv --length=N [--init=Init --next=Next --cinit=CInit] SPEC.tla`
  — bounded **symbolic** check (SMT/Z3) of `Inv` over all data up to `N` steps.
  Exit 0 + `The outcome is: NoError`, or non-zero + a counterexample under `_apalache-out/`.

Other useful flags: `--smt-solver=z3|cvc5`, `--cinit=CInit` (symbolic constant bounds —
this is what makes the result parametric), `--run-dir=DIR`, `--length=N` (default 10).

Always prefer the host-prove wrappers (`scripts/apalache_check.sh`) over calling
`apalache-mc` directly — the wrapper normalizes flags and parses the verdict to one line.

## CI lane (OS matrix; official prebuilt binary, no Docker)

```yaml
apalache:
  strategy:
    matrix: { os: [ubuntu-latest, macos-latest] }   # Apalache is JVM; windows-latest also works
  runs-on: ${{ matrix.os }}
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-java@v4
      with: { distribution: temurin, java-version: '17' }
    - run: ./tools/host-prove/install/install-apalache.sh   # version + SHA256 pinned
    - run: ./tools/host-prove/scripts/apalache_check.sh typecheck path/to/Spec.tla
    - run: ./tools/host-prove/scripts/apalache_check.sh check path/to/Spec.tla MyInv --length=12
```

The literal `apalache-mc` string in this workflow is what `host-lifecycle software --check`
detects as the Apalache lane being present for a spec that declares an `apalache:` obligation.
