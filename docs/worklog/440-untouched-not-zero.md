# 440. Untouched, not zero

**2026-09-08** - directed, continuing 439

## What was done

**One boot settled what six iterations of reading could not.** `ORBISTOUN_DIRECT_FILL=d1` fills
every direct-memory mapping with a byte nobody would write, and the command buffer's storage comes
back entirely `0xd1`:

```text
what it points at, 0x740009100000: 0xd1d1d1d1d1d1d1d1 …
```

**Nothing ever wrote it.** The fakelib sets a pointer, a size, a count and a flag, and encodes no
command at all - so a header claiming one command over untouched storage is bookkeeping with
nothing behind it (D597). Zero and never-written were indistinguishable until a diagnostic built
for exactly that distinction (D325) was pointed at it.

**Then audited the report for the shape D595 found**, and there was a second:

```text
standing … answered by an implementation (0% on stubs)
```

Nine calls land on stubs and those nine are the only ones worth looking at. `0%` reads as
*nothing*. It prints the count beside the share now. Every division and percentage in the report
was checked; there is no third.

## Surprises

- **`DIRECT_FILL` has existed since D325** and answered this in one command. Six iterations read
  "the storage is zero" and none of them asked whether zero meant *written* or *never touched*.
- **The buffer object itself is partly uninitialised** - `0xd1` at `+0x28` and `+0x38`.
- **A third wall-clock test flaked under the loop's own load**
  (`a_timed_acquisition_gives_up_at_its_deadline`, 79.9ms, passing on re-run). Three different
  tests now, all timing, all under concurrent boots.

## The ninth `Box::leak`, deliberately left

`orbistoun_fs::open::next_handle` hands file handles from the host heap - the leak D584 fixed at
eight sites, in a ninth that was missed because the search was scoped to one crate.

`orbistoun-fs` depends on neither `orbistoun-kernel` nor `orbistoun-mem`, so the shared allocator
is out of reach and the fix is a structural choice: move `blocks` down the spine, add a
dependency, or install it through a hook. **Which crate owns guest-visible blocks is not a
question to answer by whichever import is easiest to add**, so it is recorded and left.

## Next

- Where the fakelib believes it put the command. It is called, returns success, and the count is
  incremented on that success - so it believes it encoded something.
- The ownership question above, which is a decision rather than a task.
- The wall-clock tests, now three occurrences: this loop is the load, and a suite that fails under
  load is a suite that cannot gate a loop.
