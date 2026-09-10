# D597 - Nothing ever wrote the command, and a second summary read as none

**Status:** measured
**Date:** 2026-09-08

## The storage was never touched, which is different from being zero

The command buffer's storage has read as zero in every run since it became readable, and every
reading of that has assumed the guest wrote zeros or wrote elsewhere. `ORBISTOUN_DIRECT_FILL`
settles it in one boot:

```text
what it points at, 0x740009100000: 0xd1d1d1d1d1d1d1d1 …
non-zero in it: +0x0:0xd1d1… +0x8:0xd1d1… … (sixteen words, all the fill byte)
```

**Nothing wrote a byte of it.** The fakelib sets a storage pointer, sets a size, increments a
command count and sets a flag - and encodes nothing at all. A header that claims one command over
storage that was never touched is not a partially built buffer; it is bookkeeping with no payload
behind it.

The same run shows the buffer *object* carrying the fill byte at `+0x28` and `+0x38`, so parts of
it are uninitialised too.

This is the diagnostic doing what it was built for (D325): *a field nobody filled in and a
deliberate zero are currently indistinguishable*, and here the distinction was the finding.

## A second summary that read as nothing

D595 found `0 KiB` standing for four hundred and two bytes. Auditing the report for the same
shape found one more:

```text
standing 467528 of 467537 calls answered by an implementation (0% on stubs)
```

**Nine calls land on stubs**, and those nine are the only calls in the run worth looking at.
`0% on stubs` reads as *nothing is stubbed*. The share is now printed beside the count rather
than instead of it.

Both are integer division producing a true number that reads as a different claim, and both were
in the block a reader looks at first. Two found; the audit covered every division and percentage
in the report and found no third.

## The ninth `Box::leak`, and why it is not fixed here

`orbistoun_fs::open::next_handle` hands file handles from the host heap, so a `FILE *` is a
different address every run - the leak D584 fixed at eight sites in `orbistoun-kernel`, in a
ninth nobody looked at because the search was scoped to one crate.

Fixing it needs a decision rather than an edit. `orbistoun-fs` depends on neither
`orbistoun-kernel` nor `orbistoun-mem`, so the shared allocator is out of reach, and the options
are to move `blocks` down the spine, to add a dependency, or to install it through a hook as the
asynchronous file path's reader is. **Which crate owns guest-visible blocks is a structural
question**, and answering it by whichever import is easiest to add is how a spine stops meaning
anything. Recorded, and left for a deliberate choice.

## What this does not establish

**Why the encoder writes nothing.** It is called, it returns success, and the count is
incremented on that success - so the fakelib believes it encoded something. Where it believes it
put it is the remaining unknown on this wall.

**Nor that the fill byte proves the storage is the right buffer.** It proves nothing wrote *that*
region. A guest writing commands somewhere else entirely would produce the same evidence, and
that reading is still open.
