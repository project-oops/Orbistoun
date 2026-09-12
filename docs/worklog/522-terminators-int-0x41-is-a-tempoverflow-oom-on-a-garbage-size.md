# 522. Terminator's int 0x41, precisely: a Unity TempOverflow OOM on a stack address used as a size

**2026-09-12** - reading the guest's own assert message cracked what the int 0x41 is

Worklog 519/521 had Terminator's wall as "a cross-thread async-error assert, cause unknown." Reading
the message the guest formats before the trap sharpens it to something concrete, and corrects the
async-error reading.

## What the message says

The assert `memcpy`s a 776-byte string to a stable guest address (`0x740002540010`); `ORBISTOUN_WATCH`
captured it:

> Could not allocate memory: System out of memory!
> Trying to allocate: 105553124636954B with 16 alignment. MemoryLabel: TempOverflow
> Allocation happened at :Line:543 in
> Memory overview
> [ ALLOC_TEMP_TLS ] used ... reserved: 9109...

So `int 0x41` is Unity's own out-of-memory assert: the temp allocator's overflow path
(`MemoryLabel: TempOverflow`) tried to allocate **105553124636954 bytes** and failed. The
`call [rbp-0x90]; mov rax,[rbp-0x88]; test [rax+0x30],0x10; je; int 0x41` disassembled earlier is the
allocator returning failure and the guest trapping on the recorded error flag - not a cross-thread
error (that reading is retired; the other thread in `sceKernelSyncOnAddressWait` is Unity's job system,
incidental).

## The number is the tell

`105553124636954 = 0x6000_007F_BEDA`. The guest stack lives at `0x600000...` (the mapper's arg0 was
`0x600000800e20`), so this is a **stack address, not a size**. Unity read an **unfilled out-parameter** -
a stack slot still holding a leftover stack pointer - and used it as an allocation size, so the temp
allocator "overflowed" by trying to allocate 96 TiB. The wall is a garbage size, and the garbage is a
stack pointer where a size should have been written.

## What is not yet pinned

Which call left the size out-param unwritten. Ruled out this tick:

- **The APR resolve** of `RuntimeInitializeOnLoads.json` - re-tested the disk-fallback + swept all
  `ORBISTOUN_APR_ANSWER` slot permutations; the ~96 TiB size did not resolve to the file's real size,
  and it is a stack address, not a file size, so the resolve is not the source. Fallback reverted again
  (stays as D679 decided).
- **The app-content temp-data calls** (`sceAppContentTemporaryDataMount2`, the `GetAvailableSpaceKb`
  pair) - dumped their arguments; none passes a `0x6000007fbe**` stack out-pointer, so the unfilled
  size is not obviously theirs either. (`TemporaryDataMount2`'s arg2 points at a Unity error-string
  table - "Invalid info address." - not a size buffer.)

So the source is a size-returning call somewhere in Unity's early memory setup whose out-parameter
orbistoun does not fill, and finding it is a sustained IL2CPP-internals trace (walk back from the
`Line:543` allocation site to whatever supplied its size), not a single obvious missing function.

## State

- No code change (APR fallback added for the sweep, reverted; worker back to committed). Tree green,
  guard clean.
- Terminator's wall is now precisely named - a Unity temp-allocator OOM on a stack-address-as-size - and
  the next step on it is a deep trace of the size's origin, which is a committed RE effort rather than a
  loop tick.
