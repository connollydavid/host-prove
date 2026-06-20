# Apalache — CLI reference and CI lane

## Commands (subset host-prove drives)

- `apalache-mc typecheck SPEC.tla` — Snowcat type inference; the gate before `check`.
- `apalache-mc check --inv=Inv --length=N [--init=Init --next=Next --cinit=CInit] SPEC.tla`
  — bounded **symbolic** check (SMT/Z3) of `Inv` over all data up to `N` steps.
  Exit 0 + `The outcome is: NoError`, or non-zero + a counterexample under `_apalache-out/`.

Other useful flags: `--smt-solver=z3|cvc5`, `--cinit=CInit` (symbolic constant bounds —
this is what makes the result parametric), `--run-dir=DIR`, `--length=N` (default 10).

Always use `host-prove apalache` over calling `apalache-mc` directly — host-prove runs it
with normalized flags and parses the verdict to one line (no shell pipe to assemble).

## CI lane (OS matrix; official prebuilt binary, no Docker)

Wireable in **any** software repo — no host-relative paths: `host-prove` comes from
`cargo install`, the pinned Apalache from host-prove's own installer (cloned, not `./tools/…`):

```yaml
apalache:
  strategy:
    matrix: { os: [ubuntu-latest, macos-latest] }   # Apalache is JVM; windows-latest also works
  runs-on: ${{ matrix.os }}
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-java@v4
      with: { distribution: temurin, java-version: '17' }
    - name: install host-prove + the pinned Apalache
      run: |
        cargo install --git https://github.com/connollydavid/host-prove --locked
        git clone --depth 1 https://github.com/connollydavid/host-prove /tmp/host-prove
        /tmp/host-prove/install/install-apalache.sh   # version + SHA256 pinned
    - run: host-prove apalache --mode typecheck --spec path/to/Spec.tla
    - run: host-prove apalache --mode check --spec path/to/Spec.tla --inv MyInv --bound length=12
```

The literal `apalache-mc` string in this workflow is what `host-lifecycle software --check`
detects as the Apalache lane being present for a spec that declares an `apalache:` obligation.
