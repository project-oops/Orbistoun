# D509 - The console writes eight bytes where documentation said four

**measured** - 2026-09-03 (`live_launch.txt`, and a guard word)

## The reversal

D210 narrowed `sceKernelCreateSema`'s write to four bytes:

> The destination is an `int *`, not a `void **` - a semaphore handle and a mutex handle are
> different shapes, which obSCEne established **from the public interface documentation**. This
> wrote a full word until then, so every `sceKernelCreateSema` put four bytes of handle into
> whatever the guest kept next to its semaphore.

A conformance check plants `0xA5A5A5A5` in the word after an `int handle`, calls
`sceKernelCreateSema` on the `int`, and reads the guard back. It reads **`0x0`**, and obSCEne's
own verdict is *"the call wrote past the end of the int it was given"*.

So the console does the thing D210 called a bug. Orbistoun now writes eight bytes.

**This is the oracle ordering working rather than being overridden.** `Measured` outranks
`Published` in this project's vocabulary precisely because a documented layout and a real one
have diverged here before - D468, where the ctype tables were written from FreeBSD's documented
layout and hardware answered differently.

**A guest is built against the console.** Writing four bytes leaves a neighbour holding a value
the console would have cleared, and a title that has allowed for the clobber sees a difference
orbistoun cannot tell it about.

The high half is written as zero because that is what the guard read. Whether the console writes
a 64-bit handle whose upper half happens to be zero, or writes four bytes and clears four more,
is not distinguishable from one guard word - and both produce this. The test asserts the guard,
not the handle: the handle is orbistoun's own number and says nothing about the platform.

## And the mutex attribute round trip, which needed no capture at all

Six measurements sat outstanding saying they *"need settype/gettype round-tripped through one
attribute object"*. The check already does that, and the capture already carried the answer:

```text
default 1     0 -> refused     1 -> 1     2 -> 2     3 -> 3     4 -> 4
```

All six claimed. **Orbistoun accepted type 0 and stored it**, where the console refuses it - it
had no range check at all - so a guest asking for a type the platform will not give it was told
it had one. Now refused.

**The refusal is measured; the code is not.** No run recorded what `Settype` answers for zero,
so orbistoun answers its own placeholder rather than a vendor code invented to fill the gap.

**And the refusal is asserted as behaviour, not as its recorded value.** `type-0-read-back` is
`0xffff_ffff_ffff_ffff`, which is the probe's marker - its comment says each entry records what
`Gettype` read back *"or -1 where `Settype` refused the type or `Gettype` failed"*. Asserting
orbistoun answers `-1` would be asserting against the instrument, which is D497's mistake. The
test asserts what the marker encodes: after `Settype(0)`, the round trip must not succeed.

## What this says about the backlog I wrote

Both of these were entries in obSCEne's backlog 022 asking for work that **already existed**. I
wrote them without reading the capture, from the outstanding reasons alone - and the reasons
were written when the checks were younger.

Withdrawn there, with the correction stated. The lesson is narrower than "read the capture": it
is that **an entry in a work queue is a claim with a date on it**, and the queue is the last
place to learn that the work was done. Seven stale work items this week, and this is the first
pair where the staleness was mine.
