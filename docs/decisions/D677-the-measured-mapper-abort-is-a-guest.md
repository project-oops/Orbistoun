# D677 - the measured mapper abort is a guest-engine branch, not a call to fake

**Status:** decided
**Date:** 2026-09-12

## The choice

`sceKernelMapperGetParam` returns the measured `0x80020006` (7b3c), and PPSA28061 aborts on it. The
diagnostic alternative - return `0x0` - reaches 1080 calls (D643 and worklog 513). The choice is which
to keep, and it is to **keep the measured `0x80020006` even though it aborts the title**, and to treat
the abort as the next thing to *understand*, not the next thing to *move*.

## Why not `0x0`

`0x0` is a success over a 56-byte size-prefixed structure whose contents nothing has measured. On retail
firmware resource-registration is stubbed (`sceAgcDriverRegisterOwner` and friends are pure
`mov $0x8a6c9018; ret` stubs, 7b3c), so the mapper never fills and `0x80020006` is the only value it
ever returns. Answering `0x0` is therefore the plausible-output success principle 3 exists to forbid -
the same trap D670 caught for `sceCommonDialogInitialize`, where "answering 0 lets it proceed" was reach
bought with a lie. The reach is real and it is worthless.

## Why the abort is not a firmware question

The pre-abort call sites place the abort at `0x480000a1c7ad`, `0x4d` bytes after the mapper call at
`0x480000a1c760`. That module (offset ~10.6 MB) is `game.bin`, the game engine - not the `eboot.bin`
that created the shader and registered the owner (`0x400000...`). So the branch that turns `0x80020006`
into `abort` is the engine's own code. 7b3c already delivered the firmware truth; another firmware
measurement cannot explain a guest branch. The oracle for a guest branch is the guest (oracle #3/#4):
disassemble `game.bin` at `0xa1c760..0xa1c7ad` and read the condition.

This also honours D227: moving the wall by faking the return would be an intervention with no second,
different observation confirming *why* the guest proceeded. Reading the branch is that second
observation, obtained without changing the program.

## The complication worth recording

The title's `app0` is not a stock boot: it ships faked libraries (`fakelib/libSceAmpr.sprx`) and a
separately-loaded `game.bin`. The engine may therefore reach this mapper call in an init state a
legitimate boot would not produce, which is a second reason the measured firmware return need not
reproduce the console's path here. Disassembling the branch is what tells the two apart.

## Confirmed - the branch was read (worklog 516)

`ORBISTOUN_WATCH` snapshotted the engine's code at the call, and capstone decoded it:
`mov qword [rsp],0x38; call sceKernelMapperGetParam; test eax,eax; jne abort`. It is a hard
must-succeed gate - any non-zero return aborts with no fallback - and on success it distributes four
qwords the mapper filled (`[rsp+0x08/0x10/0x18/0x20]`) into four caller out-pointers. So the measured
`0x80020006` produces exactly the abort the guest's own code dictates: the behaviour is faithful. And
faking `0x0` is now shown to be five wrong values, not one - the code and the four load-bearing qwords
behind it, none measured - which would fault later in the D670 pattern. The decision holds as reasoned.

## Consequence

The measured returns stay (worklog 515, commit 647952e). Earthion's frontier stays at its honest floor
(22/391); the branch is now read, so the remaining unknown is purely a retail datum - can the mapper
return `0` at all, and the four qwords it fills - which only obSCEne can supply (a2f9, updated with the
exact offsets). No firmware request is raised for the abort itself.
