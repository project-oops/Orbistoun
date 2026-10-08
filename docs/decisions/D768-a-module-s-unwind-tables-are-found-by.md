# D768 - A module's unwind tables are found by address

**Status:** decided
**Date:** 2026-10-08
**known_by:** guest-observed

`sceKernelGetModuleInfoForUnwind(address, flags, info)` fills `info` for the placed module that
holds `address`:

- the file name from `+0x08`
- `.eh_frame_hdr` at `+0x108`
- `.eh_frame` at `+0x110`
- its length at `+0x118`
- the module's first loadable segment, and its size, at `+0x120` and `+0x128`

It answers 0. A block stating less than `0x130` bytes, or an address in no placed module, answers
the placeholder.

Each module's tables come from its `PT_GNU_EH_FRAME` header. The second field of `.eh_frame_hdr`
locates `.eh_frame`; pc-relative and data-relative four-byte encodings and absolute eight-byte ones
are read, and any other encoding is not guessed. `.eh_frame` runs to the index when the index
follows it, as in every module in the corpus, and otherwise to the end of its segment.

`_is_signal_return(address)` answers no. Orbistoun runs a signal handler on its own stack through
a reentrant call, so no guest frame returns through a trampoline.

**Why:** the title's own `libc.prx` unwinder (PPSA02664, `+0x39ae0`) asks by address. It sets
`+0` to `0x130`, calls with flags 1, and on 0 reads `+0x108`, `+0x110` (which must be non-null),
a 32-bit `+0x118` and `+0x120`. That is the layout recorded here. Unanswered, every C++ throw
ended in "Terminating due to uncaught exception".
