# 434. The answer was in the title directory

**2026-09-08** - directed, continuing 433

## What was done

Two hypotheses eliminated cheaply, then a find that reframes the whole wall.

**`sceKernelDlsym` is not how the guest reaches the Ampr functions.** It is called 35,468 times
and the run already reports every *distinct* name it asks for, answered or not - there is one,
`scriptingGetMem`. One command, hypothesis dead.

**The guest calls no `libSceAmpr` function at all**, confirmed under `ORBISTOUN_RESOLVE=all`,
which gives every import a reporting stub. Not a resolution failure; it genuinely does not call
them.

**Then the submit handler stopped guessing at the header and scanned** - every non-zero word from
`-0x40` to `+0x200`, by offset. The negative offsets held a string:
`…/Media/globalgamemanagers.resS`. Which sent somebody to look in the title directory.

## `ampr_emu.index`

```text
titles/PPSA03416-app0/ampr_emu.index          16,248 bytes   magic AMPRIDX3
titles/PPSA03416-app0/fakelib/libSceAmpr.sprx    218,678 bytes
```

**The dump ships a replacement `libSceAmpr` and an index for something to emulate the
asynchronous file path with** (D591). That is why `libSceAmpr(1 init)` is a module the loader
places, and why the title imports the command-buffer constructor and the read-file call and uses
neither: it is not calling the library, because the library was replaced.

141 entries of `{u32 name_offset, u32 name_length, u64 size, u64 modified}`. Decoded and checked
against the directory five times, including entry 26 - `/app0/Media/globalgamemanagers`, 224,748
bytes, which is the file the guest resolves and the byte count yesterday's delivery experiment
read out of it independently.

## Surprises

- **The scan found it, not the reading.** Four readings of that header have been made and
  corrected; the first thing that *asked* rather than assumed found a filename in the negative
  offsets and the trail led off the heap entirely.
- **Three sessions of planting markers in the resolve out-parameters** were asking the guest a
  question the title had answered on disk.
- **`globalgamemanagers.resS` really is a red herring**, as D578 said - it is a Unity companion
  file this title does not ship, and probing for it failing is ordinary.

## Next

- Implement `sceKernelAprResolveFilepathsToIdsAndFileSizes` from the index: the path is in it,
  the size is beside it, the identifier is plausibly the entry's position. The oracle is sharp -
  the title stops saying *"Unknown error occurred while loading"* when it is right.
- Which out-parameter takes the identifier and which the size is still the measurement, but it is
  now a measurement with a known right answer rather than a search.
- What the replacement `libSceAmpr.sprx` exports, which would say what the command buffer is
  meant to be.
