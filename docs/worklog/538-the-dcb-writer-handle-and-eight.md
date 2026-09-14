# 538. The DCB writer handle, and eight builders wired through it

**2026-09-14** - the effectful half the encoders were waiting for

## The layout was already in hand

Worklog 536 stopped at pure encoders because the writer handle in `arg0` was unmeasured, and placing
a packet without it meant inventing a struct. The request for it (`REQ-...d3cb`) is still open on the
bus - and the answer was already sitting in obSCEne's own source, because its probe **cannot work
without it**:

| offset | field |
|---|---|
| `+0x00` | buffer begin |
| `+0x08` | buffer end |
| `+0x10` | **the write cursor** |
| `+0x18` | the limit a builder checks against |
| `+0x20` | buffer-overflow callback |
| `+0x28` | its context |
| `+0x30` | reserved-dword counter |

Read off PPSA02664's stack, and then confirmed by the library's own behaviour rather than by
argument: obSCEne builds a handle to exactly this shape, and the real builders write correct packets
through it and advance `+0x10` by exactly what each builder's own `GetSize` answered - across sixteen
builders, in three separate runs.

Two offsets are confirmed harder still. Poisoning `+0x20` with `0xCC` makes the library call through
that slot and fault at `0xCCCCCCCCCCCCCCCC`; the same poison at `+0x30` underflows its space check
first. A struct that merely *fits* would not produce those two faults at those two offsets.

**This is a judgement, and worth saying out loud:** d3cb asked for a dump and is still OPEN. Nothing
in this sweep dumps the handle. What is here is a working handle plus behaviour that only that layout
explains, which is stronger evidence than a dump and is not the thing that was requested. Recorded as
`measured` with that reasoning attached, not as though the request had been answered.

## What was wired

Eight builders, each a three-line handler over the encoders from worklog 536:
`DcbEventWrite`, `DcbSetIndexCount`, `DcbSetNumInstances`, `DcbDrawIndexAuto`, `DcbSetIndexBuffer`,
`DcbSetCxRegisterDirect`, `DcbSetUcRegisterDirect`, `CbSetShRegisterRangeDirect`.

`dcb_append` reads the cursor and the limit, refuses anything that will not fit, writes the dwords,
and advances the cursor. Every builder returns `0x200060078` - measured identically in three separate
runs and across every builder called, so a constant rather than an address one process held.

**The overflow path is deliberately not implemented.** The real library calls the callback at `+0x20`
and grows the buffer; this refuses with the loud placeholder instead, because calling a guest
callback from a shim is a mechanism nothing has measured. A refusal that shows up in a trace beats a
plausible write past a guest's buffer.

## Two of the ten are deliberately still not wired

A builder that is right for one input and wrong for the rest is worse than one that answers honestly:

- **`DcbDrawIndex`** - two of its five body dwords were never read back. The size is known; the
  encoding is not.
- **`DcbSetIndexSize`** - measured at exactly one input, `(0, 0)`, producing selector `0x20000243`
  and value `0x400`. Nothing maps any other argument to any other packet, so emitting that packet
  for every call would be a guess wearing a measurement's clothes.

Both want a capture with more argument sets - a probe request, not a deduction.

## Made to fail

Seven tests in `tests/dcb_wiring.rs`, driven through `agc::implementations()` - the same table the
loader dispatches from, so the test exercises the real path rather than a private function.

Moving `dcb::CUR` from `0x10` to `0x28` fails **four of the seven**; reverting restores green. That is
the claim the whole wiring rests on, and it is now one a test would catch.

The two failure cases are tested as first-class behaviour, not afterthoughts: a full buffer is
refused with the cursor unmoved and **nothing written**, and a null handle is refused with the
measured `0x8a6c000a` rather than dereferenced.

## Surprises

**A `!` delimiter in a perl one-liner ate a doc comment.** `s!...!...!` against text containing `//!`
terminates the pattern at the first `//!`. Second self-inflicted shell wound of the day from the same
family as yesterday's truncated heredocs; both times the fix was to stop being clever and address
lines by number.

**The same leading-dash doc trap as `agc_driver.rs`.** A line wrapping onto `- that a handler...`
reads as a Markdown list item, and `-D warnings` rejects it. Worth knowing it is a *recurring* shape
rather than a one-off: prose that wraps onto a dash is a lint error in this workspace.
