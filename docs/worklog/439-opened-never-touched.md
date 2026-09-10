# 439. Opened, never touched

**2026-09-08** - directed, continuing 438

## What was done

**Every recorded message now carries the address it was said from** (D596). A thunk already
records the address a call returns to, so the emitting site costs nothing new:

```text
submitCommandBufferAndGetResult error=2147418113   [from 0x400000f1013e]
Unknown error occurred while loading '…/globalgamemanagers'.   [from 0x400000e1f513]
```

**Seeks are recorded too**, because seeking to the end is how a guest asks a file's size and
neither the opens record nor the read statistics could show it.

## What four records now say together

The three failure messages are **consecutive** - submit fails, wait fails, the load error
follows, nothing in between. And for `globalgamemanagers` the guest:

| | |
|---|---|
| opens it | the opens record |
| never reads it | the reads record - one read in the whole run, and it is `boot.config` |
| never seeks in it | added this iteration |
| never stats it | no stat call in the run at all |

So the descriptor is opened to prove the file exists and is then untouched. Everything goes
through the asynchronous file path.

## The six negatives are real

`ORBISTOUN_APR_ANSWER` was checked rather than trusted: the run says
`answered id through arg2 and size through arg3` when it writes, and returns success when it
does. So the identifier and size from the title's own index can be delivered through any pair of
arguments, the call succeeds, and the message is unchanged.

## Surprises

- **The guest opens a file it never touches.** Four separate records were needed to establish
  that, and each was added for a different reason.
- **`last_call().from` was sitting there the whole time.** The call tail has printed `from
  0x400000e1f513` beside `vsnprintf` for months; the format record simply did not carry it, so
  the two outputs could only be joined by a reader noticing.

## Next

- Where the guest expects the bytes. That is the last unknown on this wall and it is the
  fakelib's own command encoding.
- Which code *decides* to log the failure - the call site names the logger, not its caller.
- The ninth `Box::leak`, and whether any other summary rounds a finding away.
