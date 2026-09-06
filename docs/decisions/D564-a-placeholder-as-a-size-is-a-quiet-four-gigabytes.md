# D564 - A placeholder answered as a *size* is a very quiet four gigabytes

**Status:** guest-observed
**Date:** 2026-09-04

## What was found

PPSA28061 stops at 25 imports where its record claimed 47. Chasing that turned up the sequence
below, which is worth reading in order:

```text
#311  sceUltWaitingQueueResourcePoolGetWorkAreaSize(16, 16) -> 0x7fff0001
#312  malloc(0x7fff0001)                                    -> a pointer
#313  _sceUltWaitingQueueResourcePoolCreate(...)            -> 0x7fff0001
#314  sceUltUlthreadRuntimeGetWorkAreaSize(16, 3)           -> 0x7fff0001
#315  malloc(0x7fff0001)                                    -> a pointer
#316  _sceUltUlthreadRuntimeCreate(...)                     -> 0x7fff0001
```

`0x7fff_0001` is orbistoun's unimplemented placeholder. As a **size** it is 2,147,418,113 bytes,
and this emulator's `malloc` **served both requests**. Four gigabytes, twice over, and neither the
run report nor any gate said a word.

## The principle this sharpens

Principle 3 says a stub returning success is indistinguishable from working code. The placeholder
exists to be *loud* - it is a value no vendor function returns, so a guest that acts on it goes
visibly wrong.

**That reasoning assumes the return is a status.** Where a function answers a *size*, the
placeholder is not loud at all: it is a number the caller spends, and spending it succeeds. The
D125 family already covers a placeholder answered where a **pointer** was wanted; this is the same
mistake one type over, and it is quieter than the pointer case because nothing crashes.

Worth stating as a rule: **a function whose answer is arithmetic - a size, a count, a length, an
offset - cannot be left unimplemented safely.** It needs a real number or an explicit failure, and
the placeholder is neither.

### Correction, same day: that rule already existed, and was already wired

The paragraph above was written as though this were a new insight. **It was not.**
`knowledge::Returns::Count` carries the rule verbatim - *"A count, a length, a size. Zero is the
safe answer: a caller that loops over the result then does nothing, where a large value walks off
the end of a buffer"* - and `Returns::stub_value` returns `Some(0)` for it. The service consults
that on every unimplemented call.

So a function classified `count` would have answered **zero**, and no 2 GiB request would ever have
been made. The sizers answered the placeholder for a much duller reason: **they had no knowledge
entry at all**, so there was no `returns` to consult.

That relocates the finding, and makes it larger rather than smaller. The failure is not a missing
rule; it is that **the safety net only covers functions somebody has written down**. Across every
trace on this machine, **172 distinct functions are called and unimplemented, and 97 of them carry
no `returns` classification** - for all 97, the placeholder is handed to the caller whatever it
means to read it.

Classifying them from their names is not the answer and this project already knows it: of the four
whose names look size-shaped, `sceAgcDcbSetIndexSize` is a **setter** and
`sceKernelAprResolveFilepathsToIdsAndFileSizes` returns a status. Half the name-shaped signal is
wrong, which is D356's argument for refusing to classify prose.

## What was implemented

The five libSceUlt setup calls PPSA28061 makes: `sceUltInitialize`, both `GetWorkAreaSize`
functions, and both constructors. Arities came from the run - the third register on each sizing
call holds the placeholder left by an earlier stub, the evidence that fixed `sceKernelCreateEqueue`
(D516) and `scePthreadSetaffinity` (D523).

Sizes are now `0x1100` and `0xa80` - **4,352 and 2,688 bytes** where they were 2 GiB each.

**The work-area size is orbistoun's own number, not a measurement**, and this is defensible for
the same reason the Agc shader object would be: orbistoun implements the sizer *and* the
constructor, and stores nothing in the block - an Ult object's state lives in this crate's table,
the way every handle family here works. The number is chosen to be non-zero, so a caller checking
for a failed sizing sees success, and proportional to the request, so one sanity-checking "more
threads, more memory" is not surprised.

## What it did not change

**PPSA28061 still stops at 25 imports.** The abort is Agc's, not Ult's - the guest goes on to
`sceAgcDriverRegisterDefaultOwner`, `sceAgcCreateShader`, both answer the placeholder, and it calls
`abort`. That was clear before this was implemented and is unchanged by it.

So this bought no reach. It bought the removal of two 2 GiB allocations, a runtime and a pool that
actually construct, and the retirement of five entries from the work list. Constructing a runtime
is **not** running fibres on it: `_sceUltUlthreadCreate` remains unbuilt, and a guest reaching this
far then creating a fibre will stop there - which is a better place to stop.

## What this does not establish

**That the work-area size is adequate.** A console whose constructor validated the block would
reject one this small, and nothing here would see it - the constructor that reads it is also
orbistoun's.

**Nor what `sceUltInitialize`'s three arguments mean.** `0x1c`, a structure whose first word is
`0x18`, and `1`. Recorded, not interpreted.
