# D518 - The null global has exactly one writer (and the guard I blamed is never reached)

**measured** - 2026-09-03 (hardware watchpoints, and a mechanical scan of the guest's own code)

No code changed in this decision. It is a chain of measurements narrowing the wall, and it is
written down because every step is reusable and most of it used instruments this project
already had.

**Its conclusion was wrong and is corrected inline below.** The first three steps hold; the
fourth blamed a guard that never runs. D519 has the corrected chain, and the correction is
left in place rather than rewritten away, because how it went wrong is the useful part.

## The instruments were already built

`ORBISTOUN_WATCHPOINT` arms x86 debug registers - up to four, `:w` or `:rw` - and reports the
instruction that touched an address. `ORBISTOUN_WATCH` snapshots a range and reports what
changed. Both existed, both are documented in `crates/orbistoun-worker/src/watchpoint.rs`, and
neither had been used on this wall. The queue is the last place to learn the work was done
(D510), and that applies to instruments as much as to work items.

## Step one: the global is written, once, with zero

```text
ORBISTOUN_WATCHPOINT=0x400001a30610+8:w
  touched after the access at image+0xafff02; it now holds 0x0
```

**One write in the whole run.** The bytes ending there are `c5 fc 11 05 <disp32>` -
`vmovups [rip+disp32], ymm0`, one of an unrolled run of 32-byte AVX zero stores, and the one
that fires covers `0x1A30600..0x1A3061F`. So the slot is bulk-zeroed at start-up and nothing
ever puts a value in it.

## Step two: exactly one instruction in the binary would

Scanning the executable segment for every RIP-relative 64-bit store whose target is
`0x1A30610`:

```text
stores: [('0xf1b3cb', 'rcx')]
loads:  37 of them
```

**One writer, thirty-seven readers.** That is a singleton, and its one initialiser never runs
its store.

## Step three: what guards the store

```text
0xf1b3a2  call 0xEEE410
0xf1b3a7  mov  rcx, rax
0xf1b3aa  test rax, rax
0xf1b3ad  je   0xf1b49f          <- skip if the call answered null
0xf1b3b3  mov  eax,[rcx+0xc]; shr eax,0x15
0xf1b3b9  sub  eax,[0x19AB4B0]
0xf1b3bf  cmp  eax,[0x19AB4B4]
0xf1b3c5  jae  0xf1b49f          <- skip if outside a section table
0xf1b3cb  mov  [0x1A30610], rcx  <- the store
```

The second guard was the first hypothesis and **it is wrong**: a watchpoint on the count shows
it reaching `0xcb` - 203 sections - at `image+0xb13e1a`. An earlier eight-byte watch had caught
only accesses from before that and read as "always zero", which is what taking three samples is
for.

So the failing guard is the first: **`0xEEE410` returns null.**

> **Wrong, and disproved the next tick - see D519.** The guard is never *reached*. The
> instruction two before the call loads the allocator object from a global at `0x1A87210`;
> watching that global shows it holding `0x740001481b00` and read at three sites, **none of
> them `0xf1b39b`**. The recorder holds 32 distinct sites and used 3, so there was room for the
> fourth. The whole function is never entered on this run: its only caller is `0xf269e2`, which
> runs *after* the call that faults. The singleton is null because nothing has initialised it
> yet, and that is correct at that moment.
>
> This is the mistake principle 3 names, made in a decision *about* measurement: two guards were
> read out of the disassembly, one was eliminated, and the other was declared the cause without
> checking that either ran. Elimination is not the same as demonstration.

## Step four: what `0xEEE410` is - which turned out not to matter

It takes a mutex at `this+0xe8` through a PLT stub (`ff 25` - an import), and if that returns
non-zero it reports an assertion whose arguments name the source:

```text
r8d = 0x30                                                        (line 48)
rcx -> ".\PlatformDependent/PS5/Source/Threads/PlatformMutex.cpp"
```

A locked allocator, then. It returns a pointer, and the pointer is null - so the next question
is what it tried to allocate and why that failed, not what is wrong with the global.

## And a technique worth keeping: calibrating file offsets against a known fault

Reading guest code at an arbitrary address needs a virtual-address-to-file-offset map, and this
container does not hand one over. It does not need to: **the fault report already prints bytes
at a known address.** Searching the file for that exact byte string gives the offset, and the
search doubles as the check - one occurrence means the mapping is right.

```text
occurrences of the fault bytes: 1 -> segment 0 file offset = 0xbd50
"flip equeu" at a reported address -> segment 1 file offset = 0xc000
```

The second calibration was needed because the first offset produced strings that were *nearly*
right - fragments starting mid-word, which is exactly what an off-by-a-few offset looks like
and exactly what would have been read as a finding if the strings had been shorter.

## What was ruled out, and the note I had to correct

**The fourteen reservation failures at `0x6b0000000000` are not the wall.** They are
`POLICY_REGION_BASE`, and D443 and D488 already established that a policy region plants its
base into a guest argument, the guest's allocator reserves *at* that base, and the hint fails
by design - D488 confirmed it by moving the constant twice and watching every failure follow.

`docs/ADDRESS_MAP.md` called them "a standing failure nobody has explained". I wrote that,
yesterday in this session, without grepping for the constant - in the document that exists
*because* a base was chosen without grepping for the constant. Corrected there.
