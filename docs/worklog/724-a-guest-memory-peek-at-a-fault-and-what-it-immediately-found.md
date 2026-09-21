# 724. A guest-memory peek at a fault, and what it immediately found

**2026-09-20** — built the tool the wall needed and it paid for itself on the first run. PPSA02664
(Alex Kidd in Miracle World)'s `CreateWorkload` fault is in a memcpy whose source pointer is null,
and worklog 719 could not find where that pointer should come from because the code that sets it up
sits in a runtime mapping the ELF on disk does not locate - no static disassembly of the file
reaches it. `ORBISTOUN_PEEK` reads it out of the live process instead.

## The tool

`ORBISTOUN_PEEK` (a diagnostic, `Effect::Observes` - it only reads) hex-dumps a window of guest
memory at a fault:

- `caller` - the window ending at the faulting call site, and
- `<addr>[+len]` - a fixed range.

It runs in the **post-message, allocating** part of the fault reporter (`emit`), after the no-alloc
line is already out of the door - the same place the call-trace persistence runs, for the same
reason: the allocator is not what broke, and the alternative is not reading the one thing worth
reading. Each window is `readable`-checked with `VirtualQuery` (no dereference) and clamped to its
page, so a wild address yields "unreadable" rather than a second fault. Rows are address-prefixed so
they feed straight into a disassembler.

**One lesson worth keeping:** `caller` first used a frame-pointer walk and landed in *data* - this
guest is built without frame pointers, so `rbp` is not a frame link. The reliable source is
`orbistoun_thunk::last_call().from`, the return address the dispatch **records** at the call site
(the field whose doc already says it "matches the addresses a fault's frame walk reports"). That is
what `caller` uses.

## What it found, first run

`ORBISTOUN_PEEK=caller` dumped the loaded bytes at the memcpy return address (`0x400000042ebd`) - and
they are **completely different from the static file-offset disassembly worklog 719-720 relied on**,
which was wrong because the wrapper decode shifts where the bytes land. The real loop, disassembled:

```
0x…42e0e  mov  r13, [rbx+8]        ; r13 = a field of the loop object
0x…42e1c  mov  rsi, [r13+0x18]     ; r13 is a container: +0x10/+0x18 begin/end, +0x30 a count,
0x…42e45  call [r13+0x20]          ;   +0x20 a vtable/function pointer - a std::vector-like object
0x…42ea7  mov  rsi, r13            ; memcpy source = r13
0x…42eaa  mov  rdx, r12            ; count
0x…42eb8  call memcpy              ; <-- faults: rsi/r13 = 0
```

So the null the run dies on is a **null container pointer** (`r13`), reached through the loop object
in `rbx`. The memcpy is copying that container's contents (`(end - begin) / 4` elements), and the
container is null because nothing populated the field it comes from - the tail of the same
inline-helper chain worklog 723 established (six non-exports, all to be synthesised).

## Why this matters

Two things. The tool is general - any future fault in JIT'd or runtime-mapped guest code (Unity's
IL2CPP output, a loaded PRX in the mapping arena) is now readable and disassemblable, where before it
was a bare address. And it moved this specific wall off the spot worklog 719 was stuck on: the source
is not a caller-supplied argument (which is why arg-poisoning found nothing), it is a **field of the
loop object `rbx`**, populated - on hardware - by the graphics setup this build emits out-of-line.
The next step is to trace `rbx` back to where that field is written, which the tool now makes a
reading rather than a guess.

## Gate state

`orbistoun-env` gains `ORBISTOUN_PEEK`; `orbistoun-worker`'s fault reporter reads and dumps.
`./bin/orbistoun check` run to green (below); the dump path is `#[cfg(windows)]`, matching the rest
of the fault reporter, so it is not dead code on the CI platforms. Identity scan clean. No commit.
