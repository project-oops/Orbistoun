# orbistoun drives the session now, and D207 said it never would


`orbistoun session <host:port> --out <file>` connects to a listening probe, negotiates,
runs what it announced, and writes the transcript. `orbistoun probe <file>` reads it back.
The live path and the file path go through one parser, so they cannot drift into two
different truths.

D207 said orbistoun was a responder and never a driver. That is reversed, and recorded as a
reversal: machine identity must be operator-asserted, and the operator is sitting in front
of orbistoun, not in front of a freestanding C probe with no interface.

**What survives is the half that mattered.** `Client` is generic over anything that reads
and writes bytes, so all nine of its tests drive it from memory and CI never opens a socket.
`connect()` is the only function that knows a socket exists.

### Surprises

**Every interesting test is a failure path, and none of them is reachable against real
hardware.** Acknowledged-then-cut-off is a death; closed-before-acknowledgement is `lost`
rather than a death, because without an acknowledgement nothing establishes the command ran
at all; silence within the budget is a timeout and deliberately not resolved into either. A
client exercised only against a working console would have none of these tested, and they
are the normal case rather than the exceptional one.

**The capability check belongs on the client side, and that is not obvious.** Refusing to
send an un-announced verb looks like duplicated validation - the probe will refuse it
anyway. But a client that waits to be refused has already put a command on the wire that
this probe does not implement, and on a target that faults easily that is not free. The
test asserts the wire stayed clean, not merely that an error came back.

**A doc comment merged into its neighbour for the third time today.** Inserting an enum
variant before `Probe {` put it between `Probe`'s doc comment and `Probe`, so `--help`
printed three commands' prose under one. Fixing that pushed the same collision one variant
further up into `Shaders`. The rule earned twice over now: anchor on the item line, never on
the prose above it.


