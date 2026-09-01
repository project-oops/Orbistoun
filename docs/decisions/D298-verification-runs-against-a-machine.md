# D298 - Verification runs against a machine that has learned nothing


**decided** · 2026-08-26 · `--verify` reported "0 of our own" on the machine that produced the file

The first `--verify` against an honest submission found nothing to compare it with. The reason
is structural rather than a bug in the comparison: the measurement had already been **applied**,
so the wall it was measured at no longer existed, so the sweep produced no finding, so there
was nothing to re-derive.

Left alone that is fatal to the whole idea. Applying a measurement would make it permanently
unverifiable, and every later submission would be checked against a machine whose behaviour
had already been changed by the answers it was meant to be checking.

So a verifying turn runs in **its own data directory** - no learned file, no accumulated
policy, traces of its own - which is the state the original measurement was taken in. Nothing
new was needed for it: `ORBISTOUN_DATA_DIR` already decides where everything a run reads and
writes lives, and the propose tests already point trials at a temporary root for exactly this
reason, so a sweep cannot pick up an unrelated run's trace.

**The general shape is worth naming**, because it will come back. A machine that acts on what
it learns cannot check what it learns from the state it is in; it has to be able to ask "what
would I have measured knowing nothing?". Anything that accumulates - the learned file today,
the name vocabulary already - needs a way to be consulted with the accumulation switched off,
or its own output becomes the thing that confirms it.

