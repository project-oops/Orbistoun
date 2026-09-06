# 2026-09-04 - (/loop) The export was already decoded, and its blocker had half expired

```
G11 step (a) was done and nothing said so; exp decodes to a target and four registers
the BLOCKED entry's first clause expired two ticks ago - because of my own change
suites 134   clippy/fmt/identity clean on both repos
```

Thirty-seventh cron tick. No guest binary (checked, one command). The plan named G11 step (a) -
*solve the export's operand layout by probe* - as next. It has been done for some time.

## Checked by decoding, not by reading the table

The table having an EXP entry is one claim; the decoder reading it is another, and the second is
what matters. `0xF8000000 | (target << 4)` with second word `0x03020100` decodes to
`Immediate(target)` plus four vector registers, for targets 0, 1 and 12, with `operands_decoded`
true.

**Nothing asserted it**, which is how it stayed on a list of next steps.
`crates/orbistoun-shader/tests/export_decode.rs` now does - the missing assertion, not a new
capability.

Broken twice. Deleting the EXP entry fails on `operands_decoded`. Moving the target field one bit
across leaves **target 0 still decoding as zero** and fails only on target 1: a test using `mrt0`
alone would have passed a wrong field position. Three targets for the same reason the framebuffer
harness checks every pixel.

## The blocker had half expired

`BLOCKED`'s entry for `exp` said exporting *"needs a render target to export to, and there is no
concept of one yet"*. There is one, as of two ticks ago - a colour attachment, a render pass, a
graphics pipeline, and a `Location 0` output reaching attachment zero, proved on a device by
`orbistoun-gpu-vulkan/tests/draw.rs`.

`BLOCKED` exists to separate "nobody has looked at this" from "waiting on a subsystem", because
those rank differently. An entry claiming a whole missing subsystem, when what is missing is one
execution model, ranks itself far too heavily. Rewritten to the one thing still true: **every
module this translator emits is a compute dispatch, so an export has nowhere to go inside it.**
The clause that survives is the important one - mapping the export onto the storage buffer would
still be inventing a destination.

## Three stale blockers in three ticks, and this is the sharpest

D543's was wrong about its mechanism; D546's was right about its mechanism and in the wrong list.
This one expired because **the tick that invalidated it was my own, two ticks earlier**, and
nothing connected them. Check 13 asks for a gap's words to be searched when the gap closes - at
that moment, not two ticks later when something walks past.

Roadmap step (a) rewritten (check 13).

Decision: [D551](../decisions/D551-the-export-was-already-decoded.md).
