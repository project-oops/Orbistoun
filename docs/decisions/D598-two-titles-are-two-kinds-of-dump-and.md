# D598 - Two titles are two kinds of dump, and one names the code it wanted

**Status:** measured, with one section corrected
**Date:** 2026-09-08

## Nine iterations on one title, and the other one is a different object

PPSA02664 reaches furthest of any title here and had not been looked at once this session. Every
tool built for PPSA03416 - the format record, the opens and reads and seeks, the mapping record,
the call sites - was pointed at it, and the first thing it said was new:

```text
path /app0/Media/globalgamemanagers is not considered suitable for apr reads flags:0x0
                                                                    [from image+0xf0fbef]
```

**PPSA02664 asks whether a path is suitable for the asynchronous file path, is told `flags:0x0`,
and declines.** PPSA03416 never says this and goes ahead.

The directories say why:

| | PPSA03416 | PPSA02664 |
|---|---|---|
| `ampr_emu.index` | 16,248 bytes | absent |
| `fakelib/libSceAmpr.sprx` | 218,678 bytes | absent |

**PPSA03416 is a modified dump** carrying a shim that implements the Ampr API in guest code
(D592). **PPSA02664 is a clean one.** They are not two instances of one problem; they are two
different problems that happen to reach the same subsystem.

That settles the scope question D594 raised and left open: serving the shim is compatibility with
somebody else's emulation layer and applies to one title. **Serving what PPSA02664 needs is
implementing the platform**, and no scope question arises.

## The clean title names the code it expected - NO. Corrected below

**This section was wrong and is kept so the mistake is legible.** The instruction bytes below
were read out of a `turn` transcript, and a turn runs the guest under sweeps: those runs faulted
at `image+0xf56e09` reading `0x6608c`, not at the baseline wall `image+0x39f7c`. The comparison
against `0x8a6c003d` is real guest code at a site reached **only under an intervention**.

So this entry did the exact thing principle 3 names - *an intervention that moves a wall is not a
diagnosis* - and did it in a decision entry, which is where it does the most damage. The
baseline bytes are stable across runs and are different:

```text
31 c0                  xor  eax, eax
85 d2                  test edx, edx
74 15                  je   …
48 8b 76 30            mov  rsi, [rsi+0x30]
 ff ca                 dec  edx
8b 14 96               mov  edx, [rsi+rdx*4]   <- faults
```

`rsi` is `0x542e2e00776f6c66` - ASCII, `flow..T` - so the guest walked a structure and got text
where it expected a pointer. What survives from below is only that `rbx` holds `0x7fff0001` at
the baseline fault, which was read from a run under settings rather than interventions.

## What the section below claimed

Its fault is `read of 0xffffffffffffffff` at `image+0x39f7c`, which the report already flags as a
general-protection fault rather than a read of minus one (D384). The registers say more than the
address does:

```text
rbx=0x7fff0001   rsi=0x542e2e00776f6c66
```

`rbx` is **orbistoun's own `Unimplemented` placeholder**, live at the fault. `rsi` is ASCII -
`flow\0..T` - being used as a pointer.

And the instructions before it:

```text
31 c0                  xor  eax, eax
e8 c0 7f 0c ff         call …
81 fb 3d 00 6c 8a      cmp  ebx, 0x8a6c003d
75 0c                  jne  …
```

**The guest compares an error against `0x8a6c003d` and branches.** Our placeholder is
`0x7fff0001`, which does not match, so it takes the branch for a code it does not recognise -
and that branch is what dies.

This is D125's shape with the answer attached: the finding is not merely *a placeholder was used
as a pointer* but *here is the value the guest was looking for*.

## What this does not establish

**Which function should answer `0x8a6c003d`.** The comparison is on `ebx`, set before the call
immediately preceding it, and no import has been tied to it yet. `ORBISTOUN_RETURN` can force any
candidate to that value and the guest grades it, which is a measurement nobody has made.

**Nor what the code means.** `0x8a6c…` is a vendor error space this project has not enumerated.
Answering it because a guest compares against it would be fitting a constant to one branch, which
is worth doing as an *experiment* and not as an implementation.

**Nor that `flags:0x0` is wrong.** PPSA02664 declining the asynchronous path may be the correct
outcome - orbistoun cannot serve it either. What the flags should be is unmeasured, and the
fallback failing is a separate defect from the flags being zero.
