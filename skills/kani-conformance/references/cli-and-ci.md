# Kani — CLI flags and the CI lane

## `host-prove kani` (what to run)

```
host-prove kani --harness <NAME> [--dir <CRATE>] [--bound unwind=<K>]
```

host-prove runs `cargo kani --harness <NAME> --output-format terse` itself (adding
`--default-unwind <K>` when `--bound unwind=<K>` is given) and prints one verdict line —
you never pipe to a parser or read raw Kani output. Exit 0 = proved, 1 = a real
counterexample, 2 = the tool could not run.

Useful underlying `cargo kani` flags (host-prove sets the ones it needs): `--default-unwind N`
(the loop/recursion bound — the soundness `bound`), `--concrete-playback=print` (emit a
replayable counterexample test, named in the `FAILED …` replay hint), `--output-format terse`.

## Install (pinned)

Kani is a **cargo-locked source build** (`cargo install --locked kani-verifier && cargo kani
setup`), not a SHA256-verified prebuilt binary — `tools.lock` records `install=cargo-locked`,
`sha256=n/a`. host-prove's `install/install-kani.sh` pins the exact version.

## CI lane (wireable in any software repo — no host-relative paths)

`host-prove` comes from `cargo install`; the pinned Kani from host-prove's own installer
(cloned, never `./tools/…`):

```yaml
kani:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: install host-prove + the pinned Kani
      run: |
        cargo install --git https://github.com/connollydavid/host-prove --locked
        git clone --depth 1 https://github.com/connollydavid/host-prove /tmp/host-prove
        /tmp/host-prove/install/install-kani.sh   # pinned version; cargo kani setup
    - run: host-prove kani --harness my_proof --dir . --bound unwind=20
```

A declared `kani:` disposition with **no** Kani CI lane is a HAZARD under
`host-lifecycle software --check` (the conditional rung gate). The lane re-runs the proof, so
the discharge is the verifier passing on re-derivation (`call/0018`) — not this verdict's word.
