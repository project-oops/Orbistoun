# 468. A third title at the same wall

**2026-09-09** - directed, continuing 467

Both requests still open; the kexport data arrived anyway and 467 used it. No new sweeps this pass,
so: what the name bought.

## Naming it made it askable

Naming `sceKernelMapperGetParam` changed no behaviour. What it changed is that a forced dump arms by
**label**, so the call could be asked about at all:

```text
arg0 = 0x600000800e20 -> stack+0x800e20 = 38 00 00 00 00 00 00 00  00 …
```

One argument, a stack local whose first quadword is `0x38` - fifty-six - then zeroes. The
size-prefixed shape `sceKernelDirectMemoryQuery` uses (D083): the caller says how big its structure
is and the kernel fills it. `arg1`-`arg3` are consecutive locals eight bytes apart, which is
leftover registers rather than arguments (D643).

## Thirty-three imports, and the guest narrating

`ORBISTOUN_RETURN=sceKernelMapperGetParam:0x0` - **26 imports to 59, 334 calls to 961, FURTHER**. So
the first abort tests the return and nothing else.

Then the guest says what it is doing:

```text
CreateTextureFromFile(/app0/Textures/ui_assets.gnf): p=0x740005340000, sz=3211264
…
CreateTextureFromFile(/app0/Textures/panel_jp_tv.gnf): p=0x740005e80000, sz=393216
```

**Ten lines; the directory holds exactly ten files.** Every texture the title ships is opened, read
and placed. Asset loading was never the wall.

## Where it stops instead

```text
! libSceAgc::sceAgcCreateShader was called 11 times and nothing implements it
```

PPSA28061 loads its assets and walks into `sceAgcCreateShader` - **the same call PPSA02664 and
PPSA03416 stop at**. Three unrelated titles now end in one place, which is worth more than the
thirty-three imports: it changes what an AGC measurement is worth from one title's problem to the
corpus's.

## Not implemented

Answering `0x0` is worth thirty-three imports and is precisely what principle 3 forbids - nothing
measured says what the fifty-six bytes hold, and the guest aborts again further in, which is the
second observation D227 wants before an intervention counts as a diagnosis. The arity, the size
prefix and the oracle result are in the knowledge file so an implementation starts from evidence
rather than from this run's convenience.

## Surprises

- **The guest was narrating the whole time.** Ten `printf` lines naming files, sizes and addresses,
  in a run that had been summarised as "aborts after 334 calls". They only appeared once it got
  past the mapper call.
- **The compat record already held this result.** PPSA28061's `[experiment]` slot says 60 imports,
  961 calls, "the guest called abort" - the same run, from before the function had a name. The
  slot routing kept it out of the honest record, exactly as designed, and nothing connected it to
  a cause until now.

## Next

- The AGC structure contract, now three titles' wall. obSCEne cannot reach libSceAgc in any leg
  (D641), so this needs a different oracle.
- `sceKernelMapperGetParam`'s fifty-six bytes.
- The libc data-object request, still open, still PPSA21564's wall.
