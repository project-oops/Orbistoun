# D559 - The Agc argument classes, and how to get past the arity gate without guessing

**Status:** measured
**Date:** 2026-09-04

## The situation

libSceAgc became reachable on hardware. The question stopped being *can we measure Agc* and became
*what exactly should a probe call, and which calls are safe*.

Two things stood in the way, and neither was the hardware.

## The gate

obSCEne's D008 forbids calling a function whose arity is uncertain - a wrong one corrupts the
stack and surfaces far from the call. Its standing test for satisfying that, D107, is **two
independent open reimplementations agreeing**, which is how the Gnm command builders were cleared.

**That standard cannot be met for Agc.** shadPS4 and GPCS4 are PS4/Gnm projects; there is no
mature open Agc reimplementation for a second source to agree with. Read literally, the gate
blocks the entire ask, permanently, for the one library that matters most.

## The way past it, which is not a weakening

**In System V AMD64 the first six integer arguments are passed in registers, and a callee ignores
register arguments it does not take.** A call that sets all six to individually safe values is
therefore safe for any arity up to six, whichever it turns out to be - the stack is never
involved, and the stack is what D008 exists to protect.

So the risk was never really *arity*. It is **value**: a pointer parameter handed a non-pointer,
or a size parameter handed a huge number. That is a different problem, and unlike arity it can be
bounded by measurement rather than by citation.

## What was measured

A run with a 4KB region planted at `*arg0` of `sceAgcCreateShader` takes PPSA02664 past the wall -
**197 imports to 215, one shader created to 33** - and into the command-buffer layer. Reading the
`arg0` the guest passes to each of the twenty Agc functions it then calls sorts them into four
classes, and the three distinct pointer values are the finding: functions sharing a value are
taking the same object.

| Class | `arg0` | Functions | Probeable standalone? |
|---|---|--:|---|
| **B** writer struct on the caller's stack | `0x6000007fbe38` | 5 | **Yes** - construct it |
| **A** a Dcb object in the **guest's own heap** | `0x740002447868` | 9 | Yes - corrected below |
| **C** handle from an earlier call | `0x7fff0001` / `0x140081c3bf21` | 7 | **No** |
| **D** static descriptor in the guest's image | `0x4000019c9620` | 1 | Yes, zeroed block |

**Class B is the entry point.** The guest's stack at that address holds
`{begin = 0x6000007fbe70, end = 0x6000007fc270, begin, end}` - a 0x400-byte command buffer with a
begin/end pair 0x38 in front of it. A probe can build that from nothing, which makes five
functions - `sceAgcCbNop`, `sceAgcCbReleaseMem`, `sceAgcDcbDmaData`, `sceAgcDcbWaitRegMem` and one
unnamed - safe to call today, under exactly the `gnm.c` pattern.

**Class C is the one worth naming loudly.** Four of those seven receive `0x7fff0001` in `arg0` -
**orbistoun's own placeholder**. The guest is feeding them the return value of a call nothing
implements. A probe that fabricated a handle for them would be manufacturing precisely the value
risk the safety argument above does *not* cover, so they are excluded until their producer is
identified. They come free once A and B are answered.

## The decision

Ask for Class B first, `sceAgcCbNop` as the control, under the six-safe-registers rule; ask for the
Class A constructor to be *identified* rather than for Class A to be called; refuse Class C for
now and say why. Written up in `docs/HANDOVER-OBSCENE.md` with the counts, the pointer values and
what each unblocks.

## Correction, same day: Class A was never library-owned

**`0x7400_0000_0000` is orbistoun's own fixed-base heap** (`docs/ADDRESS_MAP.md`), so
`0x740002447868` is memory the **guest allocated**, not something libSceAgc handed back. This
entry called it "a library-owned Dcb handle" and asked hardware to find the constructor that
issues it. There is probably no such constructor.

What there is instead is an **initialiser taking a caller-allocated block**, and one candidate
stands out: **`sceAgcDcbResetQueue` is called on that exact pointer**, twice, before every other
use of it. "Reset queue" on a fresh block is what an initialiser looks like.

That moves all nine functions from Class A to **Class B** - a caller-owned buffer is the shape
obSCEne already probes successfully four times over (D565). The ask stops being "find a
constructor" and becomes "call `sceAgcDcbResetQueue` on a zeroed block and dump what it writes",
which needs no new technique.

**The mistake was reading an address without checking the map.** `0x740002447868` looked like a
handle because it was large and opaque; one line of `ADDRESS_MAP.md` says it is the heap. The map
is gated against the source precisely so it cannot go stale (D513), and it was right there.

## What this does not establish

**That any of these arities is known** - the argument is that it does not need to be, which is a
different claim and a weaker one. A function taking more than six integer arguments, or a variadic
one, is still outside the rule; nothing here shows that none of these is either, only that the
common case is covered.

**Nor that the class boundaries are real rather than incidental.** Two functions sharing a pointer
value in one run of one title is strong evidence they take the same object and is not proof. A
second title calling the same functions with a different sharing pattern would refute it, and no
second title in the corpus reaches this code.

**Nor that Class B's struct is `{begin, end}`.** That is the obvious reading of two pointers 0x400
apart with the buffer immediately behind them, and the repeat suggests a cursor that has not moved
yet - but nothing observed writes through it, because nothing here implements these functions.
