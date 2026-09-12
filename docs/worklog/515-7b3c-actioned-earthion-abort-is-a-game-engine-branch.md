# 515. 7b3c actioned faithfully; Earthion's abort is a game-engine branch, not a firmware gap

**2026-09-12** - actioned obSCEne's 7b3c resolution; committed the measured returns as legboots (647952e)

obSCEne resolved `REQ-...0925Z-7b3c` by disassembling retail `libSceAgcDriver.sprx` and confirming on
hardware. The whole AGC resource-registration family is a stub on retail: `sceAgcDriverRegisterOwner`,
`RegisterResource`, `InitResourceRegistration` are each `mov $0x8a6c9018, %eax; ret`, and
`sceKernelMapperGetParam` returns `0x80020006` (EINVAL) because it is gated on that stubbed subsystem.
Implemented every one at its measured value and committed - the honest firmware behaviour, principle 1.

## What was implemented (647952e)

- `agc_driver.rs`: `sceAgcDriverRegisterOwner`, `RegisterResource`, `InitResourceRegistration`,
  `QueryResourceRegistrationUserMemoryRequirements` all declared and bound, each returning the measured
  `0x8a6c9018` (`RESOURCE_REGISTRATION_NOT_SUPPORTED`).
- `orbistoun-kernel`: `sceKernelMapperGetParam` declared (`=> 1`), bound in `TABLE`, returns the
  measured `0x80020006`.

**Comments corrected before landing.** The handlers had been written claiming the measured return
"lets the guest take the path the console does" and "unblocks PPSA28061's startup". The run falsifies
that (below), so those causal claims were removed - they were the principle-3 tooling trap (a comment
reporting more than the measurement supports). The returns are landed for *fidelity*, not as an unblock.

**One deferred fidelity gap.** 7b3c's disassembly of `QueryResourceRegistrationUserMemoryRequirements`
is `test %rdi; je; movq $0,(%rdi); mov $0x8a6c9018,%eax; ret` - it writes `0` to `*arg0` before
returning. The handler returns the code but does not write. It is **not on the current abort path**
(the guest aborts at the mapper, before reaching Query in an honest run), so adding an `unsafe` guest
write for an unreachable branch is exactly what principle 6 warns against. Noted here; implement with a
test when the guest reaches it.

## The run did not move (and D670 says that is correct)

Honest PPSA28061 run, 2026-09-12: **22 distinct imports, 391 calls, verdict `same`**, `the guest called
abort`. That is *below* the best-ever frontier of 47/933 (2026-08-23) - and that is expected, not a
regression. Per D670 the old record was earned under a placeholder that read as success (`0x7FFF0001`,
positive); flipping it to `0xF7FF0001` (negative) made the guest see unimplemented calls as the failures
they are, so the corpus reads `BACK`. 933 was how far it got before the lie caught up. 391 is the floor.

## Where it actually stops - and it is not the eboot

The pre-abort call sites are the finding:

```
just before: libc::abort(0xbe9c)                                   from 0x480000a1c7ad
just before: sceKernelMapperGetParam(0x600000800e20) -> 0x80020006 from 0x480000a1c760
just before: sceAgcDriverRegisterOwner(0x600000800e8c) -> 0x8a6c9018 from 0x40000012747c
just before: sceAgcCreateShader(0x4000004f6e50) -> 0x0             from 0x40000012741e
```

`create-shader` and `RegisterOwner` are called from the **main image** (`0x400000...`). The mapper call
and the abort are in a **different module** at `0x480000...`, `0x4d` (77) bytes apart - exactly D643's
"seventy-seven bytes after the call". The offset `0xa1c760` (~10.6 MB) is past the 2.3 MB `eboot.bin`
and inside the 24.8 MB `game.bin`, so the caller is the **game engine (`game.bin`), not the eboot**. The
eboot creates the shader and registers the owner; the engine calls the mapper and aborts on its result.

And the title's `app0` is **not a stock boot**: it ships faked libraries (`fakelib/libSceAmpr.sprx`) and a
separately-loaded `game.bin`. So the engine reaches this mapper call in a modified init context, which
matters for the divergence below.

## The divergence, stated honestly

The engine requires `sceKernelMapperGetParam` to succeed (D643: `:0x0` reaches 59 imports; the diagnostic
owner+mapper prop reached 1080). But on retail firmware resource-registration is stubbed, so the mapper
*never* fills - it returns `0x80020006` every time (7b3c). A shipped title presumably still runs, so
either it has a fallback path the engine does not take here, or the non-stock boot flow puts the engine
in a state a legitimate boot would not. **We cannot tell which from a firmware measurement** - 7b3c
already gave the firmware truth, and the branch that turns `0x80020006` into `abort` is in the engine's
own code. The next move is **orbistoun-side guest disassembly** of `game.bin` at `0xa1c760..0xa1c7ad`
to read the exact condition - oracle #3/#4, not another request to obSCEne. Recorded as D677.

Returning `0x0` to move past it is off the table: nothing measured says what the 56-byte size-prefixed
struct holds, so `0x0` is the plausible-output success principle 3 forbids (and D670's whole point).

## sceAgcInit confirmed

7b3c item 6 named the long-unresolved `libSceAgc::0x53bbd82b51d172db` as **`sceAgcInit`** (from obSCEne's
`platform.h`), confirming D649's identification. It is already named in `libSceAgc.toml`; the resolver
still shows the hash because the name does not hash to that NID - it is a published alias no derivation
reaches (D649), which is a known, documented limitation, not a gap to close.

## State

- `cargo test -p orbistoun-gpu -p orbistoun-kernel` green; identity guard `--cached` clean; committed
  647952e as legboots. Push stays blocked on the prose gate (d529), the other session's to clear.
- 7b3c is RESOLVED/terminal in the mesh; filed a scoped follow-up under OPEN REQUESTS reporting it is
  actioned and asking the one thing obSCEne can uniquely answer (is there any retail path where the
  mapper fills, e.g. sequenced after `sceAgcInit` rather than resource-registration).

## Next

- Disassemble `game.bin` at the mapper-call/abort branch (`0x480000a1c760`) - the load-bearing step.
- Implement Query's `movq $0,(%rdi)` write when the guest reaches it, with a test.
