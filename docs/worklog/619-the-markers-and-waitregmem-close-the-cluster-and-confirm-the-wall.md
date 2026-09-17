# 619. The markers and WaitRegMem close the builder cluster, and going BACK confirms where the wall is

**2026-09-16** - the last three placeholder builders on PPSA02664's path now hand real cursors; the
title went BACK by one import, and that is the clearest evidence yet that the wall is not the cursors

## What was wired

The three AGC builders still answering the placeholder on PPSA02664's command buffer, now reservation
skeletons - `implementations()` 41 -> 44:

- `sceAgcDcbPushMarker` / `sceAgcDcbPopMarker` - a 12-byte `SET_UCONFIG_REG` to the command-processor
  marker register `0x342`, header `0xc0017904`. The header carries a set bit in its reserved low byte
  that `command_header` does not build; it is inert to the packet walk and kept as the raw measured
  value (`166-agc/dcb-push-marker`/`pop-marker`, sweep `20260915-174357`). The value that distinguishes
  push from pop is zeroed - one pass cannot separate a constant from a colour argument.
- `sceAgcDcbWaitRegMem` - the measured 56-byte **compound** of three packets: a `SET_UCONFIG_REG`
  (`0xc0027904`, four dwords), a `WAIT_REG_MEM` (`0xc0053c00`, seven dwords), and a closing
  `SET_UCONFIG_REG` (three dwords). All three headers are kept and the bodies zeroed, so the
  reservation walks back to three packets and advances the cursor by the measured 56.

## Going BACK is the finding

With these wired, PPSA02664 does **not** go further - it goes BACK, from 222 imports to **221**, while
its calls rise **+80** and its stubs fall **26 -> 16**. That reads wrong until you read what it means:
the markers and WaitRegMem are called heavily, and while they answered the placeholder the guest was on
an *error* path that happened to touch one more distinct import (an error handler) before dying. Handed
real cursors, the guest is on the **real** command-buffer-building path - it builds more (the +80
calls, the 10 fewer stubs) and reaches the actual wall sooner, one distinct import earlier.

Same fault, unchanged: `VCRUNTIME140.dll+0x1dc8d`, `read of 0xa8`, a null base plus an offset in the
Cx-indirect `memcpy` loop. Three batches now - the patch family (616), the a70f cluster (618), and
these three (here) - have each answered more of the cluster's placeholders, and the wall has not
moved once. **The wall is not the placeholder cursors.** It is a structure the guest reads at `+0x8`
of a null pointer (`0xa8 = 168`, and the loop copies eight-byte entries), which no builder answering a
cursor populates. That is the next investigation, and it is a guest trace, not more wiring - which is
exactly what wiring the whole cluster was worth finding out.

The BACK also leaves the best-ever record at 222 where the current, more faithful build reaches 221 -
the Earthion situation (worklog 610): a record from a less faithful path that the honest one no longer
matches. Kept as-is, because rewriting a measured-correct change to hold a metric up is the
plausible-output failure one level up (principle 3); the run report says BACK, so it is not hidden.

## Made to fail

- `the_markers_and_wait_reg_mem_reserve_their_measured_shapes` (gpu) - the markers write the measured
  `0xc0017904` header (reserved bit and all) over 12 bytes with a zero body; WaitRegMem reserves 56
  bytes with all three packet headers present at offsets 0, 16 and 44. A wrong opcode, count, or
  compound framing fails it.
- `the_wired_set_is_the_size_the_module_documentation_claims` - 41 -> 44.
- `every_implemented_function_is_written_down` (service) - two new marker knowledge entries (WaitRegMem
  already had one); each carries its measured header and extent.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,360 pass / 0 fail, worklogs unique, identity scan clean. Frontier
regenerated for the session's cumulative record improvements (PPSA02664 and PPSA03416 to 222,
PPSA04263 to 71).
