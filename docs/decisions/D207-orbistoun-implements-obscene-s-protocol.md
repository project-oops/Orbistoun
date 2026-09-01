# D207 - orbistoun implements obSCEne's protocol; it does not shape it


**Status:** decided (2026-08-21) - agreed with the user before either side has code

A conformance probe running on real hardware answers questions this project can otherwise
only infer. The two programs need to talk. The whole risk in that is coupling: the moment
obSCEne exists to serve orbistoun, it stops being a probe of the platform and becomes this
emulator's test harness, and every change in one is a break in the other.

### The frame that avoids it

**The protocol asks questions about the platform, not about either program.** obSCEne
answers by running on the console; orbistoun answers by emulating; a third implementation
could answer too. Under that framing obSCEne does not know orbistoun exists, which is the
point.

The test for whether it has been kept: *would someone with no interest in this emulator
still find obSCEne useful?* A command that exists because orbistoun needs it, rather than
because the platform has it, will be visible in the spec as a command that makes no sense
on real hardware.

### What this side commits to

**obSCEne owns the protocol and the record format.** It is the tool; this is the emulator.
orbistoun implements against whatever is published and has no vote. Asking for a format
that suits a Rust test harness would be the coupling arriving as a favour.

**No shared code, in either direction, ever.** Not a crate, not a C library, not a
submodule. It is a line protocol and each side writes its own parser. Sharing one would
cost more than it saves and would create the build-time dependency this is avoiding.

**The contract is the spec plus captured exchanges.** Implementing against *what obSCEne's
C happens to do* makes it the reference implementation and this a follower - every fix
there becomes a silent break here. Captured sessions are the conformance test for the
protocol itself, and they are what lets this side be built and tested with no hardware
attached.

**Unknown commands are refused, never guessed.** The same rule as everything else here: an
emulator that improvises an answer to a command it does not understand produces a
plausible wrong one, which is worse than a gap. A responder supporting nothing at all is
still a valid responder.

**Transport is separable, and CI never needs a console.** Records are files; files are what
tests depend on. Any socket is opt-in, off by default, and absent from the gate. A check
that needs hardware plugged in is a check that fails for everyone else.

### Reversed on 2026-08-22: orbistoun drives, and this entry was wrong about which side does

This entry said orbistoun is *a responder, never a driver*, that the driver lives in
obSCEne's `tool/`, and that orbistoun never opens a socket. The first two are now wrong and
the third needs qualifying.

**What changed the answer** is `HANDOVER-ORBISTOUN-NET.md`'s point about machine identity: a
probe cannot certify its own machine, so the origin has to be **asserted by the operator**.
That has to be collected where the operator is. obSCEne is a freestanding C probe with no
interface to put a form in; orbistoun has one. Routing the operator through a separate tool
to capture a session and then importing the file back is a worse loop than the interactive
one this whole effort exists to build.

So orbistoun connects, drives the session, collects the assertion, and writes the corpus.
obSCEne's `tool/` remains its own reference driver - two clients speaking one protocol is
what a specification is *for*, and neither is subordinate to the other.

**What survives unchanged, and it is the important half:** CI never opens a socket. The
client is generic over anything that reads and writes bytes, so every test drives it from
memory - and the paths worth testing are the ones where the far end stops answering, which
are unreachable from a happy-path run against real hardware even when hardware is to hand.
`connect()` is the only function in the crate that knows a socket exists, and it is small
enough to have nothing worth testing in it.

Recorded as a reversal rather than quietly widened, because an entry that changes its mind
without saying so is worth less than one that was never written.

### Order: consume before respond

Two jobs, and the smaller one is the more attractive.

**Consuming the corpus** turns recorded answers into fixtures and into the values HLE
functions actually return. That is what makes the emulator better and the reason to own
the hardware at all.

**Answering the protocol** lets one driver diff console against emulator live. It is the
better demonstration and the smaller win, and it only pays off once there is a corpus to
disagree with.

So the consumer first, and the responder when there is something for it to be wrong about.

### A record is one observation

A function returning zero for one set of arguments does not mean it always does. Every
record carries its input, its output, the firmware, and the part that produced it - and
anything measured on a stand-in rather than the console is `assumed`, not `measured`.
Without the producing part recorded, a value measured on one machine and consumed as
authoritative for another is invisible, which is D139 exactly.

