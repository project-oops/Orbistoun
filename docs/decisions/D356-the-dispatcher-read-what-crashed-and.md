# D356 - The dispatcher read what crashed and never what we had written down as unknown


**decided** · 2026-08-27 · asked why a person was needed for something already specified

A turn ends by reporting *implementing a function is a person writing code*. On
`sceKernelDirectMemoryQuery` - **67.5% of every call the corpus makes** - that was false. The
project had already written down what it did not know, ranked it, and named the experiment:

```
500031 calls in 4 module(s)   libkernel::sceKernelDirectMemoryQuery
  ? The map shape the guest will accept is unknown: it completes the walk,
    finds nothing it wants, and starts again.
  ? Fields 0 and 2 are never fed back... That needs a read watchpoint on the buffer.
```

277 questions, machine-readable, ranked by call volume. **Nothing read them.** `grep` for the
questions API in the dispatcher returned nothing: it is driven entirely by run reports - what
crashed, this time.

And the apparatus was already built. `MapShape` has had three variants since D218 with
**no consumer anywhere in the tree** - the experiment was designed, the enum written, and
nothing ever selected between them. The instrument existed, the question existed, and no code
joined the two.

### What was missing, in full

| piece | state |
|---|---|
| the question | recorded and ranked since D218 |
| the shapes | `MapShape`, three variants, no consumer |
| a way for a run to select one | **absent** |
| a diagnostic axis for it | **absent** |
| anything reading questions | **absent** |

`ORBISTOUN_MAP_SHAPE` now selects one, `Axis::MapShape` sweeps them, and a turn asks the
highest-ranked question that names its own experiment.

### Why a label and not prose

A question is written for a person, so classifying one by its words is guesswork wearing a
rule's clothes - and a rule that silently fails to match is indistinguishable from a question
nobody can act on. A knowledge entry names the experiment in a field, `answerable_by`, and
anything unlabelled is reported rather than filtered away: *"we have no rule for this"* and
*"there is nothing to ask"* must not look the same.

A label this build does not recognise is printed as an error, not dropped. A knowledge file
naming an experiment that does not exist is a claim nobody can act on, and silence is how it
stays that way.

### The fourth instance of one bug, found by running it

The first run reported **all three shapes stopped the fault**. They had not: PPSA04263 spins
to the time limit and never faults at all.

```rust
(_, None) => Change::NoLongerFaulted,
```

A wildcard on the baseline. A run that never faulted cannot stop faulting, and three
interventions came back as though each had fixed something. Fixed to `(None, None) =>
Nothing`, and it now reports honestly: **the map shape makes no difference to that title** -
a real measurement, and the first answer to that question in the project's history.

**That is the fourth time today** a field only meaningful when a fault happened was read on a
run where none did: the progress verdict (D309), the sweep's oracle (D351), `Derailed`
(D351), and this. The shape is always the same - `Option<u64>` where `None` means *it did not
happen*, consulted as though it meant *it happened, at zero*.

### What it does not do

It runs the experiment and prints what each run did. Concluding that a shape is *the* shape
needs the guest to accept it and get further, which is a judgement this leaves alone. The
loop asks the question; it does not decide it has been answered.


