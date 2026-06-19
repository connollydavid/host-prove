---
name: apalache-symbolic
description: "Symbolically check a TLA+ invariant with Apalache (SMT/Z3) — proving it for PARAMETRIC/unbounded data, not just the finite instances TLC enumerates. Use when: (1) discharging an obligation dispositioned `apalache:<inv>` — a `.tla` invariant quantifying over all N (servers, workers, items) that bounded TLC can only sample, (2) you need symbolic coverage of a data domain TLC cannot enumerate, (3) classifying an Apalache counterexample. Drives `apalache-mc` through host-prove's wrapper so the verdict is one machine-readable line."
---

Read `guide.md` for the full procedure.
