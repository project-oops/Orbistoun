# 418. The classification failed, and found two bugs on the way

**2026-09-04** - directed

## The ask, and the honest answer

Classify the 97 called-but-unimplemented functions that carry no `returns`, using tagged runs to
see what the guest does with each placeholder.

**It cannot be done from the traces as they stand, and the reason is measurable.** Four titles run
under `ORBISTOUN_TAG_PLACEHOLDERS`, 350 distinct stub slots seen, and **exactly one** producer left
an attributable tag - `sceAgcCreateShader`, still live in `rbx` at the fault. Nothing else was
consumable as evidence.

Two limits explain it, and both are structural rather than bad luck:

- **`TracedCall` records `arg0` and nothing else.** A tag passed as `arg1` or `arg2` is invisible.
  The four-gigabyte bug (D564) was only ever visible because `malloc`'s size *is* `arg0`.
- **The recorder keeps 8,192 calls**, so the observation window is 48 calls out of hundreds of
  thousands.

Classifying anything from that would have meant classifying from names, which the same day's
evidence already refuted: of four size-shaped names, `sceAgcDcbSetIndexSize` is a **setter**.

So: no classifications. Recorded because a negative that names its own cause is worth more than a
list of guesses.

## The first bug: the tail was not the tail

Chasing the observation window turned up that `tail` - documented as *"the last calls the guest
made"*, printed as `last calls before the fault`, and given its whole purpose by D154 as *the
neighbourhood of the wall* - is **the last 48 of the first 8,192**.

| Title | Calls | Tail covers | The end? |
|---|--:|---|---|
| PPSA28061 | 332 | #284-#331 | yes |
| PPSA02664 | 418,632 | #8,144-#8,191 | **no** |
| PPSA03416 | 467,457 | #8,144-#8,191 | **no** |
| PPSA25872 | 12,958,032 | #8,144-#8,191 | **no** |

The comment that hid it said *"every run so far stays under that, by an order of magnitude"*. True
when written; now wrong for three of the four titles that reach anywhere, one of them over by three
orders of magnitude.

**It had already cost a wrong reading of mine, the same day.** PPSA02664's tail was reported as
`strlen`/`memcpy` and read as *"the guest is in its own code, not immediately after a failed
import"* - from calls 8,144-8,191 of a 418,632-call run. Withdrawn. The printer now says which it
is (D568).

## The second bug: I overwrote three honest records

The tagged runs were **recorded as honest measurements**. The guard that refuses to record an
intervened run exists and is emphatic (D227, D355) - it never fired, because
`Experiments::intervenes` kept a **hand-maintained list** of which diagnostics are in force, under
a comment claiming the registry was the source. The effects were derived; the presence list was
not.

Now `any_intervenes` asks `orbistoun_env::active()`, which walks the whole registry - so a
diagnostic is covered the day it is declared. Three guards, two broken and caught; the third break
did not fire and is written into the test, because the one-line effectful wrapper is deliberately
untested rather than tested flakily through process-global environment (D569).

Records re-measured and restored, two of them with `--force`: the polluted numbers were marginally
*higher*, and `beats` refuses anything that is not an improvement. **Second time today that "a
record only moves up" was the obstacle rather than the protection.**

## What this day keeps teaching

Three of my own conclusions corrected in one session - D559's Class A, D564's rule, and now the
tail reading - and every one found by reading something this repository already contained. The
failure is identical each time: a conclusion drawn from a reading, when the thing that settles it
was one grep away.
