# 415. The constructor was the wrong question

**2026-09-04** - directed

## What was asked for, and what was wrong with it

The first handover asked obSCEne to **find what constructs the Dcb object** that nine `libSceAgc`
functions take, and called `0x740002447868` "a library-owned handle". A hardware run came back
having audited for one and found nothing.

**It was the wrong question, and this repository had the answer already.**
`0x7400_0000_0000` is orbistoun's own fixed-base heap. It is one line of `docs/ADDRESS_MAP.md` -
a file gated against the source precisely so it cannot go stale (D513) - and I did not look. The
address is memory **the guest allocated itself**.

So there is no library-owned handle and probably no allocating constructor. There is an
**initialiser taking a caller-allocated block**, which moves all nine functions from Class A to
**Class B** - the shape obSCEne has already proved four times over.

## The candidate, which was in the data all along

**`sceAgcDcbResetQueue`.** PPSA02664 calls it on that exact pointer, twice, before every other use
of the object. "Reset queue" on a freshly allocated block is what an initialiser looks like, and
it is the only observed call that could be one.

The ask is now the recipe that already worked - poison a block, pass it, dump the delta - with no
new technique needed. A structure written means nine functions open at once. Nothing written and
an error is equally useful: it says the object is built somewhere unobserved.

## The mistake, named

**An address was read as a handle because it was large and opaque.** The map that says otherwise
is generated, gated and one grep away. This is the same class as D555 (a path read from the wrong
place) and D547 - trusting a reading of a number over the file that defines it - and it is now
three for the day.

The cost was real but small: one hardware section that audited for something that does not exist.
The saving is that the corrected ask needs no new probe code.

## Also reported back

The capture contradicts itself - `sceAgcCbNop` recorded `absent` while section `166-agc` called it
and captured its output, with 121 other libSceAgc symbols alongside. `900-surface/agc` skipped as
*"belongs to the other console generation"* on hardware just identified as generation 5, `gpu =
agc`.

That is worth more than the constructor ask: the **first handover was built on "472 symbols
absent"** and treated it as the blocker on the whole Agc axis. A working export census would also
answer the constructor question properly - whether any constructor-shaped name exists at all is
something no amount of calling can establish.

Sent to the obSCEne session and written into `docs/HANDOVER-OBSCENE.md`. D559's Class A table is
corrected in place, with the reasoning, rather than left to be read as it was written.
