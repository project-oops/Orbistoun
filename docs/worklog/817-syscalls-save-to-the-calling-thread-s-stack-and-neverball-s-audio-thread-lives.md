# 817. Syscalls save their registers to the calling thread's own stack, not one shared buffer — the race that nulled libvorbis's table is gone, and Neverball's audio thread streams the title music

**2026-09-24** — worklog 816 left Neverball faulting on a write to `0x0` at `image+0x25f06a`, on its
second guest thread. The next run with the mapping trace on did not fault at all.

## What the fault was

- **Where**: the instruction is `movss [r13 + r14*4 - 4], xmm0` in a loop that converts `cos` results
  to floats. The port's linker map puts `image+0x25f06a` in libvorbis's `mdct_init`, which `malloc`s its
  twiddle table and fills it. `r13`, the table, was null: `malloc` had failed.
- **Why `malloc` failed**: the SDK serves anything over 4 KiB with its own anonymous `mmap`
  (syscall 477), and the faulting run's report carried the refusal — *1 reservation failed … base=0x10000
  len=0x600000801000*. That length is a guest **stack address**, not a size. The `mmap` had been handed
  another call's registers.
- **Whose registers**: the syscall gadget saved every call's number and six arguments into **one fixed
  buffer** (`mov r11, imm64; mov [r11+…], reg`) and passed that buffer to the dispatcher. Its own doc
  comment named the limit — *"Two guest threads issuing syscalls at the same instant would overwrite
  each other's registers. Nothing measured does it … a per-thread buffer is the fix when something
  does."* Neverball is the something: its main thread loads data while its SDL audio thread
  (`SDLAudioP1`) opens the music and allocates vorbis tables, both through the gadget. When their calls
  overlapped, one thread's `mmap` read the other's registers. It happened on some runs and not others,
  which is this race's signature.

## The fix

The gadget now carves its save area out of **the calling thread's own stack**: after the pushes a
syscall preserves and the `and rsp, -16` alignment (D384), `sub rsp, 64` reserves the frame, the number
and the six argument registers are stored `[rsp+0..48]`, and `mov rdi, rsp` hands that address to the
dispatcher. The only absolute address left in the gadget is the dispatcher's. `syscall_gadget` refuses a
dispatcher that wants more words than the frame holds (`SYSCALL_SAVED_WORDS`, 7 — orbistoun-thunk's
`SAVED`). The encoding was checked instruction by instruction with capstone; the byte-for-byte test is
updated to it, and a new test, `the_gadget_saves_through_the_callers_stack_and_names_no_buffer`, pins the
property: one `imm64` (the dispatcher), every save through `[rsp+disp]`.

## What Neverball does now

Three runs, three the same — no fault, no failed reservation:

```
run 1: 0 faults | imports 49, 53141 calls
run 2: 0 faults | imports 49, 53255 calls
run 3: 0 faults | imports 49, 53250 calls
threads  SDLAudioP1  last called libSceAudioOut::sceAudioOutOutput
         main        last called libkernel::sceKernelUsleep
```

Both threads live. The audio thread decodes and streams the title-screen music through
`sceAudioOutOutput` for the whole run. The main thread passes the GL clear self-test and then waits on
the fence of its first frame's submission, which stops at `DRAW_INDEX_AUTO` (D710) — the wall that is now
shader execution at submit (`36c0`). The render step drove that submission to the backend afterwards:
3 commands carried out, **450 refused**, which sizes what translation still has to cover for this guest.
The cube is unchanged (clear self-test passes, 21 imports).

The import count reads 49 against 52 in worklog 816's lucky run: a run that happened not to race and
reached a few more calls before the first fence, not a regression.

## Gate state

`crates/orbistoun-abi/src/enter.rs`: `syscall_gadget_code` (stack frame, no buffer), `syscall_gadget`
(the words check), `SYSCALL_SAVED_WORDS`, the tests. `orbistoun-abi` 28 tests pass. Generated docs
regenerated after the runs. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean.
No commit.
