# 414. The first command packets came back

**2026-09-04** - directed

## What arrived

obSCEne ran the probe set from `docs/HANDOVER-OBSCENE.md` on hardware - firmware 12.40,
generation 5, **as a native title**, with a submitted frame reaching the display. Section
`166-agc` called the Class B builders and captured what each wrote.

**This is the first command-stream material this project has ever had, and it is `measured`.**

| Builder | Bytes | First dword |
|---|--:|---|
| `sceAgcCbNop` | 4 | `0xffff1000` |
| `sceAgcCbReleaseMem` | 32 | `0xc0064900` |
| `sceAgcDcbDmaData` | 28 | `0xc0055000` |
| `sceAgcDcbWaitRegMem` | 56 | three packets |

## The thing that made it evidence

obSCEne recorded, for each, **how many bytes changed** - measured without reference to any header
field. So a length rule has something to be wrong about.

Reading the header as type/count/opcode with a packet of `(count + 2)` dwords, the arithmetic
closes **exactly** on three of four, including `sceAgcDcbWaitRegMem` decomposing into 16 + 28 + 12
and landing on its measured 56 to the byte. That is what makes the field split evidence rather
than a guess: nothing cited it, it is the only reading under which the measured lengths add up.

**One builder emitted three packets.** A command-stream reader cannot assume one call is one
packet, and that is the shape the parser has to survive.

## The part I did not expect

`orbistoun-gpu/src/packet.rs` already had a walker, built from AMD's public documentation, and its
own module doc carried a caveat and a prescription:

> The values below are **transcribed and not yet verified line by line** [...] a walk over a real
> command buffer that desynchronises immediately is how a mistake here announces itself.

There had never been a real command buffer. There is now, and **the walker consumes three of the
four exactly**. A component that had been waiting on precisely this got its verification, and the
module's caveat is narrowed from *unchecked* to *agreed with hardware on four buffers* - narrowed,
not lifted, because a rule that erred on a packet none of these contains would still pass.

## The shader call refused, and named a new code

`sceAgcCreateShader` was called with the header shape this project had established - magic
`31 32 33 34`, then `0x18`, then a length, at both `0xd8` and `0x118`. It answered **`0x8a6c002f`**
and **wrote nothing**; both out-slots came back sixteen zero bytes.

That is a real negative: the recorded header shape is **not sufficient**, so the wall is not merely
"nobody called it". `0x8a6c002f` is a fifth measured member of the `0x8a6c` family.

## A report that contradicts itself

The same capture records `sceAgcCbNop` as **`absent`**, and 121 other libSceAgc symbols with it,
while section `166-agc` called it and captured four bytes of its output. All five symbols the Agc
section exercised are recorded absent.

The cause is two sections up: `900-surface/agc` **skipped** with *"belongs to the other console
generation"*, on hardware `005-generation` had just called generation 5 with `gpu = agc`.

**This morning's ask was built on "472 libSceAgc symbols recorded absent"** and treated it as the
blocker. That count was not measuring what it appeared to. Reported back rather than worked
around - it is obSCEne's to fix, and a probe result that contradicts its own census is the more
reliable half.

## Guards

Four, each watched failing: the count adjustment dropped (the exact mistake the module's own
comment warns about), the adjustment doubled, the opcode read from the wrong bits, and a
multi-packet walk stopping after the first packet.

**A gate caught a real classification error.** `sceAgcCbNop` was recorded `measured` while
carrying an assumption, and D541's guard refused it: *claims to be measured yet still lists open
questions*. It was right, and the fix was not to soften the provenance - the doubt is about how
**I** read the bytes, which no further hardware run settles, so it is an edge and never was a
question.

## Still open

**Class A.** `166-agc/dcb-constructor-audit` passed naming nothing, so what constructs the Dcb
object those nine functions take is still unknown - the one answer that would open nine at once.
