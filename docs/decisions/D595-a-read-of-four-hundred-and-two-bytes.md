# D595 - A read of four hundred and two bytes printed as zero

**Status:** measured
**Date:** 2026-09-08

## The line that misled four days of work

```text
files 1 reads, 0 KiB, none cut short
```

D578 read that as *"one file read of zero bytes"*, said so in three places, and built a line of
investigation on it: a guest that opens files and reads nothing must be getting its data some
other way, so the asynchronous file path must be the wall.

**It was four hundred and two bytes, complete, of `/app0/Media/boot.config`.** Integer division
by 1024 rounded a successful read to nothing.

The output was never false. `402 / 1024` is `0`, and the line said `KiB`. It is the shape
principle 3 exists to refuse one level up: **true, and reads as something else**. Anything under
a mebibyte says bytes now.

## What actually happens, recorded per read

`read_stats` counts reads, bytes and short reads, which cannot say *which* read of *which* file
returned what - and for a run with one read those are the only two things worth knowing. So the
opens record keeps the reads too, under the same gate:

```text
orbistoun: and read from them 1 time(s):
  /app0/Media/boot.config: asked 402, got 402
```

**The guest opens `globalgamemanagers` and never reads a byte through the descriptor.** That much
of D578's conclusion survives, and it is now measured rather than inferred from a rounding error.

## The diagnostic was polluting what it measured

Reading `ampr_emu.index` through `orbistoun_fs::open` put orbistoun's own read in the guest's
descriptor table and in the guest's statistics - a run reported *"1 of 3 reads were CUT SHORT"*
where two of the three were orbistoun's and the short one was a chunked read of its own index.

It goes through the mount table to a host path now. **This is orbistoun reading a file, not the
guest reading one**, and the distinction is exactly what principle 9 is about: an observation
heavy enough to change what it observes has changed it.

## The count of fifty-eight is not the wall either

D594 found the replacement library has 99 imports and 58 unresolved, and framed the wall as
fifty-eight functions. Its own caveat said *"unresolved means orbistoun does not declare it; some
may never be called"*, and the measurement confirms the caveat: under `ORBISTOUN_RESOLVE=all`,
which gives every import a reporting stub, **the title reaches three of them** - the resolve, the
submit and the wait. No `sceKernelWrite*Command` is called at all.

So the framing was too broad and is corrected here rather than left standing.

## What this does not establish

**Why the load fails.** The Apr calls are three, they are reached, forcing all three to succeed
removes the guest's two complaints about them and changes nothing else - the format record shows
the two runs saying byte-identical things otherwise. Something after the Apr path fails, and
nothing here names it.

**Nor that no other unit is rounding a finding away.** This one was found because a per-read
record disagreed with a summary. Every other summary in the report is a candidate for the same
mistake and none has been checked.
