# D431 - The C-runtime threading family, and a metric that fell as it was fixed


**measured** - 2026-09-01 (user-directed)

The family the C++ standard library's threading rests on, built as one unit: `_Mtx_init`, `_Mtx_lock`,
`_Mtx_unlock`, `_Mtx_trylock`, `_Mtx_destroy`; `_Cnd_init`, `_Cnd_wait`, `_Cnd_timedwait`, `_Cnd_signal`,
`_Cnd_broadcast`, `_Cnd_destroy`; `_Xtime_get_ticks`; `_Thrd_sleep` - the same `std`-runtime layer as
`_Execute_once` (D430), reached when a guest constructs a `std::mutex` or a `std::condition_variable`
during static init. Each maps onto the honest primitives already in `sync` (the `GuestMutex` /
`GuestCond` the POSIX `scePthreadMutex*` calls use), so the mutual exclusion is real, not a
success-returning stub. Declared in `orbistoun-libc`, implemented in `orbistoun-kernel` beside `sync`,
listed in the declared-elsewhere exceptions - the D367 split, as with `_Execute_once`.

Why it was worth doing even though it did not move the wall: stubbed, each answered the `Unimplemented`
placeholder `0x7fff0001`, and the standard library reads their return as a `_Thrd_result` and *throws*
a non-success one - `_Throw_C_error(0x7fff0001)`, a placeholder turned into an exception the guest
cannot unwind (the D125 shape). Implemented, `_Throw_C_error`, `_Mtx_*`, `_Cnd_*`, `_Xtime` and
`_Thrd_sleep` all left the unimplemented-imports list at once, and the throw with them. Any title that
gets past the current memory wall and uses a std mutex would have hit that throw; this removes it ahead
of the need.

**The honest part: the metric fell.** PPSA25872 went 20→19 distinct imports (verdict BACK) even as this
was a correct fix, because the placeholder-throw was a *side path* that called imports of its own (the
exception machinery) before dying, and removing it stopped those calls. The fault PC did not move -
`image+0x1668a51`, the same `sceKernelMapDirectMemory` / `sceKernelAllocateMainDirectMemory` null-write
as before - which is the tell that the throw was never on the path to this wall. So the family is
recorded as *correct infrastructure that does not advance these two titles*, not as progress: an import
count is not a diagnosis, and a fix that lowers it is still a fix (principle 3).

Two decisions inside it, both `assumed`: every std mutex is created `Recursion::Allowed` rather than
distinguished by the type word, so a same-thread re-entry (undefined in a correct program) cannot raise
a false deadlock before the runtime's type bits are measured the way the POSIX ones were; and
`_Thrd_sleep`'s span is clamped to one second, so an absolute-versus-relative convention mix-up cannot
turn a retry-sleep into a multi-year hang. Both are in the *Needs review* index.

**The next wall needs the guest's instructions, and the format hides them.** `image+0x1668a51` is a
null write (rax/rcx zero) in wrapper-encoded libc code, past `sceKernelVirtualQuery` →
`sceKernelAllocateMainDirectMemory(0x4000)` → `sceKernelMapDirectMemory`. The eboot is a SELF/eboot
wrapper, so the ELF `p_offset` fields do not locate the loaded bytes - only the loader, which decodes
the wrapper, knows the mapping. Rather than a `dump`/`disasm` verb, the fault report itself grew the
capability (D432): it now carries the bytes at the fault and the window before it, straight from the
loaded image. That cracked this wall's shape - see D432.

