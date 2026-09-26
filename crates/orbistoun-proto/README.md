# orbistoun-proto

The shim-to-worker protocol: messages as data.

It defines `Request`, `Event`, `Outcome`, the ordered `Phase` axis, and the shared wire shapes
(`ImportRecord`, `SurveySummary`, `ContainerInfo`). The shims,
[orbistoun-service](../orbistoun-service/) and [orbistoun-worker](../orbistoun-worker/) all
speak it.

## Rules

- **Protocol is separate from transport.** This crate defines what the shims and the worker
  say, not how it travels. `codec` holds one transport, newline-delimited JSON; changing the
  channel does not move the protocol.
- Newline framing is safe because JSON escapes literal newlines inside strings, so no message
  body contains the delimiter. A test asserts it.
- `Phase` is ordered, so "furthest reached" is a comparison, and a phase regression between
  runs is the clearest signal a report can carry.
- **Nothing borrows.** A compile-time assertion checks it: a message that cannot own its data
  cannot cross a process boundary.
