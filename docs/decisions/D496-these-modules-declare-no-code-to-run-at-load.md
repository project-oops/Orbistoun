# D496 - These modules declare no code to run at load, and `DT_FINI` is why that is a finding

**measured** - 2026-09-03 (six places checked, one asymmetry that settles it)

D495 found the guest calling `sceKernelLoadStartModule` by full path for both modules it ships,
and orbistoun answering a handle without starting anything. The obvious next question is what
"start" runs. **These modules do not say.**

## Six places, checked

| where | what is there |
|---|---|
| `DT_INIT_ARRAY` / `DT_INIT_ARRAYSZ` | both `0x0` (D491) |
| `DT_PREINIT_ARRAY` | absent from the tag list entirely |
| `module_start` / `module_stop` / `module_prolog` exports | none of the three hashes (D492) |
| ELF entry point | `0x0`, on all three modules the title ships |
| `PT_SCE_MODULE_PARAM` | SDK versions at `+0x10`/`+0x14`, per selfish's own writer - not a start address |
| the sibling projects | obSCEne and prosperous have nothing on it |

## And `DT_INIT = 0x10` points at zeros

D491 recorded `DT_INIT = 0x10` as "not an address - `base + 0x10` is the ELF header". **The
conclusion was right and the reason was wrong**, which is worth correcting because the reason is
what a reader would reuse.

Segment 0 is `PT_LOAD`, executable, mapped at the module base, and its contents begin at **file
offset `0x4000`** - so `base + 0x10` is not the header. It is inside the executable segment, and
the bytes there are:

```text
vaddr 0x00:  00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
vaddr 0x10:  00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Zeros. Calling it would execute `add [rax], al` until something faulted. Not a constructor, for
a different reason than the one written down.

## The asymmetry is the actual evidence

`DT_FINI` in the same module is **`0x1487860`** - a real offset, eighty-two bytes from the end of
segment 0's copied run, which is where a last function in `.text` would sit.

So the same toolchain, writing the same table, emitted a genuine offset for the finaliser and
left the initialiser pointing at padding. **That makes the absence deliberate rather than a
format quirk this project has failed to decode.** One missing tag is a gap in understanding;
one missing tag beside its populated twin is a statement.

## What follows

These modules declare no code to run at load. So either the platform runs nothing on them at
`LoadStartModule` - and the `.bss` is filled by the game calling in - or it runs something named
by a mechanism none of the six places above describes.

The first is now the cheaper hypothesis, and it fits: the game calls two of the module's five
bound exports and dies in the second, having written exactly one string (`/app0/Media`) into two
of 318,492 `.bss` words (D495). A module being *driven* rather than *started* is the picture
D492 already pointed at.

**Not concluded**: that the platform runs nothing. Six absences are not a proof, and the run
that would settle it is on hardware - obSCEne can load a module it built and report whether
anything of its own executed before the caller touched it.
