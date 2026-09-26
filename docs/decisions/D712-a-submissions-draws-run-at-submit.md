# D712 - A submission's draws run at submit

**Status:** assumed
**Date:** 2026-09-24

A submission's draws run on the device at submit, together, and their result becomes the guest's
colour target before the command processor passes them as done. They run only when every packet
from the first draw to the last is a draw or memory-inert, into one target of one base and
extent in a tiling and format that write back exactly, starting from what guest memory holds;
anything else leaves them unexecuted. The executor runs on a host thread of its own.

**Why:** a fence may stand only for work that ran (D705), and a GL frame loop waits on the fence
after its draws. The guest reads and composes over its target, so a frame kept only on the host
is one it never drew, and all or nothing keeps a partial frame from counting. The host's
exception dispatch cannot walk a guest stack, so device work runs off it.

**Rejected:**
- Writing the fence and rendering later: tells the guest its pixels exist.
- Each draw at its own packet: needs per-draw rendering with carried state, and the draws seen
  so far are contiguous.
- No write-back: the guest reads its own target.
