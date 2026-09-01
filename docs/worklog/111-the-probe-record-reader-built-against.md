# The probe-record reader, built against transcripts and no hardware


obSCEne published `docs/PROTOCOL.md` - the contract, written before either implementation,
and shipped with ten captured exchanges whose stated purpose is that a consumer can be
built and tested with nothing plugged in. So it was.

`orbistoun-probe` reads what a probe emits: requests, records, sessions, capabilities and
outcomes. It does not drive a session, does not open a socket, and does not know a Steam
Deck exists. **Nine conformance tests against all ten real transcripts, first run green.**

The transcripts are **copied in as data**, not referenced across repositories. A test that
reads a sibling checkout fails for anyone without one, and a build dependency between the
two projects is the coupling D207 exists to prevent. A transcript is data, so copying it is
the right kind of duplication; copying code would be the wrong kind.

### The one design decision

`Outcome` carries a value **only** in the variant that observed one. `Died`, `Timeout` and
`Lost` have no result field - not an `Option`, no field at all.

The protocol states the rule this enforces: a command that did not answer is never recorded
as having answered, and a corpus blurring `died` with `returned 0` is worse than no corpus,
because the fiction is indistinguishable from evidence. Giving those variants a value would
leave somewhere for a reader to find a number nobody observed, and a number found in a
record is eventually trusted.

Defence in depth rather than types alone: the parser also *refuses* a `done` record whose
outcome did not answer but which carries a value, with a message saying why. Unrepresentable
in the type, rejected at the door, and asserted across every transcript.

### Surprises

**The specification had already absorbed the whole conversation.** `ack` flushed before the
command runs so that death is legible from the other end; `died`/`timeout`/`lost` kept
distinct with `timeout` deliberately not resolved into `died`; `observed-by=driver` so a
fact reported by the system is distinguishable from one inferred from its silence; `part`
records carrying what produced an answer, justified with this project's own wrong-generation
failure. Nothing needed negotiating.

**Capability negotiation is doing real work already.** The stand-in target announces
`call,read,blob,reset,report,gpu` and **no `resolve`** - it has none of the platform's
libraries. A consumer discovers that rather than assuming it, which is the difference
between a question refused and a question answered wrongly.

**The transcript covering a death spans two sessions, and that is the feature.** A faulting
command ends the probe; a fresh session identifier is how the discontinuity becomes visible
instead of silently continuous. `part` records are bound to the session they name rather
than the most recent one - binding them to whichever came last would attribute one process's
answers to another.

**The workspace still does not build, still not here.** `orbistoun-report` is missing a
struct field mid-edit, on top of `orbistoun-elf`'s missing import. My crates were verified
directly: 265 tests, clean clippy, clean fmt.


