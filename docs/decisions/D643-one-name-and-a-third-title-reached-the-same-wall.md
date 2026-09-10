# D643 - One name, and a third title reached the same wall

**Status:** measured
**Date:** 2026-09-09

## What the name bought

D642 named `sceKernelMapperGetParam` by pairing the console's export table with this project's
firmware layout, and confirmed it by hash. Naming it changed no behaviour - the report said
`libkernel::sceKernelMapperGetParam` where it had said a hash, and PPSA28061 still aborted after
334 calls.

What the name bought was the ability to **ask about it**. Forced dumps arm by label, so:

```text
arg0 = 0x600000800e20 -> stack+0x800e20 = 38 00 00 00 00 00 00 00  00 …
```

One argument, pointing at a stack local whose first quadword is `0x38` - fifty-six - followed by
zeroes. That is the **size-prefixed structure** shape `sceKernelDirectMemoryQuery` uses (D083): the
caller states how large its structure is, and the kernel fills the rest. `arg1` to `arg3` are
consecutive locals eight bytes apart, which is what leftover registers look like rather than
arguments.

## The one-bit oracle, and what it answered

`ORBISTOUN_RETURN=sceKernelMapperGetParam:0x0`:

```text
imports  59 distinct (+33), 961 calls (+627)
verdict  FURTHER  executed code it could not reach before
```

**Twenty-six imports to fifty-nine.** So the first abort tests the return code and nothing else -
seventy-seven bytes after the call, in the title's own module.

And then the guest says what it is doing, in its own words:

```text
CreateTextureFromFile(/app0/Textures/ui_assets.gnf): p=0x740005340000, sz=3211264
CreateTextureFromFile(/app0/Textures/ui_logos.gnf): p=0x740005780000, sz=3211264
…
CreateTextureFromFile(/app0/Textures/panel_jp_tv.gnf): p=0x740005e80000, sz=393216
```

**Ten lines, and the directory holds exactly ten files.** Every texture the title ships is opened,
read and placed. Asset loading is not the wall and never was.

## Where it stops instead

```text
! libSceAgc::sceAgcCreateShader was called 11 times and nothing implements it
! libSceAgcDriver::sceAgcDriverGetResourceRegistrationMaxNameLength  9 times
! libSceAgcDriver::sceAgcDriverGetDefaultOwner  9 times
```

PPSA28061 loads its assets and walks into `sceAgcCreateShader` - **the same call PPSA02664 and
PPSA03416 stop at** (D621). Three of the corpus's titles now end in one place.

That is worth more than the thirty-three imports. A wall three unrelated titles arrive at
independently is a different kind of target from three separate walls, and it changes what the next
measurement is worth: the AGC contract is not one title's problem.

## Not implemented, and why not

Answering `0x0` is worth thirty-three imports and is exactly what principle 3 forbids: nothing
measured says what the fifty-six bytes hold, and a success return over a zeroed structure is a stub
that cannot be told from working code. The guest agrees - it aborts again, further in, which is the
second observation D227 requires before an intervention counts as a diagnosis.

What is needed is the structure's contents. No capture holds them, and the `166-agc` section already
shows the shape a request for them would take - a before-and-after byte dump of a filled structure.
Recorded here rather than guessed, with the arity, the size prefix and the oracle result written
into the knowledge file so an implementation starts from evidence.
