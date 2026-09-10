# D632 - One function, two answers, decided by how the guest asked

**Status:** measured
**Date:** 2026-09-08

## The claim that was true of two-thirds of the table

`sceKernelDlsym`'s own doc comment says what makes it trustworthy:

> A name is looked up in the same table of stubs the linker resolves imports into, so a function
> reached this way and the same function reached by an import are the **same address**.

It is true of every function this project implements, and false of every one it merely declares.
The by-name table is built from `symbols::resolvable()` - the implemented set - so a name reached
by **import** lands on a stub answering the placeholder, and the same name reached by **name** is
refused. **949 declared, 680 implemented**: 269 names with two answers depending on how the guest
asked.

One payload run asks by name for **266** of them.

Four entries on the differential are that, and they say so in the probe's own words:
`101-input-ext/keyboard-presence`, `101-input-ext/mouse-presence`, `107-videodec/symbols`,
`108-audiodec/ajm` - all *"loaded but no symbols resolved"*, for libraries orbistoun declares and
stubs.

## Two-sided, so measured rather than argued

The console resolves both. Whether orbistoun should is not obvious, and D125's reasoning against
is real: a guest handed a stub calls it and gets a placeholder, where a guest handed null may take
a fallback path it would have preferred. An argument does not settle that; a run does.

So `ORBISTOUN_DLSYM_STUBS` - declared as `Effect::Intervenes`, off by default, and the table it
installs is **empty unless a run asks**, which is what makes the two runs comparable at all.

## What the runs said

The conformance payload, which resolves by name:

```text
orbistoun: 105 declared name(s) are resolvable by name under ORBISTOUN_DLSYM_STUBS
  imports  226 distinct (+3) …  fault the guest called exit   verdict FURTHER
```

And the second observation, of a different kind, because an intervention that moves a wall is not
a diagnosis - the probe's own verdicts against the console:

| | baseline | under the flag |
|---|---|---|
| checks both ran, concluding differently | 255 | **240** |
| passed there, failed here | 115 | **106** |
| distinct findings | 17 | **13** |

The four *"loaded but no symbols resolved"* entries are gone, and the absent-library census shrank
from 99 to 94.

**And three titles are byte-identical**: PPSA03416, PPSA02664 and PPSA04263 report the same
imports, the same call counts, the same standing and the same fault address with the flag and
without. They resolve by import, so this cannot touch them - now measured rather than assumed.

## Left as a flag

No guest in the corpus got worse and one got better, which is evidence and not proof: four guests
is a small population, and the case against is about a guest that prefers a null it can branch on
- exactly the kind that would not appear in a corpus assembled from guests that already run.

Changing what `dlsym` answers by default is a user-visible behaviour change, which this project
flags rather than assumes. The flag, the measurement and the recommendation are here; the default
stays as it was until somebody decides.

The narrowing in D629 sits above this and is unaffected: a libkernel handle still refuses a name
declared elsewhere, whether or not this table is installed. Order matters and is fixed - an
implementation first, then the guest's own export, then this - so a stub for a declared name can
never shadow either.
