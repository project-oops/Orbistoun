# 506. The AGC builders run as pure encoders, the decoder holds, and one builder is new

**2026-09-10** - loop, on obSCEne's `20260910-174437-eboot.obs.log` (the operator pointed me at it)

The `166-agc` section that had skipped on every prior leg ("libSceAgc is not loaded") finally
produced bytes on this eboot leg. It is real external-oracle GPU data - obSCEne calling the actual
hardware command builders - and it is the safe kind to build against, distinct from the home-app
trap flagged earlier this session (making our own app run proves only that two things we wrote
agree). Here orbistoun's *decoder* is held against bytes obSCEne read off hardware.

## The builders are pure encoders, which is why this channel works at all

The leg reports `title/unknown-gpu | no GPU library among loaded modules`, and every `libSceAgc`
import is `linked | unresolvable`. Yet `cb-nop`, `cb-release-mem`, `dcb-dma-data`,
`dcb-wait-reg-mem` and `dcb-reset-queue` all ran and wrote real PM4 (rc `0x200030078` each). The
reconciliation: these builders are **pure PM4 encoders** - they write packet bytes into a
caller-supplied buffer and need no initialised driver - so obSCEne can resolve their addresses and
call them even where the GPU library is not a loaded module. That is precisely why the builder
channel yields data while `sceAgcCreateShader` on the same leg refuses with `0x8a6c002f` and writes
nothing (sentinel-verified, worklog 505): create-shader needs the driver, the encoders do not.

**So the builder channel is the sustainable GPU-decoder oracle**, reproducible without solving the
libSceAgc-mapping problem (7a5d) that blocks the shader/out-parameter fills.

## The decoder holds, on a second independent run

`measured_packets.rs` was built (worklog 499, D565) on the earlier `run-native-title.txt` captures.
This leg reproduces the same structural headers from a different sweep and a different leg:

| builder | header | opcode | length | vs run-native-title.txt |
|---|---|---|---|---|
| cb-nop | `0010ffff` filler | 0x10 (nop) | 4 B | identical |
| cb-release-mem | `0xc0064900` | 0x49 RELEASE_MEM | 32 B | header/length identical; body differs (zeroed args here vs the `b0bbffee` marker there) |
| dcb-dma-data | `0xc0055000` | 0x50 DMA_DATA | 28 B | header/length identical |
| dcb-wait-reg-mem | `0xc0027904`,`0xc0053c00`,… | 0x79, 0x3c, 0x79 | 56 B (16+28+12) | identical three-packet split |

Bodies vary with the probe's arguments; the **structure** - headers, opcodes, packet boundaries,
lengths - is invariant and matches. `cargo test -p orbistoun-gpu --test measured_packets`: 4/4 green
against this. Two independent hardware runs now agree with the transcribed-from-AMD-docs decoder.

## New: dcb-reset-queue, and 0010ffff confirmed as filler

The one builder not previously captured. 32 bytes, which obSCEne labels a `writer-struct` rather than
a `pm4-pass` - and the label is right:

```
0010ffff  047902c0 42030000 000020ce 00000000  047901c0 42030000 0000a0ce
[filler]  [ 0x79, count 2, 4 dwords          ] [ 0x79, count 1, 3 dwords ]
```

It leads with `0010ffff` - the *same* dword `sceAgcCbNop` emits as its whole output - then two
opcode-0x79 packets. That is the second place `0010ffff` appears as an uninitialised/empty slot whose
`0x3fff` count is not a body length, which is exactly why orbistoun's decoder records it as the one
capture that "does not close" (`the_no_op_is_the_one_that_does_not_close`). Recorded on the
`sceAgcDcbResetQueue` knowledge entry, `known_by = measured`. Not added to `measured_packets.rs`: it
introduces no new opcode and its NOP prefix makes it another non-closing case already covered - a
test entry would be bloat, not coverage.

## Next

- The builder channel can grow the decoder's coverage cheaply: any *new opcode* a future capture
  emits (a draw, a set-context-reg, a dispatch) is worth adding to `measured_packets.rs`, because
  that is new structural ground. `dcb-reset-queue` was not, and I said so rather than padding.
- `sceAgcCreateShader`'s fill still needs a driver-initialised leg (7a5d/b7e2) - the builders being
  pure encoders is exactly why they cannot substitute for it.
- home stays parked, per the operator: it is our own code and not an oracle.
