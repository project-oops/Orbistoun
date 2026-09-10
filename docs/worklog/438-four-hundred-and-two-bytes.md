# 438. Four hundred and two bytes

**2026-09-08** - directed, continuing 437

## What was done

**Measured which of the fifty-eight the title reaches: three.** Under `ORBISTOUN_RESOLVE=all`,
which stubs every import so any call is reported, only the resolve, the submit and the wait are
called. No `sceKernelWrite*Command` at all. D594's own caveat said unresolved need not mean
called; the caveat was right and the framing above it was too broad, so it is corrected.

**Diffed what the guest says under baseline and forced success.** Two lines differ out of 296 -
the two `error=2147418113` complaints - and everything else, including *"Unknown error occurred
while loading"*, is byte-identical. The Apr return values are not what fails the load.

**Then recorded every read, and found the premise of four days of work was a rounding error**
(D595).

## `files 1 reads, 0 KiB, none cut short`

That line was read as *one file read of zero bytes*, written into D578 and three worklogs, and
used to argue that the guest must be getting its data another way.

**It was four hundred and two bytes of `boot.config`, read completely.** `402 / 1024` is `0`, and
the line said `KiB`.

```text
orbistoun: and read from them 1 time(s):
  /app0/Media/boot.config: asked 402, got 402
```

Anything under a mebibyte says bytes now. What survives of the original conclusion is that the
guest opens `globalgamemanagers` and never reads a byte through the descriptor - measured per
read rather than inferred from a rounded summary.

## And the diagnostic was polluting what it measured

Reading `ampr_emu.index` through the guest's filesystem put orbistoun's own read in the guest's
descriptor table and statistics: a run reported *"1 of 3 reads were CUT SHORT"* where two were
orbistoun's. It goes through the mount table to a host path now - orbistoun reading a file is not
the guest reading one.

## Surprises

- **The output was never false.** It was true and read as something else, which is the shape
  principle 3 refuses one level up, arriving as a unit conversion.
- **A ninth `Box::leak`**: `orbistoun_fs::open::next_handle` hands file handles from the host
  heap, missed when the eight in `orbistoun-kernel` were moved to one region (D584). Not fixed
  yet.
- **Two gate tests are load-sensitive**, and this loop is the load: the frontier test compares
  committed numbers against compat records my own runs update, and
  `a_timed_wait_never_gives_up_before_its_deadline` failed at 4.72ms of a 5ms deadline under
  concurrent boots, then passed three times in isolation.

## Next

- Why the load fails, which is now the whole question: three calls, all reached, forcing them all
  to succeed changes nothing else the guest says.
- The ninth `Box::leak`.
- Whether any other summary in the report rounds a finding away. This one was found because a
  per-read record disagreed with a summary; nothing else has been checked that way.
