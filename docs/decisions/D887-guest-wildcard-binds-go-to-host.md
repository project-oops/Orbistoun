# D887 - Guest wildcard binds go to host loopback unless LAN is opted in

**Status:** decided
**Known by:** assumed
**Date:** 2026-10-15

A guest socket bound to "any address" (`INADDR_ANY`, `in6addr_any`) is bound on the host to
loopback in the same family, with the port the guest asked for. `ORBISTOUN_GUEST_LAN=1` binds it
on every host interface instead. A guest that names a specific address is bound to it either way.
An unbound datagram socket that connects binds loopback when its peer is loopback, and any
address otherwise, because a datagram to the network needs an address that routes there.

**Why:** a host firewall asks about every new binary that binds the wildcard, and `ab` runs each
build from a fresh copy under the data root, so every A/B run raised a prompt and no rule keyed
on a binary's path could cover them. Loopback raises none. Nothing a run measures reaches a guest
from another machine; a driver or `pros`-style check on this host still connects.

**Rejected:**
- A firewall rule per binary: the copies are content-addressed and new each run.
- Turning the firewall's prompt off: it changes the host for every program, not this one.
- Always exposing, with loopback opt-in: the default would keep the prompt it exists to remove.

The guest sees loopback from `getsockname` where it bound the wildcard. A guest that compares the
two has not been met; if one is, the host address is reported back as the wildcard.
