# D357 - The loop can close a question when the discriminator is arithmetic


**decided** · 2026-08-28 · asked how to close the limit D356 left open

D356 made a turn *ask* the top open question and stop there: it ran every map shape and
reported what each did to the fault, which answers a question about crashing rather than the
one that was asked. Closing that needed three things, and none of them was a model.

### An experiment says what to run and must also say what to read

`Answers::axes` produced axes. Nothing said what the run was **for**, so the only vocabulary
available was every diagnostic's - the fault moved, the guest reached further. Whether a guest
accepts a memory map is not visible in reach at all: it walks every shape correctly and
restarts.

What separates the two live readings of the second field is **which offset it queries next**,
and that is arithmetic on numbers already recorded. `Reading` carries it, and `Undecided`
carries a reason, because a run that *could not* have decided must not be recorded as one that
failed to.

### The shape that would settle it did not exist

`Fragmented` was believed to be this experiment. It is not - it has four regions and every one
begins exactly where the last ended, which is what the knowledge entry warned about in as many
words: *"needs a map with a gap in it, not a map with more regions"*.

Under a contiguous map, feeding back `end` and feeding back `start + size` are **the same
number**. Three shapes existed, all contiguous, so the question was unanswerable by
construction and had been since D218. `MapShape::Gapped` is the first shape that can separate
them.

### A run must record the map it presented

The offsets a guest queries mean nothing without the boundaries it was given, and those were
computed inside the emulator and discarded. Recomputing them from the configured shape would
be a second copy of the thing being measured - and wrong whenever a shape falls back because
its regions do not fit. `Conditions::memory_map` records what was actually built.

### The answer

```
does the guest accept a gapped physical memory map?
  *** it walks by End - the question is answered
```

**The guest feeds back the end of the region it was shown.** A question open since D218,
settled by the loop, with nobody reading a trace.

The three contiguous shapes report *undecided: the map is contiguous, so feeding back an end
and a next start are the same number* - which is the honest answer for a run that could not
have decided, and the reason the fourth shape had to be built.

### The bug in between, and what it looked like

The first wired version reported *"the guest queried fewer than two offsets"* for a title
making **twenty million** of exactly those calls. `calls` in a trace is a summary - one row per
import with a count and no arguments - and the ordered record carrying them is `tail`.

A reader pointed at the wrong array produced a sentence that was false about the guest and
perfectly plausible about the experiment. It was caught only because the number was absurd on
its face.

### Where the limit still is

This closes questions whose discriminator is **arithmetic on recorded data** - an offset, an
address, a count. It cannot close one that asks whether behaviour is *correct*: that needs the
conformance probe, which grades against a spec, or a person. The two classes are worth keeping
apart, because the first is now automatable and the second is not.


