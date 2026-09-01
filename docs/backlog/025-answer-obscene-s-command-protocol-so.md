# Answer obSCEne's command protocol, so one driver can diff hardware against emulator


obSCEne is the conformance probe and it speaks a command protocol: a driver sends a
command, the probe runs it and sends the record back. If **orbistoun answers the same
commands**, one driver can point at either the real hardware or this emulator and compare
live, rather than running them separately and diffing files afterwards.

Second, not first. It is the smaller win: live diffing is convenient, but it only pays
off once there is a corpus to disagree with, and the corpus is what improves the emulator.

D056's remote-controlled mode is the capability this belongs to, and it is recorded there
as outranking most of the roadmap - because it does not work around the absence of a
specification, it removes it.

**Direction of the seam matters.** orbistoun is a *responder*, never a driver. The driver
lives in obSCEne's `tool/`, obSCEne owns the protocol and the record format, and orbistoun
implements against whatever they define - it is the emulator, not the test harness, and it
has no business knowing what hardware is on the other end.

**Off by default, and never in CI.** A responder opens a socket, which is exactly what this
crate should not be doing unasked. Opt-in at run time, and the CI path stays files-only:
a gate that needs the hardware plugged in is a gate that fails for everyone else.

**Done** (D236). `orbistoun serve` answers `hello`, `report` and `bye`; `call` and `read`
are implemented in the responder and left unannounced until there is a loaded guest behind
them, because a capability offered and then refused misleads a driver worse than one never
offered. Verified end to end against our own client and read back by our own reader.

