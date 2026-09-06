# 2026-09-03 - (/loop) One writer, thirty-seven readers, and a guard that never passes

```
no code changed   suites 124   tests 2006   clippy/fmt/identity clean
```

Fourth cron tick. A chain of measurements, no fix - the wall moves from "a global is zero" to
"one specific allocation returns null", and every step used an instrument this project already
had and had never pointed at this wall.

## The instruments were already built

`ORBISTOUN_WATCHPOINT` arms x86 debug registers - four, `:w` or `:rw` - and names the
instruction that touched an address. It has been in `orbistoun-worker/src/watchpoint.rs` the
whole time. **D510's lesson applies to instruments too.**

## One write, of zero

```text
ORBISTOUN_WATCHPOINT=0x400001a30610+8:w
  touched after the access at image+0xafff02; it now holds 0x0
```

The bytes there are `vmovups [rip+disp32], ymm0` - one of an unrolled run of 32-byte AVX zero
stores covering `0x1A30600..0x1A3061F`. Bulk-zeroed at start-up, and nothing ever fills it.

## One instruction in the whole binary would fill it

Scanning the executable segment for RIP-relative 64-bit stores targeting `0x1A30610`:

```text
stores: [('0xf1b3cb', 'rcx')]        loads: 37
```

A singleton whose single initialiser never runs its store.

## The guard that fails, and the one that does not

```text
0xf1b3a2  call 0xEEE410
0xf1b3aa  test rax,rax
0xf1b3ad  je   skip              <- fails here: the call answers null
0xf1b3b9  sub  eax,[0x19AB4B0]
0xf1b3bf  cmp  eax,[0x19AB4B4]
0xf1b3c5  jae  skip              <- NOT this one
0xf1b3cb  mov  [0x1A30610],rcx
```

The section-table guard was the first hypothesis and **it is wrong**: a four-byte watchpoint on
the count shows it reaching `0xcb` - 203 sections. An earlier eight-byte watch had caught only
accesses from before that and read as "always zero". Three samples exist for this.

## What the caller is

`0xEEE410` takes a mutex at `this+0xe8` through a PLT stub - an import - and its assertion path
names its own source:

```text
r8d = 0x30                                                        (line 48)
rcx -> ".\PlatformDependent/PS5/Source/Threads/PlatformMutex.cpp"
```

A locked allocator returning null. The next question is what it tried to allocate, not what is
wrong with the global.

## A technique worth keeping

Reading guest code at an arbitrary address needs a vaddr-to-file-offset map the container does
not hand over - and does not need to. **The fault report already prints bytes at a known
address**; searching the file for that exact string gives the offset, and *one* occurrence is
the check that it is right. Segment 0 came out at `+0xbd50`, segment 1 at `+0xc000`.

The second calibration was needed because the first produced strings that were *nearly* right -
fragments starting mid-word. Exactly what an off-by-a-few offset looks like, and exactly what
would have been read as a finding if the strings had been shorter.

## Ruled out, and a note of mine corrected

The fourteen reservation failures at `0x6b0000000000` are **expected**: D443 and D488 already
established that a policy region plants its base into a guest argument, the guest reserves *at*
that base, and the hint fails by design - D488 confirmed it by moving the constant twice.

`docs/ADDRESS_MAP.md` called them "a standing failure nobody has explained". I wrote that
yesterday-in-session without grepping for the constant, **in the document that exists because a
base was chosen without grepping for the constant.** Corrected there.

Decision: [D518](../decisions/D518-the-null-global-has-exactly-one-writer.md).
