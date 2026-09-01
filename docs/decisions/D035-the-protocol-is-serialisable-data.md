# D035 - The protocol is serialisable data, defined separately from its transport

**decided** · 2026-08-19

Shim↔worker messages are serde types in their own module, with the channel (stdio
pipes today) a separate concern. The transport can change without the protocol
moving.

Consequence that shapes the service crate: operations must take and return
serialisable values rather than handing back rich types holding references into
loaded modules. Deciding this before writing the service is the whole point of
having decided it.

