# D236 - orbistoun answers the probe's protocol, and is a stand-in for itself


**Status:** decided (2026-08-25)

obSCEne speaks a command protocol; orbistoun now answers the same commands. One driver can
be pointed at either and diff the records live, instead of running them separately and
reconciling files afterwards. `docs/BACKLOG.md` filed this as *blocked on nothing but the
protocol*, and the protocol is now stable enough to implement against - a full client and
fifty-two tests were written against it first.

### The seam points one way, deliberately

orbistoun is a **responder, never a driver**. obSCEne owns the verbs, the record format and
the refusal grammar; this implements against whatever they define. That is not deference.
A test harness that also owned the protocol could define a disagreement away, and the
emulator has no business knowing what is on the other end of a comparison.

So no vocabulary is invented here. Every token written is one the reader already parses,
and a round-trip test holds it: sixteen record kinds rendered, re-parsed, and compared.

### It announces only what it serves

Just `report`. `call` and `read` need a guest that is loaded and running, and the responder
holds a service rather than a run - so those capabilities are **not announced**, rather than
announced and then refused. The reply to `hello` is what a driver plans against: a
capability offered and later withheld is worse than one never offered, because by the time
the refusal arrives the driver has already decided the comparison was possible.

What `report` is worth on its own is symbol presence - ninety-five names, of which
sixty-nine have a real handler. "Does the emulator know the names the platform has" is
answerable from one place, live.

### `sym` says linkage; the counts say implementation

A `sym` record's availability field means *how the symbol is reached*, and writing `stub`
there would make every line differ from a probe's on an axis the field does not mean. The
stub/implemented split is the thing this project cares about most, so it is stated plainly
as `sysinfo` counts rather than smuggled into a field that means something else. Whether the
protocol should carry it per symbol is a question for obSCEne, not a field to redefine here.

### The first thing it says is that it is not the platform

`part|<session>|kind|emulator`, unprompted, before anything else. A driver comparing an
answer against a reference has to know which end is which, and this is the one thing this
end can honestly certify about itself - machine identity is operator-asserted everywhere
else in this project precisely because a probe cannot certify its own machine (D225).

**And `orbistoun` joins the stand-in list.** A transcript can now be this emulator's own
account of itself, and grading that as a fact about the platform would be the project
marking its own homework. Verified end to end: a session served, driven by our own client,
read back by our own reader, and refused any grade above `assumed`.

### A socket is opt-in and stays out of every automated path

`orbistoun serve`, never by default. It binds loopback unless told otherwise; `--no-key` is
**refused** on a non-loopback bind, because "I did not want a password" and "anything on
this network may drive this" are separate decisions and only the first one was made. The
session secret is generated once at start and displayed once - per connection would be one
no driver could have presented - and compared without returning early on the first
differing byte.

The tests open no socket at all. The whole exchange runs over a pair of in-memory buffers,
so the gate needs nothing plugged in (D016).
