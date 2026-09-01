# 2026-09-01 - (/loop) PPSA21564 boots: the sceLibcMspace allocator family + bcmp

Surveyed the non-PPSA02664 titles for loop-tickable work. PPSA21564 was faulting `write to 0x7fff0001` at
131 calls - the `Unimplemented` placeholder written through as a pointer - and the call right before it was
`sceLibcMspaceMalloc(0x0, 0x48)`. The platform's `sceLibcMspace*` is Doug Lea's `mspace` arena allocator;
unimplemented it answered the placeholder as a 72-byte buffer.

Implemented the family (`Malloc`/`Calloc`/`Realloc`/`Free`) in `orbistoun-libc` by delegating to the heap
this crate already owns and ignoring the arena handle - sound because the guest only ever touches mspace
memory through this same family, so `allocate`'s header and `free` agree regardless of arena. Also
implemented `bcmp` (= `memcmp`; equal-vs-not-equal is all it promises), which was called 16k times and,
unimplemented, read as "always differ".

Result, repeatable across runs: **PPSA21564 went from a 131-call fault to ~550k calls with no fault** - it
now runs to the call budget instead of crashing. Stub rate 7% -> 4%. Next wants: `scePthreadGetthreadid`
(22.5k calls, needs the thread registry) and a few singletons. Recorded D451. `fmt`/`clippy`/tests/knowledge
audit pass for the touched crates; the change is additive (new symbols only), so other titles are
unaffected.
