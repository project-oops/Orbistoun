# D735 - Logical time is per thread

**Status:** decided
**Date:** 2026-09-28

The logical clock (D582) is kept per guest thread. A thread's time advances by its own readings
and its own waits only; another thread's readings and waits do not move it. A guest thread starts
at the time its creator had when it created it, so no thread reads a time earlier than the one
that made it. Every clock the guest reads still comes from `clocks::since_start_nanos`, so one
thread's process time, tick counter and monotonic clock stay consistent. Under
`ORBISTOUN_CLOCK=host` nothing changes.

**Why:** one shared counter moved by every thread made the main thread's readings depend on when
the host scheduled the others. Bugdom's SDL audio thread advanced it by 21.3 ms at each buffer it
queued, at host-timed moments, and two runs of one build parted after about ten seconds. With the
clock per thread they agree for about forty. A comparison between runs, and a compatibility
record anyone can reproduce, needs runs that repeat.

**Limits:** a guest whose logic waits on another thread's progress still depends on the host's
scheduling. Bugdom's logo screen runs until its song has played, and the song plays on the audio
thread at the host's pace, so runs part again after a transition that waits on audio. Making that
repeat needs deterministic scheduling of guest threads, which this does not attempt. Clocks on two
threads can disagree; a guest comparing timestamps across threads sees the difference.

**Rejected:**
- Only device waits stop moving the shared clock: fixes Bugdom's audio thread, and leaves any
  title whose worker threads read the clock or sleep diverging the same way.
- Lamport merges at every synchronisation (a woken thread taking its waker's time): consistent
  across threads, and repeats only where the order of synchronisations does, which the host's
  scheduler still decides.
