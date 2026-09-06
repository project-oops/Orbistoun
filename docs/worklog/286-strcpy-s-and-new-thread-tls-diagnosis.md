# 2026-09-02 - (/loop) strcpy_s implemented; new-thread TLS gap diagnosed (the real wall)

Chased PPSA04263's post-collision wall (`image+0x2ba47bc`, 10,123 calls in). Two things.

**strcpy_s (implemented, honest stub-reduction, did NOT crack the wall).** It was flagged
unimplemented and called 102 times copying build strings ("Jan 14 2026", "00:48:41"), a plausible
culprit. Implemented it: Annex K bounds-checked copy, answers `errno_t` (0 success, `EINVAL`/
`ERANGE` otherwise), empties a usable dest on a constraint violation. Its answer is a **status**,
which is the point - stubbed, it returned a placeholder a caller reads as failure. `known_by =
published`, knowledge recorded, clippy/fmt/audit clean. But the run was byte-identical afterward
(same 10,123 calls, same wall), so the guest does not depend on the copies to reach the wall.
Stub reduction, not a crack - recorded honestly.

**The real wall is a new-thread TLS gap.** Decoding the faulting bytes
(`64 48 8b 04 25 00 00 00 00`) gives `mov rax, fs:[0]` - a thread-pointer read via the FS segment
with the base at **zero**, so it reads address 0. The reporter's "null base is r9/r13" guess is
wrong here: the access is segment-relative, which the register heuristic cannot model and cannot
decode without disassembling the guest (so it stays a guess, not a fix). The fault `rsp` is
`0x6100…`, which is `THREAD_STACK_BASE` - a **spawned** guest thread, not the main one.

Root, pinned to the line: Windows zeroes a user-set FS base on the next context switch (D433), and
the `tls_backstop` restores it only for a thread whose `GUEST_TP` was set. The main thread gets the
full setup - `install_main_thread_tls` (worker `lib.rs:2032`) builds a TLS block, installs the FS
base, and calls `tls_backstop::remember`. **A spawned thread gets none of it**: the spawn body
(`orbistoun-kernel/src/thread.rs:433`) reserves a stack and enters guest code, and never builds a
per-thread TLS block, installs a base, or remembers one. So its first `fs:[0]` reads zero and the
backstop declines (`GUEST_TP == 0`). New guest threads have no thread-local storage at all.

**Fix design for next tick (deps all confirmed feasible: worker -> kernel -> abi/loader; `TlsLayout`
is `Copy`):** an inversion hook, exactly the call-budget shape (D238).
1. `orbistoun-kernel/thread.rs`: add `static ON_THREAD_START: OnceLock<fn()>` + `pub fn
   install_thread_start(fn)`; the spawn body calls it after `become_thread`/`note_this_stack`,
   before `enter_guest`, so it runs on the new thread.
2. `orbistoun-worker`: extract `build_tls_block(base, &TlsLayout, &[u8]) -> Result<u64,String>` from
   `install_main_thread_tls` (reserve, `render_block`, `thread_pointer::install`, `remember`); store
   the template `(TlsLayout, tdata: Vec<u8>)` in a `OnceLock` when the main thread sets up; add a
   per-thread `THREAD_TLS_BASE` arena (e.g. `0x6A00_0000_0000`, clear of stacks `0x6100`, reentrant
   `0x6800`, main TLS `0x6900`) with an atomic counter stepped by the block size + a gap; add
   `fn set_up_this_threads_tls()` (plain `fn`, reads the stored template, builds a block at the next
   base) and register it via `orbistoun_kernel::thread::install_thread_start` during setup.
3. Verify: PPSA04263 should pass `0x2ba47bc`. Blast radius likely - any threaded title hits this.
   High value; threading is fundamental.

Also newly visible and trivial when convenient: `scePthreadAttrSet{inheritsched,schedpolicy,affinity,
guardsize}` (each 1 call, advisory attr setters) - accept-and-OK stub reduction, not wall-movers.
