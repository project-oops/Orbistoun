# D231 - A chooser is slower than running everything it would choose between


**decided** · 2026-08-25 · measured, after building the thing this retires

The plan was an orchestrator: something that understands orbistoun's tools and picks the
next experiment given the evidence. Two measurements, taken while building it, say that is
the wrong shape.

| | cost |
|---|---|
| one boot of the guest against a wall | **~0.13 s** |
| every argument of every import, exhaustively | 276 boots, 50 s |
| every other diagnostic axis against one fault | 6 boots, **1 s** |
| one answer out of a local model, on the GPU | 5-20 s |

A boot is cheap because a wall is hit early. So **the model is slower than exhausting the
space it would be selecting from**, and any prior it supplies saves nothing. This already
retired `orbistoun-propose::wall`, which ranked which import and argument to suspect; it now
also decides what the orchestrator is.

`orbistoun-propose::turn` is therefore a **dispatcher**, not a chooser. Every `Gap` the
report can name maps to a fixed `Step`, and where the step is a sweep the sweep is
exhaustive. A model appears in exactly one branch - `NameAHash` - and that is a measured
placement rather than a preference: of the names a model earned against the hash oracle,
not one exists whole in any module string and none of its words appears in any
vendor-prefixed string, so the string harvester could not have found them. The two sources
are disjoint, not redundant. `only_the_branch_with_an_oracle_behind_it_proposes_anything`
pins it, so widening it is a deliberate act rather than a drift.

Three consequences worth stating, because each is a thing not built:

- **No ranking.** The report already ranks by how many calls a finding concerns, which is a
  fact about the run. Re-ranking here would be claiming to know better on no evidence.
- **No implementation.** `Gap::Unimplemented` maps to `Step::Person`, pinned by a test,
  because THE_LOOP is explicit that step 18 is a person writing code.
- **A refusal states why.** `Step::Person` carries a sentence naming what is missing.
  A dispatcher that quietly did something plausible with a gap it has no rule for is
  principle 3's failure one level up.

Measured end to end against a live title: eight findings, nine steps, everything mechanical
done in **2.6 seconds**, stopping where a person is genuinely required.

