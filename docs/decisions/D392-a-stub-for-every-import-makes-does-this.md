# D392 - A stub for every import makes "does this symbol exist" unanswerable


**assumed** - 2026-08-30

Running the conformance probe under orbistoun produced 498 passes, 3 failures and a partial.
**Two of the failures and the partial are one bug**, and the probe named it without being
asked to:

```text
900-surface/control      fail  a symbol that does not exist reported present;
                               every count in this section is meaningless
015-sync/machine-kind    fail  the platform reports being both a devkit and a retail unit
005-generation/detect    partial  both generations' drivers resolve (real back-compat, or
                                  a stub-everything loader answering for free)
```

Every import gets a stub, so a call to something unimplemented is *reported* rather than a
jump into a zeroed slot. That is the whole interception model (D005) and it is right for
measuring.

It also means **the platform answers yes to every symbol anything has ever asked about**. A
guest cannot tell a function this emulator implements from one no console ever exported,
because both resolve to an address. A probe inferring a machine's kind, or its console
generation, from which symbols are present gets *both* answers - not because orbistoun claims
to be a devkit, but because it claims to have everything.

### The line, and why it is that one

An import is a hash. This project resolves it to a name through a database it can re-derive
from its own inputs, and a name it **cannot produce** is one it knows nothing about: not that
the function is unimplemented, but that no evidence anywhere here says such a symbol exists.

So `ORBISTOUN_RESOLVE=named` refuses exactly those, and they relocate as unresolved - which
the tally already counted and reported, so refusing is not a new outcome, it is one that had
no way of being chosen.

A refusal is also **not a failure to link**. Entering was gated on a complete relocation
tally, which made the setting unusable the moment it did anything; a run that deliberately
left imports unresolved is as linked as it was asked to be.

### What it changed, and what it did not

Under `named` the probe refuses 5385 of 35851 imports and a library comes back **honestly
absent** - `900-surface/corpus_0176_libSceNpCppWebApi: none of this library is present` -
where before it was silently whole. The census moves from 498/10 to 473/34, and those 24 new
failures are libraries orbistoun does not have and used to claim.

**The control still fails**, so the symbol it uses is one this build *can* name. Whether that
is a real symbol the probe expects to be absent, or a name this project's database should not
contain, is a question for the probe rather than for this repository - and it is the right
kind of question, because it is now about one symbol rather than about the whole method.

### Not the default

Every measurement in `compat/` was taken with everything resolving, and a run that refuses
reaches fewer imports by construction, so the two are not comparable. The default is unchanged
and verified so: `PPSA02664` still stops at `image+0xafc959` with 23 imports and 222 calls, and
the probe's default tally is identical to before the change.

**Accuracy is the eventual default and measurement is the current one**, which is the honest
statement of where this is: a console does not invent symbols, and this project cannot yet
afford not to.

### A setting that changes the program must be in the list that says so

Adding it exposed a gap in the machinery that keeps measurements honest. `Experiments` decides
whether a run was ordinary, and it reads a **hand-written list** of variables - so a new
diagnostic is invisible to it until somebody remembers. `ORBISTOUN_RESOLVE` and
`ORBISTOUN_HANDOFF_POISON` were both missing, and the first `named` run promptly wrote itself
into `obscene`'s **status** slot, which is reserved for runs nothing helped.

The registry already carries `Effect::Intervenes` on both, and `intervenes()` is derived from
it *"so a diagnostic added with the wrong effect is wrong in one place instead of two"* - but
only for variables the struct happens to list. Half-derived, and the half that was manual is
the half that failed. Both are in it now, the record was reverted, and a `named` run leaves the
status slot alone.

**This is the third setting-shaped defect today** - a report that could not fire, a diagnostic
that could not see a thread's stack, and now a diagnostic that did not declare itself. The
common shape is a mechanism that is right where somebody looked and silent everywhere else.

### And the memory map was already right

Of the five behavioural failures sent back from real hardware, the two memory-map ones -
`150-memory-map/walk` and `/after-allocation` - **pass** under orbistoun, through
`sceKernelDirectMemoryQuery`. Silence about them was silence for the right reason, but only
because nobody had looked; the question was fair and the answer is now measured rather than
assumed.

