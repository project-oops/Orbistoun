# Answering Prosperous


See **[PAYLOADS.md](../PAYLOADS.md)** - what it would take for `pros check` pointed at orbistoun
to report the same five services a jailbroken hardware does, staged, with the function counts
measured rather than estimated.

The short version: `pros check` is a TCP connect, so a service reads as up the moment the
guest is listening; the four payload servers need 100 imports between them of which **one**
is vendor-specific; and the only research problem is the entry handoff structure, which is
one structure shared by all five (D308).
