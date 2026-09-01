# A probe cannot certify its own machine, and the grading was resting on it


obSCEne's network handover (`docs/HANDOVER-ORBISTOUN-NET.md`) corrected the most important
assumption in `orbistoun-probe`, and it was mine.

**Machine identity is asserted by the operator, never self-reported.** Running inside an
emulator, obSCEne's call to the platform's version query returns *the emulator's* chosen
version - so a probe stamping that as `firmware=` would be putting an emulator's answer in a
console's badge. It would look exactly like a measurement of real hardware, which is the one
confusion this project's whole grading vocabulary exists to catch.

My grading read `parts["target"] == "console"` straight off the wire and believed it.

### What changed

`Origin` is now a separate type carrying what the **operator** asserts - device, firmware,
and one load-bearing boolean, `real_hardware`. Grading turns on that boolean and on nothing
the session said. `Session::is_target()` is gone; `Session::claimed_target()` replaces it and
is documented as a claim rather than evidence, kept because a claim that disagrees with the
operator is worth seeing.

`Origin::unasserted()` is the default and grades nothing above an assumption. A client that
forgets to ask produces a corpus of assumptions, which is recoverable; a corpus of
measurements that were never measured is not.

`orbistoun probe` gained `--device`, `--firmware` and `--real-hardware`. The same transcript,
claiming `target|console` throughout:

    (no flags)         results   0 of 2 are facts
    --real-hardware    results   2 of 2 are facts, measured 2
    --device shadPS4   known_by = "assumed", "the operator did not assert real target
                       hardware for shadPS4"

### Surprises

**The correction invalidated a refusal I was pleased with.** `--as-knowledge` used to refuse
on a report because a report has no session to grade from. With grading moved to the
operator that reasoning evaporated - a report plus an assertion is perfectly gradeable - so
the refusal came out. A guard built on the wrong foundation looked exactly like a careful
one.

**Only three verbs are implemented today.** `hello`, `report` and `bye`. `resolve`, `call`,
`read`, `write`, `blob`/`run` and `reset` are in the grammar and refused - which explains,
rather than merely confirms, why no `call` records exist anywhere. The earlier decision to
leave those record kinds unparsed was right for a better reason than the one given.

**The handover's checklist is a list of things already built.** TCP client aside, every item
- client-owned sequences, refusing un-announced verbs, restart detection, ack-before-done
with no value on a non-answer, replay from transcripts, never a socket in CI - was already
done from the specification alone. The one item that was not is the operator form, and that
is the one that mattered.

