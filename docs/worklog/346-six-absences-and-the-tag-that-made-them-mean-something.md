# 2026-09-03 - (/loop) Six absences, and the tag that made them mean something

```
tests   1991  ->  1991   (investigation; no code)
```

D495 left one question: what does the platform *start* when the guest calls
`sceKernelLoadStartModule` on a module it ships. Six places could name code to run at load.
None of them does.

## The search, in order, siblings first

| where | what is there |
|---|---|
| obSCEne / prosperous / selfish docs | nothing on module start semantics |
| `DT_INIT_ARRAY` / `DT_INIT_ARRAYSZ` | both `0x0` (D491) |
| `DT_PREINIT_ARRAY` | not in the tag list at all |
| `module_start` / `module_stop` / `module_prolog` | none of the three hashes exported (D492) |
| ELF entry point | `0x0` - on all three modules the title ships |
| `PT_SCE_MODULE_PARAM` | SDK versions, per selfish's own writer |

Six absences is a closed search rather than a shrug, which is the point of listing them.

## And a correction to D491, which I had reused twice

D491 recorded `DT_INIT = 0x10` as "not an address - `base + 0x10` is the ELF header". I have
since repeated that reasoning in two loop prompts.

**It is not the header.** Segment 0 is `PT_LOAD`, executable, mapped at the module base, and its
contents begin at file offset `0x4000`. `base + 0x10` is inside the executable segment. What is
actually there:

```text
vaddr 0x10:  00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Zeros. So the conclusion survives - calling it executes padding, not a constructor - and the
*reason* was wrong, which matters because the reason is the part a reader reuses. Checked
because "could `base+0x10` be code?" is the same question that caught D490 and D491 themselves,
turned on my own note.

## `DT_FINI` is what makes the absences evidence

The same module's `DT_FINI` is **`0x1487860`** - a real offset, eighty-two bytes from the end of
segment 0's copied run, exactly where a last `.text` function sits.

So the toolchain that left `DT_INIT` pointing at padding wrote a genuine offset for the
finaliser in the next tag. **A missing tag beside its populated twin is a statement, not a gap
in my understanding of the format.** Without that comparison the six absences would only mean
"I have not found it yet".

## Where that leaves the wall

Either the platform runs nothing at `LoadStartModule` and the `.bss` is filled by the game
calling in, or it runs something no part of the module names. The first is now cheaper and fits
what D495 measured: two of five bound exports called, one string written, 318,490 words of
`.bss` untouched, dies in the second call.

**Not concluded.** Six absences are not a proof, and the run that settles it is on hardware -
obSCEne can load a module it built and report whether anything of its own ran before the caller
touched it. Worth banking with R3/R4 for the next console session.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. fmt clean, identity
scan clean. No code changed this tick.

Nothing committed. The day holds worklogs 292-346 and D466-D496.

**Next**: the encoder path probes - 48 outstanding hardware measurements that the 537-module
manifest makes answerable, and a change of subject after four ticks on one global.
