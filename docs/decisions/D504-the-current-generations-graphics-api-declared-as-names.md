# D504 - The current generation's graphics API, declared as names and nothing else

**decided** - 2026-09-03

D500 found that orbistoun declares `libSceGnmDriver` - the **previous** generation's graphics
interface - while guests call `Agc`, and that not one `Agc` name was declared or censused
anywhere in the project. Six of them were being called and reported as `unknown::`.

Declared. Nothing implemented.

```text
declared functions   674 -> 735
unresolved imports   342 -> 286        (PPSA02664's executable)
```

## Where the names come from, which is the whole of what makes this legitimate

**Real import tables.** PPSA02664's executable imports **fifty-one** names from `libSceAgc` and
**five** from `libSceAgcDriver`; five further `libSceAgcDriver` names appear in the recorded
corpus from other modules. Every declared name is one of those.

That is the provenance `orbistoun-input` already documents for `libScePad`: *"not a name this
project derived and hoped matched, but one a real module demonstrably asks for."* No name here
was completed from a pattern, and the six that the corpus shows as `<unknown>` hashes in the
same library are **not** declared, because a hash is not a name.

## Names confirmed, arities not - and the precedent for saying so

`orbistoun-input` states the asymmetry and it holds here for the same reason:

> a wrong arity degrades a call trace and does not break the call, while a wrong name means a
> NID that matches no import and a shim that can never be reached.

Checked rather than trusted: `arity` is carried into `ImportDesc`, the CLI's listing, and the
knowledge generator. **It never reaches the call path.**

**Every arity is 6, and that is not a claim that these take six arguments.** Six is the
trampoline's full capture. With nothing established, recording every argument register loses no
information where guessing low silently discards it - and for a command-buffer interface the
arguments *are* buffer addresses and sizes, which is what a trace of it exists to show.

One exception, and the guard found it: **`sceAgcCreateShader` is declared with 4**, because the
knowledge base already recorded that arity from a dump (D194).
`declared_arity_and_recorded_arity_never_disagree` refused the uniform 6 and was right to - the
record is the older claim, so the declaration defers to it.

## `SERVES_NOTHING` gets its first entries, with reasons

`every_declared_library_either_serves_something_or_says_why_not` refused two libraries that
declare sixty-one functions and implement none. That guard exists because `orbistoun-input` was
once registered with its implementations missing, and nothing failed.

The list was empty until now, and these are the honest use of it: the first frame is Phase 6,
which has not begun, and principle 6 puts a subsystem after the address space and threads.
**Implementing one of these to shorten the list would be writing the abstraction before its
caller.**

## What it bought, measured, including what it did not

Fifty-six of PPSA02664's imports stop being unresolved, and a guest reaching the graphics
interface is now named, counted and answered by the loud stub policy rather than by a
placeholder a caller might store as a handle (D125).

**It changed nothing about how far the guest gets.** Four runs after: 2077/44 and 2080/46, the
same bimodal pair as before, the same fault at the same address. The guest dies long before it
reaches graphics - which is what principle 6 predicts and why this is preparation rather than
progress. Recorded because a declaration that moved the wall would have been a surprise worth
chasing, and one that did not is worth not chasing.
