# 2026-08-27 - Proof of sourcing for the harvested constants


The constants are the only table here that is neither derived by experiment nor written by
a person - copied out of somebody else's headers - so *where did this number come from* is
the only question about them, and until now nothing answered it after the fact.

`./orbistoun.sh check` regenerates and diffs (D354). Pointed at three tamperings before
being believed:

```
hand-edit a value                     caught
delete a constant                     caught
name a different revision in the header   PASSED   <- the one that mattered
```

### The hole

`--revision` was an argument stamped into the header, so the header was a **claim** - and
the gate re-derived the file *using that claim*. Regenerating with whatever the file said
produced a file saying the same thing, so `ee81cd1d` edited to `deadbeef` matched itself.

For a table whose whole purpose is provenance, that is the one failure that counts: the file
could name any source and nothing would notice.

The generator asks `git -C <source> rev-parse HEAD` now, so the header says what the harvest
actually read. `--revision` is gone rather than made optional - an override would restore
exactly the hole it closed. All three are caught.

### What found it

Watching the guard *reject* things rather than watching it pass. Two of three were caught
immediately and read as confirmation; the third was the only one worth running. **Two-thirds
of a guard looks exactly like a whole one.**

### Also

A raw NUL byte had got into `orbistoun-libc/src/lib.rs` - my `strerror` message - which
compiled fine and made every text tool treat the file as binary, so `grep` silently stopped
answering questions about it. Cause: this shell collapses backslashes inside quoted
heredocs, so a `\0` written into a Python string arrived as a real NUL byte, because Python reads it as an
**octal escape for NUL**. Worth knowing, because three attempts to fix it failed the same
way before the cause was clear.


### Three titles turned, and two of the results went nowhere

PPSA28061, PPSA25872 and PPSA21564 had never had a turn. PPSA21564 found a real contract -
`sceLibcMspaceMalloc` answering the code the guest followed, zero reaching **25 against 13**,
reproducing a finding from earlier in the session exactly - and **nothing was written**.

`--apply` was gated by a caution about *tracked* files, and what it writes is not tracked. So
an ordinary turn persisted nothing. Emitting a proposal and applying policy are different
acts and only one of them needed gating: `turn` now always writes to `patches/`, verified to
apply with `git apply --check`, with the assumption intact - *"zero is what a pointer-returning
function must answer rather than an error code; what it should really return is not
measured"*.

Making that possible needed one more fix: `unimplemented_calls` stripped `library::` from
every candidate because `ORBISTOUN_RETURN` cannot express a qualified name. True of the
variable, and it discarded the library for everything downstream, so no measurement could say
which knowledge file it belonged in.

### The same diagnostic, four different answers

| title | BSS poisoned with 0xa5 |
|---|---|
| PPSA28061 | **stopped faulting** - reach saturated |
| PPSA25872 | broke earlier: 2 against 14 |
| PPSA21564 | broke earlier: 7 against 13 |
| PPSA02664 | broke earlier |

One title responds differently to the same intervention - a comparison across guests nothing
here could make before today, and only legible because the report names the *kind* of change
rather than counting the silent ones (D331).

Three results now say "it stopped faulting", and all three print that reach has saturated so
the probe is what separates them from a wrong answer. That is D301 firing on real data, in
exactly the cases where the naive reading is "we fixed it".

### The bug, and the gap that was not one

Adding recording to `turn` filed `25 imports, ran to the time limit` for a title that reaches
13 - a number bought by a reserved region the guest never asked for, recorded as a
compatibility claim. `record_compat` refuses an intervened run now; **the hazard was already
there for `run`** and nothing had exercised it.

And the gap that prompted it was not real. `GuestTrial` shells out to `orbistoun-cli run` for
every boot, so each boot already records itself - the baseline honestly, the intervened ones
now refused. The call added to `cmd_turn` was redundant, read the last trace, and printed
"not recorded" on every turn while the baseline had recorded fine. Removed.

It surfaced by the guard printing "not recorded" **while the file changed anyway** - two
statements that could not both be true, which is the only reason it was chased.

### The loop now asks its own questions

A turn ended by saying a person must write code, on a function taking 67.5% of every call the
corpus makes - while the project had already written down what it did not know, ranked it, and
named the experiment. 277 open questions, machine-readable, and **nothing read them**: the
dispatcher is driven entirely by run reports.

The apparatus was already there too. `MapShape` has had three variants since D218 with **no
consumer anywhere in the tree**. The experiment was designed and never wired to anything a run
could turn.

Four pieces were missing and all four are small: a variable to select a shape
(`ORBISTOUN_MAP_SHAPE`), an axis to sweep it, a field on a knowledge entry naming which
experiment answers its questions (`answerable_by`), and a turn that reads them.

A label rather than prose, because classifying a question by its words is guesswork wearing a
rule's clothes - and an unlabelled question is reported rather than filtered, so "no rule for
this" and "nothing to ask" stay different facts.

### The fourth instance of one bug in a day

The first run reported that **all three map shapes stopped the fault**. PPSA04263 spins to the
time limit and never faults, so nothing had stopped.

```rust
(_, None) => Change::NoLongerFaulted,
```

A wildcard on the baseline. Fixed, and the honest answer came back: the map shape makes **no
difference** to that title - which is the first answer that question has ever had.

The progress verdict, the sweep's oracle, `Derailed`, and now this: four times today a field
whose meaning is only defined when a fault happened was read on a run where none did.
`Option<u64>` where `None` means *it did not happen*, consulted as though it meant *it
happened, at zero*. Worth naming as a class, because writing each fix down separately has not
stopped the next one.

### The loop closed a question that had been open since D218

```
does the guest accept a gapped physical memory map?
  *** it walks by End - the question is answered
```

**The guest feeds back the end of the region it was shown.** Nobody read a trace.

Three things were needed and none was a model. An experiment has to say what to **read**, not
only what to run - reach answers "did it crash differently", and whether a guest accepts a map
is invisible in reach because it walks every shape correctly and restarts. A run has to record
the **map it presented**, because the offsets a guest queries mean nothing without the
boundaries it was given. And the shape that could settle it had to exist at all.

That last one is the finding. `Fragmented` was believed to be this experiment and is not: four
regions, every one beginning exactly where the last ended. Under a contiguous map, feeding
back `end` and feeding back `start + size` are **the same number** - so all three shapes were
unanswerable by construction, which is what the knowledge entry had said in as many words:
*"needs a map with a gap in it, not a map with more regions"*.

The three contiguous shapes now report *undecided, the map is contiguous* - the honest answer
for a run that could not have decided, and the reason the fourth had to be built.

### The bug that was plausible and absurd at the same time

The first wired version reported *"the guest queried fewer than two offsets"* for a title
making **twenty million** of exactly those calls. `calls` in a trace is a summary - one row per
import with a count and no arguments - and `tail` is the ordered record carrying them.

Pointed at the wrong array, the reader produced a sentence that was false about the guest and
entirely plausible about the experiment. Caught only because twenty million and "fewer than
two" cannot both be true.

### Where the limit now is

Questions whose discriminator is **arithmetic on recorded data** - an offset, an address, a
count - the loop can now close. One that asks whether behaviour is *correct* it cannot: that
needs the conformance probe or a person. Worth keeping the two apart, because the first is
automatable and the second is not.

