# orbistoun-proto

The shim-to-worker protocol: messages as data.

**Models:** `Request`, `Event`, `Outcome`, the ordered `Phase` axis, and the shared
wire shapes (`ImportRecord`, `SurveySummary`, `ContainerInfo`).

**Deliberately fakes:** nothing.

**Design note.** This crate defines what the shims and the worker *say*, not how it
travels. `codec` holds one transport - newline-delimited JSON - and is deliberately
separable; changing the channel should not move the protocol.

Newline framing is safe because JSON escapes literal newlines inside strings, so no
message body can contain the delimiter. That property is asserted rather than assumed,
since the whole framing rests on it.

`Phase` is ordered so "furthest reached" is a comparison - a phase regression between
runs is the clearest "that change made it worse" signal a report can carry.

Nothing here borrows: a compile-time assertion checks it, because a message that
cannot own its data cannot cross a process boundary at all.

**Status:** complete for the current design.
