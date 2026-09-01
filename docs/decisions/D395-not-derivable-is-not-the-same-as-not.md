# D395 - Not derivable is not the same as not measurable


**assumed** - 2026-08-30

`sceKernelGetModuleInfo` fills a vendor structure whose field offsets no lawful source here
describes. The first move was to refuse it - the same reasoning that leaves a notification's
message undecoded, because a guest reading a name out of the wrong offset gets whatever was
beside it, printed as though the platform had said it.

Refusing to **invent** a layout is right. Refusing to **measure** one is not, and that is what
the refusal had quietly become. This project's whole method is that the guest is the oracle;
the handoff field-walk written the same morning does exactly this for a structure nobody has
documented (D390), and it did not occur to me to point it at an out-parameter.

### Two separate things, and only one of them was a mistake

**The refusal itself stays, and improved.** Unimplemented, the call answered this project's
`Unimplemented` placeholder - `0x7fff0001`, deliberately *positive* so it can never be mistaken
for a firmware value, which for a status makes it read as success with a small code. The
conformance probe reported that number straight back. It answers a negative status now, and the
probe reports `0xffffffff`: an honest no, in a form a caller can act on (D125, D273).

**And a run can now ask.** Under `ORBISTOUN_DESCRIBE=module-info` the structure is filled with
markers that name their own offset and the call reports success. A guest that reads a field and
uses it stops on an address decoding back to the offset it came from. One run, one question:
*which field does a title actually want* - which is a work list, not a layout, and that is the
right size of answer.

Emphatically a diagnostic: it writes memory further than the guest may have asked for, answers
success for something that did not happen, and is recorded as intervening.

### What the first run of it found

The probe has a **dump path** for this structure and an assumed layout of its own:

```text
OBS|moduleword|0|0x5e2b00000000
OBS|moduleword|8|0x5e2b00000008
...
110-modules/names  fail  modules were described but not nameably;
                         the layout is not what this assumes
```

Eight words, echoed faithfully, and a verdict that the shape is wrong rather than that nothing
came back. So the structure is 64 bytes as far as that probe is concerned, its dump is exact,
and **the same probe on real hardware emits those lines with the real values**. That is the
layout, measured, from a run somebody is already doing.

Nothing further should be guessed here until those eight numbers exist.

### The general shape

A structure this project cannot describe has three possible answers and only two of them are
honest: refuse, or fill it with something that makes the guest say what it wanted. Filling it
with something *plausible* is the third, and it is the one that produces a run that looks like
it worked.

