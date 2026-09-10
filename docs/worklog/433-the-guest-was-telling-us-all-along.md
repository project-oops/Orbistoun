# 433. The guest was telling us all along

**2026-09-08** - directed, continuing 432

## What was done

Three iterations had pushed the asynchronous file path and got negatives. Changed angle: the
guest renders a hundred and fifty-two bytes with `vsnprintf` immediately before reporting
abnormal termination, and **orbistoun produced that string and threw it away**.

`ORBISTOUN_TRACE_FORMAT` keeps what `render_with` renders - the one function every format entry
point reaches (D590). The last hundred lines of a boot:

```text
submitCommandBufferAndGetResult error=2147418113
waitCommandBufferCompletion error=2147418113
Unknown error occurred while loading '/app0/Media/globalgamemanagers'.
Failed to load PlayerSettings (internal index #0).
Most likely data file is corrupted, or built with mismatching
editor and platform support versions.
```

**The chain is measured rather than inferred.** Apr fails with orbistoun's placeholder, the title
cannot load `globalgamemanagers`, `PlayerSettings` is the first object in it, the title stops.
Everything downstream - the abnormal-termination report, the null singleton, the fault - is the
guest dying tidily.

## It corrects yesterday's framing

D589 concluded the Apr path was not what the title was waiting for, because delivering the bytes
changed nothing. The guest's own words say it is exactly what it was waiting for. What D589
measured is narrower than what it said: delivering bytes *to that address* is not how the guest
collects them.

The two-dimensional run was also finally done properly - forced success **and** delivery
together, which D283 and D286 warn is the only way to see a guest that checks a return before
reading a buffer. It removes both `error=` lines and leaves the load failure, so the return and
that buffer are not the two dimensions that matter.

## Surprises

- **The most useful diagnostic in the run was already being computed.** Not a new measurement, a
  new *record* - the fourth time this week that the thing needed was the half of an existing
  output nobody kept (D578 paths, D581 mappings, D588 ranges, this).
- **Two of the three decisive lines never reach a stream.** The `error=` pair is visible and is
  what led to the Apr path in the first place; the three lines naming the actual failure go into
  a buffer the guest keeps.
- **The record keeps the last strings, not the first** - the opposite of `opened`. A guest says
  the interesting thing immediately before it stops.
- **I made the same insertion mistake twice in one iteration**, anchoring a new declaration
  before the *next* doc comment instead of after the previous `};`, which detached a doc block
  from its item both times. Caught by `missing_docs` both times.

## Next

- Where the guest actually expects the file's bytes, which is now the whole question and has a
  measurable oracle: the title stops saying "Unknown error occurred while loading" when it is
  right.
- `sceKernelDlsym` is called 35,468 times. A guest resolving symbols dynamically may be checking
  for the Ampr functions and skipping the path when it cannot find them, which would explain why
  it imports `sceAmprAprCommandBufferReadFile` and never calls it.
- Everything the earlier iterations queued is still queued.
