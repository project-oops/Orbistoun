# D026 - Crunch until the queue is dry, then re-plan

**decided** · 2026-08-19

Unattended runs continue as far as they can rather than stopping at a phase
boundary. Consequence for planning: the work queue must be kept **deep enough not to
run dry**, and ordered so that verifiable work is always available - otherwise the
run degrades into hours spent on code that cannot be exercised, which D004 exists to
prevent. Defer unverifiable work rather than filling time with it.

**Refinement on what to assume versus flag.** Assume freely on *implementation* - new
crates, splitting things up, file layout, naming, structure. Those are expected and
are the reason the restructuring is happening now rather than later. What warrants
stopping to ask is a **new concept** that is not already in this log: a mechanism, a
user-visible behaviour, or a subsystem nobody has agreed to. Adding a crate is not a
new concept; adding a plugin system is.

