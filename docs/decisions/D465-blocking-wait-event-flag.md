# D465 - sceKernelWaitEventFlag blocks on the event-flag condvar rather than being absent


**measured** - 2026-09-02 (user-directed /loop: overnight oracle-free crunch)

`sceKernelWaitEventFlag` was never implemented. The event-flag family had create, poll, set, clear
and delete, but not the **blocking** wait - so a guest that waits for another thread to set an event
called it against the default stub, read the placeholder as "not ready", and called it again: 304,583
times, 91% of PPSA04263's run, spinning where it should have parked. The spin only became reachable
once spawned threads ran at all (D464); before that the guest never got far enough to wait on one.

The subsystem already had what a blocking wait needs - each `GuestEventFlag` carries a `Condvar` that
`event_flag_set` notifies. `event_flag_wait` (sync) locks the flag's bits, and while the pattern is
unmet waits on that condvar, which releases the bits lock so a setter is never shut out and re-takes
it on wake. `all` selects AND (every bit) over OR (any); the mode's clear bits (`0x10` all, `0x20`
matched) are honoured on success; a NULL timeout waits indefinitely and a microsecond one uses a
deadline so re-checks do not extend it. It answers as `event_flag_poll` does - a bad handle, a
timeout, and the matched pattern told apart - and the kernel shim maps those to ESRCH, ETIMEDOUT and
`OK`-with-the-pattern-written.

**Verified.** PPSA04263 went from 332,914 calls to **28,343** (`-304,571`, exactly the spin), the guest
now parking instead of busy-looping. Blocking is correct even when it does not advance the fault: the
run still ends at the same `int 0x41` guest assert (`image+0x196b91a`), which was there behind the
spin all along and is a separate, deeper problem - a guest deciding to abort on some upstream value,
the same class as PPSA21564's wall. A blocked waiter is released by the time limit ending the process
if its event is never set, so a real deadlock costs the run's clock rather than hanging it.

Marked `assumed` in its knowledge entry, not because the blocking is uncertain - it is the standard
event-flag semantics - but because the mode-bit values and the timeout error code are the documented
SCE/FreeBSD ones rather than measured on this target, and the observed guest waits indefinitely so the
timeout path is unexercised.
