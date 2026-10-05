# D746 - A compute dispatch binds each traced buffer and writes it back

**Status:** decided
**Date:** 2026-10-05

A compute dispatch's buffers are traced as a draw's are (D733), stores included: every buffer a
`buffer_load_dword*` or `buffer_store_dword*` reaches through a descriptor the dispatch's user data
supplies gets a binding of its own, filled from the guest range the descriptor names and, after the
dispatch, written back to guest memory where it changed - the same all-or-nothing write the window
gets (D711). The window covers only the accesses left untraced. Two bound ranges that overlap each
other or the window are refused, since separate copies of one byte would disagree. A program that
writes any other way - a typed store, an atomic, a global or scratch store - traces nothing and
keeps the single window.

**Why:** the window is one power-of-two span of at most 64 Ki words, and a dispatch's buffers need
not be near each other. radeonsi's buffer copy reads one buffer and writes another wherever its
allocator put them: STKT00001's dispatch at 0xd04 copies four bytes from `0x40a600000` to
`0x412012008`, 128 MiB apart, and was refused.

**Rejected:**
- Several windows: each would carry the window's bounds and placement rules, which a bound buffer
  already replaces with the descriptor's own.
- Binding buffers read-only and storing through the window: a program that reads back what it
  stored would read the snapshot.
