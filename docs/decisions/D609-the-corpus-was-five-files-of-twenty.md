# D609 - The corpus was five files of twenty-eight

**Status:** measured
**Date:** 2026-09-08

## What the measurement table was built from

`orbistoun-gen measurements` reads capture files and writes `crates/orbistoun-hle/data/hardware.toml`,
which is the table `tests/hardware.rs` asserts against. It read one directory, `../obscene/data/hardware`,
and accepted one extension, `.txt`.

The sibling project has captures in three places and writes two extensions. So the table rested
on **five files out of twenty-eight**, and neither the generator nor the gate said so - the run
reported "227 distinct measurements" and every number in it was correct.

Two mechanical reasons, both quiet:

- **The extension.** A session transcript is `.txt`; a report is `.obs.log`. Pointed at a
  directory of reports the walker found nothing at all, and the only symptom would have been
  "no capture carried a measure record" - true of what it read, false of what was there.
- **The one directory.** `console-native-run.txt` and `live_launch.txt` are cited as sources by
  the committed table and are not in the directory the generator reads: they moved to
  `reports/archive/hardware` at some point and nothing noticed, because a capture that has gone
  missing contributes no measurement and therefore no complaint.

`--records` is repeatable now, defaults to all three directories, skips one that does not exist
with a warning, and skips a file that is not valid UTF-8 with a warning rather than aborting the
run over a raw kernel log.

## What the other twenty-three files said

| | before | after |
|---|--:|--:|
| captures read | 5 | **28** |
| distinct measurements | 227 | **374** |
| constant across every run | 227 | **262** |

**147 new measurements, and 19 demotions.** Nothing was lost and nothing was contradicted
outright; what changed is that measurements previously seen once or twice are now seen against
fifteen or nineteen runs, and nineteen of them stopped being platform constants.

The demotions are the interesting half:

- **Seven `120-measure/cpuid` fields.** `signature_eax` is `0x740f12` in some runs and `0x840f60`
  in others, with `model`, `stepping` and two feature words following. **The corpus is not one
  machine** - it spans two console generations, which the new `title/ps4-bc` context in this
  morning's reports says out loud. Anything asserted as a platform constant from CPUID was
  asserting one of them.
- **`035-libc/fpu-environment:mxcsr:raw`**, `0x9fe0` twelve times and `0x9fc0` three. The
  difference is bit 5, the sticky precision flag. So the flag is not a property of the platform
  at all - it is whether the console's own startup happened to do inexact arithmetic before the
  probe looked, and orbistoun installing `0x9fc0` is now exactly what three console runs show.
- **Three of the four `direct-memory-query-flags` conditions.** `flags-0` answered `0x0` sixteen
  times and `0x8002000d` three; `flags-2` and `flags-4` answered the invalid-argument code
  eighteen times and `0x0` once. Whether a query with no allocation finds anything is a property
  of the state the machine was in.
- Five module handles, counts and pointers that were already understood to be bookkeeping.

Both assertions that rested on a demoted value are rewritten to claim **membership of every value
any run reported** rather than one of them - which is what `Measurement::values` was built for
and had no caller. The mxcsr test additionally asserts that every bit the runs differ in is a
status bit, so a configuration difference could not hide inside the permitted variation.

## The gate did its job, loudly

`every_constant_measurement_is_claimed_or_declared_outstanding` exists so that "a hardware run
becomes work rather than a file nobody reads". It failed with 110 unaccounted constants, which is
the mechanism working exactly as designed and at four times the scale it had ever run at.

Triaged into two:

**54 opaque** - claims orbistoun structurally cannot make. Console addresses (every base
orbistoun uses is in `docs/ADDRESS_MAP.md` and none of them is this), one machine's user
identifier and network-interface flags, module handles from the console's own numbering, and the
kernel's handoff to an exploit payload, which is below the user-space boundary this project works
inside.

**56 outstanding**, and the split inside it is the finding:

- 25 are conditions of functions orbistoun implements that nothing asserts yet. Ordinary work.
- **31 are symbols the console resolves that this project does not declare at all** - the whole
  of `sceMouse*`, `sceKeyboard*`, `sceAudiodec*`, `sceAjm*`, and six of seven `scePad*` extended
  entries. Not a hard problem: a gap that was previously *unknown* rather than merely undone.
  Principle 6 says which order those come in, and it is not this one - but they are now named,
  counted, and in a file the gate reads.

**Each entry carries its own reason rather than sharing a class note.** A shared note would let a
later entry join the class without anybody deciding it had, which is the same drift a ceiling
file exists to stop.

## And the second copy of the record format is gone

`orbistoun-gen` had its own `OBS|measure|...` field-split, one directory away from the reader
that defines the format. It reads through `orbistoun_probe::parse_line` now - the same reason
D291 and D292 gave that crate its `orbistoun-hle` dependency instead of a second copy of the
merge rule.

The export census is excluded there, deliberately: its subject is a hash and its value is where
the kernel happened to place it, so folding 2,443 of those into a table of checkable claims would
make it four fifths symbol table and mark every address as a platform constant. It goes to the
name search instead (D605).
