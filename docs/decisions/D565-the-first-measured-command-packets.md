# D565 - The first measured command packets, and a census that contradicts its own report

**Status:** measured
**Date:** 2026-09-04

## What arrived

obSCEne ran the probe set from `docs/HANDOVER-OBSCENE.md` on real hardware - firmware 12.40,
generation 5, **as a native title**, with a submitted frame reaching the display. Section
`166-agc` called the Class B builders and captured what they wrote.

**This is the first command-stream material this project has ever had, and it is `measured`.**

## The encodings

| Builder | Bytes | First dword | Closes? |
|---|--:|---|---|
| `sceAgcCbNop` | 4 | `0xffff1000` | **no** |
| `sceAgcCbReleaseMem` | 32 | `0xc0064900` | yes |
| `sceAgcDcbDmaData` | 28 | `0xc0055000` | yes |
| `sceAgcDcbWaitRegMem` | 56 | three packets | yes |

Read as a header of `type` in bits 30-31, `count` in bits 16-29 and `opcode` in bits 8-15, with a
packet length of `(count + 2)` dwords:

```text
ReleaseMem   0xc0064900  type 3  opcode 0x49  count 6  -> 32 bytes   = extent
DmaData      0xc0055000  type 3  opcode 0x50  count 5  -> 28 bytes   = extent
WaitRegMem   0xc0027904  type 3  opcode 0x79  count 2  -> 16 bytes
      +16    0xc0053c00  type 3  opcode 0x3c  count 5  -> 28 bytes
      +44    0xc0017904  type 3  opcode 0x79  count 1  -> 12 bytes   = 56 total
```

**The arithmetic closes exactly, three times, including a three-packet decomposition that lands on
the extent to the byte.** That is what makes the field split evidence rather than a guess: it was
not cited from anywhere, it was the only reading under which the measured lengths add up.

`sceAgcCbNop` is the exception and is recorded as such - its count field reads `0x3fff`, which
describes no 4-byte packet. Consistent with a no-op whose count is not a body length.

**One builder emitted three packets.** That is the finding a parser has to be built around: a
command-stream reader cannot assume one call is one packet.

`b0 bb ff ee` appears in three of the four bodies and came from no argument the probe supplied.

## The shader call refused, and named a new code

`sceAgcCreateShader` was invoked with the header shape this project had established - magic
`31 32 33 34`, then `0x18`, then a length, at both observed lengths `0xd8` and `0x118`. It
answered **`0x8a6c002f`** and **wrote nothing**; both out-slots came back sixteen zero bytes.

So the recorded header shape is **not sufficient**, which is a real negative result: the wall is
not simply "nobody called it". And `0x8a6c002f` is a fifth measured member of the `0x8a6c` family,
beside `0x8a6c000a`, `0x8a6c0002` and the `0x8a6c003d` the guest tests for.

## The census contradicts the report it is in

The same file records `sceAgcCbNop` as **`absent`** - and 121 other libSceAgc symbols with it -
while section `166-agc` called it and captured four bytes of its output. All five symbols the
Agc section exercised are recorded absent.

The cause is visible two sections up: `900-surface/agc` **skipped** with *"belongs to the other
console generation, so absence is expected rather than a gap"*, on hardware that
`005-generation` had just identified as generation 5 with `gpu = agc`. The gate is inverted for
this library.

**This matters beyond one report.** This morning's ask was written around "472 libSceAgc symbols
recorded absent" and treated that as the blocker. The absent count was not measuring what it
appeared to measure, and a probe result contradicting its own census is the more reliable half.
Reported back rather than worked around - it is obSCEne's to fix.

## What is still open

**Class A is unanswered.** `166-agc/dcb-constructor-audit` passed with `0x0` and named nothing, so
what constructs the Dcb object those nine functions take is still unknown.

`sceAgcCbNop`'s unnamed sibling `0x7d86501b8094ef57` was skipped - *"the loader did not resolve
this symbol for this build"* - which is expected: nothing on either side knows its name.

## What this does not establish

**That the field split is the vendor's.** It is the reading under which three measured lengths
close, which is strong and is not a citation. A different split that happened to agree on these
three would be indistinguishable here.

**Nor what any opcode means.** `0x49`, `0x50`, `0x79` and `0x3c` are numbers that appeared; the
builders' names suggest what they are for, and nothing here confirms it.
