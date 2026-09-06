# 2026-09-02 - (/loop) Bulk port batch 12: the lock-attribute families - and my stop recommendation was wrong

```
documented   715 needed, 264 missing   ->   715 needed, 243 missing
```

Twenty-one entries. **Worklog 312 recommended stopping the batches on the grounds that the cheap
wins were exhausted. The user said keep going, and the very next batch moved the number by
twenty-one.** That recommendation was wrong, and the reason is worth more than the code.

## Why the recommendation was wrong

Worklog 312 measured the wrong thing. It ran one check - "is any of this already written and
merely unwired?" - got three false positives and zero real hits, and concluded the seam was
mined out. But "nothing is already written" says nothing about **how cheap the unwritten work
is**. The barrier and read-write-lock attribute families were sitting in plain sight in the same
list, following a pattern this crate had already established four times over.

The honest version of that tick's finding was narrower than what it claimed: *no function
remains that needs only wiring*. It generalised that into *the remaining work is expensive*, on
no evidence. That is the same over-claim principle 3 keeps catching, in a recommendation rather
than a report.

## What went in

All in `orbistoun-kernel`, all following `pthread_attr_init`'s shape - this crate allocates the
object, the guest holds a handle, accessors read and write fields by offset:

- `pthread_barrierattr_{init,destroy,getpshared,setpshared}`
- `pthread_rwlockattr_{init,destroy,getpshared,setpshared,gettype_np,settype_np}`
- `pthread_yield`, `sched_yield`
- `pthread_getconcurrency`, `pthread_setconcurrency`

with the `posix_`-prefixed twin of each delegated to it (D475).

Three things worth stating:

- **`pthread_getconcurrency` answers zero unless the guest has set a level**, and that is the
  standard's own answer rather than a stub: it is a hint to an implementation that multiplexes
  threads, this one does not, and zero means "the system decides". The setter records the value
  purely so the pair agree with each other; nothing reads it.
- **`sched_yield` is a hint by definition** - the scheduler may run the same thread again
  immediately - so handing it to the host scheduler is the whole of the contract, not an
  approximation of it.
- The attribute destroys **do not reclaim the object**, matching the existing ones: a guest may
  destroy an attribute while something built from it is still live, and freeing here would leave
  that reading freed memory.

## State

clippy `--tests` clean, fmt clean, kernel/posix/libc tests pass, identity scan clean, nothing
committed.

**Still ahead**: 243. The remaining clusters are thread cancellation (`pthread_cancel`,
`setcancelstate`, `testcancel` - which need a cancellation model, not just storage), the timed
waits (`pthread_mutex_timedlock`, `pthread_cond_timedwait`, `sem_timedwait`), the scatter/gather
socket calls (`sendmsg`/`recvmsg`/`sendto`/`recvfrom`), and `pthread_once`. Several of those are
ordinary work; the cancellation set is the one that needs a design rather than a spec reading.
