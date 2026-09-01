# D273 - A count, an offset and a descriptor are read as data, not tested against a table


**decided** · 2026-08-25 · the fourth instance of one shape in a day

`sceKernelOpen` answered `GuestError::InvalidArgument` for a null path. `sceKernelRead`
answered `InvalidHandle` for a bad descriptor. `sceKernelLseek` did the same.

All three return **values a caller uses directly** - a descriptor, a byte count, a file
offset - and a `GuestError` placeholder is a small positive integer. So the probe reported,
in its own words: a null path *opened successfully*, and reading and seeking an invalid
descriptor *reported success*.

One `FAILED_DESCRIPTOR` now covers them, negative so a caller testing `< 0` sees it.

**Assumed, not established**: that failure is reported negatively. It is the POSIX
convention and this kernel is FreeBSD-derived, which makes it a good assumption and still an
assumption. `-1` rather than a specific errno, because which code comes back is a question
for hardware.

Counting today: a failed open looking like a descriptor, two booleans reading as true
(D271), and now a count and an offset. The placeholders were designed to be unmistakable in
a *trace*, and the property that achieves that - staying below the high bit - is exactly
what makes them mistakable in a *register*. That tension is worth stating plainly rather
than fixing one call at a time.

