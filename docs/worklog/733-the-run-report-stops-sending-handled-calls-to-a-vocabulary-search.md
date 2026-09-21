# 733. The run report stops sending handled calls to a vocabulary search

**2026-09-20** — PPSA02664's wall is blocked on obSCEne `a3f0` (the `sceAgcInit` state buffer, worklog
731), so this tick took a non-blocked reporting gap the wall investigation kept walking past: the run
report tells a reader to go name a function orbistoun already answers.

## The gap

The `unnamed` finding fires for every call whose label carries a raw NID rather than a name, and it gives
one of two actions - "the title's own code, do not search a vendor vocabulary" for a symbol the game
ships, or "extend the candidate vocabulary and re-run the name search" for a vendor hash. Neither checks
whether the call is **implemented**. So the phantom `0x7d86501b8094ef57` - which orbistoun answers by NID,
with a handler that runs (`implemented: true` in its own trace) - was told, every run, to extend a
vocabulary and search for a name. That search cannot help: the call already runs, and this particular
hash is a vendor alias no hash derivation reaches (the same class of alias `sceAgcInit`'s knowledge
documents). A reader following the advice spends effort on a call that has none to gain.

## The fix

The trace already carries `implemented` per call, so the finding did not need the knowledge base to know
better - only to read the field it was handed. `unnamed` now has a third branch: for an implemented call
it says the missing name is documentation, not a reach gap, and the vocabulary search is the wrong tool;
a hash that never resolves to a name is a title-private symbol or a vendor alias, recorded in its
knowledge entry rather than found in the name table. The phantom's line now reads "has no name, though a
handler answers it by NID", and its action points away from the search instead of into it. The vendor
alias `0x53bbd82b51d172db` (`sceAgcInit`, still unimplemented) keeps the search advice, which is the
right split - and it is **forward-compatible**: once `sceAgcInit` is implemented after `a3f0` lands, it
becomes `implemented: true` and inherits the correct message automatically, with no further change here.

## Why it was worth a tick while blocked

This is the "make the issues less likely to mislead" work, applied to the report itself. The finding had
been quietly steering every reader of a PPSA02664 run - including this project across a dozen worklogs -
toward naming a call that was already handled, which is exactly the kind of confident-but-useless
instruction principle 3 warns a report can produce as readily as a stub. It cost one field read and a
test to stop, and it will keep being right as more of these non-export inlines get answered by NID.

## Gate state

One code change: `crates/orbistoun-report/src/diagnose.rs` - the `unnamed` finding's message and action
gain an `implemented` branch, with the test extended to assert a handled hash is not sent to the search
while an unimplemented vendor hash still is. `./bin/orbistoun check` green; identity scan clean. No
commit.
