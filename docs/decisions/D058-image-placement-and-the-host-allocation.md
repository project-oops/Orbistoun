# D058 - Image placement, and the host allocation granularity

**decided** · 2026-08-19

Loading now reaches **placement**: a container's span is reserved and every loadable
segment is copied to where the guest expects it, with `.bss` zeroed. Verified on a
76 KB module and a 96 MB commercial executable, inside a worker process.

**`.bss` must be zeroed, not merely left alone.** A segment occupies `p_memsz` bytes
while carrying only `p_filesz` in the container, and the guest is entitled to assume
the remainder reads as zero. Leaving it as whatever the allocator returned produces a
guest that works or fails depending on what ran before it - the least debuggable
failure there is.

### Host allocation granularity is not the guest page size

D054 found that Windows *reserves* at 64 KiB granularity. Placement then found the
sharper edge of the same fact: a reservation **base** must be granularity-aligned, not
merely page-aligned. A span rounded to 4 KiB is silently rounded down by
`VirtualAlloc` to the enclosing 64 KiB boundary, which this crate then correctly
refuses as a relocation.

So `orbistoun-mem` now exposes `allocation_granularity()` and placement rounds the span
base to it. The two values coincide on Unix, where `mmap` has no unit coarser than the
page - meaning **code written and tested only on Unix would never notice**, and code
written only against the guest page size fails exclusively on Windows. That asymmetry
is why the constant is named and queried rather than assumed.

Found by running the tests, not by reading the documentation.

