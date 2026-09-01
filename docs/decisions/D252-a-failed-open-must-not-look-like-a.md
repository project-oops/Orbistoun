# D252 - A failed open must not look like a descriptor


**decided** · 2026-08-25 · found by the probe in its first minute of running

`sceKernelOpen` answered `GuestError::InvalidArgument` on failure. Those placeholders
deliberately avoid the high bit so they can never be mistaken for an established firmware
value - and that same choice makes them **small positive integers**, which is exactly what a
valid file descriptor is.

The probe opened a file that was not there, got `0x7fff0002`, and passed it straight to
`sceKernelRead` as a descriptor. Six commercial titles never surfaced this.

A descriptor-returning call now answers `-1`. **Assumed rather than established**: that
failure is reported negatively is the POSIX convention and this kernel is FreeBSD-derived,
which makes it a good assumption and still an assumption. `-1` rather than a specific errno,
because which code actually comes back is a question for the probe on hardware.

**The result was immediate.** With the failure legible as a failure, the probe stopped dying
on its report file and ran **nine conformance sections**, announcing every check by name
before making the call - 990 calls, 774 answered by a real implementation, and a ranked list
of what is missing with counts against each entry. The debug loop went from "PPSA28061 faults
at `image+0x43c4`, nine attempts, three classes eliminated" to "`035-libc` died here, and
`scePthreadMutexDestroy` was wanted seventeen times".

