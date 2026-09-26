# D573 - Address waits follow the umtx contract

**Status:** assumed
**Date:** 2026-09-07

The sync-on-address wait and wake are modelled on FreeBSD `_umtx_op` wait and wake: the wait
compares a 64-bit word under the queue's lock and sleeps only while it holds the expected value;
the wake ends up to the requested number of sleeps and leaves nothing behind when nobody sleeps.
A wait with a non-zero third argument is refused with the placeholder.

**Why:** a wait that returns without waiting is a busy loop, and no return value fixes it.
Comparing under the lock is what stops a wake landing between the read and the sleep. A
remembered wake would end a later, unrelated sleep, and waking more than asked hands one job to
two workers. The third argument may be a timeout in an unestablished unit, so it is refused
rather than read as forever.

**Rejected:**
- Answering success: the guest spins harder and reaches less.
- Reading the word before taking the lock: loses a wake in the gap.
- Treating any timeout argument as infinite: a plausible answer to a question nobody read.
