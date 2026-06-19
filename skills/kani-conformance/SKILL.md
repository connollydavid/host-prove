---
name: kani-conformance
description: "Verify Rust code against its spec with the Kani model checker. Use when: (1) discharging an obligation dispositioned `kani:<harness>` — proving a function's contract holds for ALL bounded inputs, not just example tests, (2) proving panic/overflow freedom or a spec-level property at the code level, (3) reproducing and recording a counterexample Kani found. Drives `cargo kani` through host-prove's wrapper so the verdict is one machine-readable line."
---

Read `guide.md` for the full procedure.
