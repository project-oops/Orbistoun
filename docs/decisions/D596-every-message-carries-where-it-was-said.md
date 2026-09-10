# D596 - Every message carries where it was said from

**Status:** measured
**Date:** 2026-09-08

## A message says what a guest believes; a call site says which code believes it

D590 started recording what the guest formats, and the strings named the failure at once. What
they could not name is *which* code emitted each one - and a title that prints the same generic
message from four places is telling you nothing about which of the four ran.

The address is already there. Every call into a format function is a thunk call, and a thunk
records the address it returns to, so `last_call().from` is the emitting site with no new
plumbing:

```text
submitCommandBufferAndGetResult error=2147418113   [from 0x400000f1013e]
Unknown error occurred while loading '…/globalgamemanagers'.   [from 0x400000e1f513]
```

Two different functions, a hundred kilobytes apart. The second is the generic logger the run's
call tail had already shown as `libc::vsnprintf(0x6000007fa250) -> 0x98 from 0x400000e1f513`, and
the two records now agree by construction rather than by a reader noticing.

## And a seek is how a guest asks a file's size

The opens record says which files a guest found and the read record says what came back. Neither
can show the oldest way to ask a size: seek to the end, read the position. A title that then
calls a file corrupt was told that size by a seek and by nothing else.

Recorded under the same gate, and it answered immediately: **PPSA03416 never seeks at all.**

## What the three records together establish

For `/app0/Media/globalgamemanagers`, the guest:

- **opens** it - the opens record
- **never reads** it - the reads record, which shows one read in the run and it is `boot.config`
- **never seeks** in it - this record
- **never stats** it - no call to any stat in the run

So the descriptor is opened to establish the file exists and is then untouched. Everything else
goes through the asynchronous file path, which is consistent with what the shim is for (D592) and
is now measured on four separate axes rather than inferred from one.

## What this does not establish

**Which code decides to log a failure.** The call site is where the *message* is formatted, and
for a generic logger that is one function serving every caller. Naming the decision needs the
caller of the logger, which the thunk does not record.

**Nor why the load fails.** The identifier and size can now be answered from the title's own
index and written through any pair of arguments, the answer returns success, and the message is
unchanged - verified rather than assumed this time, because the run says
`answered id through arg2 and size through arg3` when it does it. Six placements, all negative,
and the negatives are real.
