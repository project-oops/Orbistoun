# 791. PPSA03416 loads a working fakelib `libSceAmpr` and still faults identically at `image+0x3f8f0`, ruling out the Ampr command-buffer construction as the cause; the `array[1]+0x18` null is neither the descriptor nor Ampr

**2026-09-22** — worklog 790 left PPSA02664's `array[1]+0x18` null as unallocated Unity workload-array
state. A frontier reassessment surfaced a buildable-looking candidate - orbistoun stubs every
`libSceAmpr` command-buffer function (`ampr.rs`, 5 names, none implemented) - and PPSA02664 lacks the
fakelib that PPSA03416 ships. This tick tested that candidate against PPSA03416 and it is disproven.

## The Ampr candidate, and why it looked right

`array[1]` is a `0x70`-byte workload-array element whose `+0x18` entry-buffer pointer is null, and
PPSA02664's unimplemented calls include `sceAmprCommandBufferConstructor`,
`sceAmprAprCommandBufferConstructor` and `sceAmprCommandBufferSetBuffer` - a constructor-and-set-buffer
family that plausibly builds exactly that element and sets its buffer pointer. PPSA02664 ships no
`libSceAmpr`, so those calls land on orbistoun's empty stubs (nothing built); PPSA03416 ships
`fakelib/libSceAmpr.sprx` + `ampr_emu.index`, a replacement that implements the `sceAmpr*` names in guest
code (`amprindex.rs`, D591/D592). If the Ampr construction set `+0x18`, PPSA03416 - whose fakelib builds
the command buffer in-guest - should clear the wall.

## Disproven: the fakelib loads and it faults the same

PPSA03416's run binds **`libSceAmpr 5`** imports and starts **`libSceAmpr(1 init)`** - the fakelib is
loaded and initialised, so its guest code builds the command buffer and orbistoun records no `sceAmpr*`
call. And it **still faults byte-for-byte at `image+0x3f8f0`, write to `0x94`**, through the same chain
(`0x71040c4df8235e1d -> placeholder`, `memcpy n=0x100`, `0x7d86501b8094ef57 -> 0`). So a correctly built
Ampr command buffer does not populate `array[1]+0x18`, and the construction is not the cause.

That rules Ampr out on the same evidence that ruled the zeroed AGC descriptor out (worklog 789): both
titles reach the identical null with Ampr handled two different ways (fakelib vs orbistoun stub), so the
null is upstream of and independent of Ampr.

## What stands after the eliminations

`array[1]+0x18` is set by **neither** the AGC descriptor (`0x71040c4df8235e1d`, zero-fill no-op) **nor**
the Ampr command buffer (fakelib no-op). The `count`/`index`/source are Unity engine values (worklog
790). So the null is Unity's own workload-array element buffer, established somewhere orbistoun diverges
that none of the named AGC/Ampr calls covers - which is the `CreateWorkload` hardware trace worklog 770
filed (REQ-...7a5d), or a multi-level trace of the Unity workload-array constructor. The phantom-return
hint the report repeats is a heuristic mis-attribution here (worklog 790): the `0x7d86501b8094ef57` call
in function `0x3f000` feeds `[rbx+0x10]` size arithmetic, not the array element.

This narrows the trace's job to exactly one question - what allocates each `unity_obj+0x38` element's
`+0x18` buffer - and confirms no named-import stub (`libSceAgc` or `libSceAmpr`) is the lever. The two
sibling AGC titles are the same wall, now eliminated down to Unity-internal state.

## Gate state

No code changed - an elimination that loads PPSA03416's working Ampr fakelib and shows the wall is
identical, ruling out Ampr construction (as worklog 789 ruled out the descriptor), and restores the
`[experiment]` scratch both diagnostic runs wrote. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
