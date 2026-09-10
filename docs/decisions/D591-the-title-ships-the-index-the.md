# D591 - The title ships the index the asynchronous file path needs

**Status:** measured
**Date:** 2026-09-08

## Scanning instead of guessing, and the string next door

Every reading of the command buffer's header has been a guess at which field is which, and each
one has had to be corrected. So the submit handler stopped guessing and **scanned**: every
non-zero word from `-0x40` to `+0x200` around the buffer, reported by offset.

The header was as before. The negative offsets were not:

```text
-0x28:0x626f6c672f616964  -0x20:0x616d656d61676c61  -0x18:0x722e73726567616e  -0x10:0x537365
```

Little-endian, that is `dia/glob` `algamema` `nagers.r` `esS` - **`…/Media/globalgamemanagers.resS`**,
sitting immediately before the object. Which is a Unity companion file this title does not ship,
so the probe for it fails and that is ordinary: D578 was right that the four missing paths are
not the problem.

What it did was send somebody to look in the title directory.

## `ampr_emu.index`, in the title's own root

```text
titles/PPSA03416-app0/ampr_emu.index      16,248 bytes
titles/PPSA03416-app0/fakelib/libSceAmpr.sprx    218,678 bytes
```

`AMPRIDX3` is the magic. **The dump ships a replacement `libSceAmpr` and an index built for
something to emulate the asynchronous file path with**, which is why `libSceAmpr(1 init)` is a
module the loader places rather than a set of stubs orbistoun declares - and why the title
imports `sceAmprAprCommandBufferConstructor` and `sceAmprAprCommandBufferReadFile` and calls
neither. It is not calling the library. The library was replaced.

## The format, decoded and checked four times

| | |
|---|---|
| `+0x00` | `AMPRIDX3` |
| `+0x08` | `3` - a version |
| `+0x0c` | `24` - the entry stride, and the entries are 24 bytes |
| `+0x10` | `141` - the entry count |
| `+0x30` | the entries begin |
| `+0xd68` | the names begin, at `48 + 141 * 24` |

An entry is `{u32 name_offset, u32 name_length, u64 size, u64 modified}`.

**Checked against the directory rather than believed**, which is the only reason to state it:

| entry | index says | the directory says |
|---|--:|--:|
| 0 `/app0/debug.log` | 0 | 0 |
| 1 `/app0/eboot.bin` | 27,744,015 | 27,744,015 |
| 2 `/app0/fakelib/libSceAmpr.sprx` | 218,678 | 218,678 |
| 3 `/app0/sce_sys/keystone` | 96 | 96 |
| **26 `/app0/Media/globalgamemanagers`** | **224,748** | **224,748** |

Entry 26 is the file the guest resolves, and 224,748 is the byte count D589's delivery experiment
read out of it independently.

## Why this is admissible, said plainly

This is **guest material at rest**, which `docs/PROVENANCE.md` calls `static` evidence: read out
of a file in a title directory, nothing executed, nothing disassembled, no vendor source. It is
the same category as reading a module's own import table, and it sits a tier below a name
confirmed by hash only in what somebody else needs to reproduce it - the title.

It is emphatically not the vendor's `libSceAmpr`. The interesting file here is the *replacement*,
and what is read is a sixteen-kilobyte table of paths and sizes.

## What this changes

`sceKernelAprResolveFilepathsToIdsAndFileSizes` has, for the first time, a **source of truth for
what it should answer**: the path is in the index, the size is beside it, and the identifier is
plausibly the entry's position. Three sessions of planting markers in its out-parameters were
asking the guest a question the title had already answered on disk.

## What this does not establish

**Which out-parameter takes which.** The index says what the answers *are*; it says nothing about
where to write them. That is still the measurement to make, and it is now a measurement with a
known right answer rather than a search.

**Nor that the identifier is the entry index.** It is the obvious candidate and nothing has
tested it. The index carries no explicit identifier field, which is itself evidence - a position
is the only identifier available.

**Nor what the remaining header words mean.** `4618`, `8056`, `16` and `512` sit at `+0x18`
through `+0x2c` and are not the name-table offset, which is arithmetic from the count. Unused
here, and unexplained.

**Nor that every title carries one.** This is one dump. A title without an index needs the
mechanism understood rather than tabulated, and nothing here reaches that.
