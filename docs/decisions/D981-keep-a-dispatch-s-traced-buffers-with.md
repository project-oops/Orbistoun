# D981 - Keep a dispatch's traced buffers with the guest memory they were worked out from

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A dispatch's traced buffers - what `Pipeline::dispatch_buffers` answers - are kept with what the
tracing read from guest memory, and served again while memory still answers it the same. The
queued writes of a submission (D876) and the command processor's preparation of the same dispatch
ask the same question, and for a program whose stores go through a descriptor it computes, the
answer is a host run of a sample of its waves (D753, D914, D951).

**What was measured.** Grand Theft Auto V's in-world loop submits seven dispatches of one program
together, about every 40 to 55 s. Its store descriptor comes from a table indexed per wave, so each
is bound from a 256-group sample run on the host: 1.6 to 2.4 s a dispatch, about 12 s for the seven.
The guest thread paid that in its queued writes, holding the live pipeline. Then the command
processor paid it again preparing the same dispatches. Each cycle held every preparation for about
24 s. The command processor's draw segments waited on the live pipeline meanwhile. The wait showed as 11 to 12 s in
"run draws: streamout", the first span a segment opens. No streamout draw ran in either run: the
time was the segment's check of whether streamout was on, waiting for the pipeline.
`with_registers` and `prepare_state` now count that wait as "live pipeline: wait" too, and
"dispatch: buffers" and "settled read" name the tracing and its reads.

**What is kept.** `dispatch_buffers_kept` keys an answer by the program's address, a hash of its
bytes, and the whole `DispatchState`. The work reads through a recording `GuestMemory` that notes
three things. It notes each span whose bytes it read, merged where spans touch, with a hash of
those bytes. It notes each span it only asked was mapped (`GuestMemory::mapped`: a margin checked
before it is bound, a range checked for a refusal's note). It notes each span it found unmapped.
A later ask is served when every read span hashes the same and every asked span is mapped, or not,
as before. Anything else works the answer out again. Refusals are kept as answers too. Sixty-four
answers are kept, newest last. An answer whose reads cannot be read back, or that read more than
65,536 spans, is not kept.

**Why presence is apart.** The sampled range's margin is checked by reading the whole 42 MiB range
the dispatch binds. If those bytes counted, the first of the seven dispatches storing into the
buffer would change them, and the other six would never be served.

**Measured.** On a 480 s GTA V run, before and after, the program's slow preparations on the
command processor fell from 49 to 21. Cycles where the command processor was served stall about
12 s, not 24 s. The guest thread's queued writes still run the sample once a cycle, about 12 s.
Amended by D982: where the backend runs device-addressed dispatches, such a dispatch is prepared
device-addressed and runs no sample at all. The memo keys on whether host runs are allowed.

**The test.** `a_dispatch_s_buffers_are_kept_until_what_they_read_changes` stores through a
descriptor loaded from a table in guest memory. It asks for the write spans twice over the same
memory and gets the second answer with fewer reads. With the table rewritten to name another
buffer, the spans move to it. `buffers_memo`'s own tests check that a byte read invalidates,
that a byte only asked to be mapped does not, and that repeated and touching reads merge.

**Rejected:**
- Handing the queued writes' answer to the command processor whole: an earlier dispatch in the
  same submission can write what a later one's tracing read, which only a check of those bytes
  finds.
- Reporting such a dispatch's writes as unbounded so only the command processor runs the sample:
  the command processor holds the live pipeline while preparing, so the guest waits as long.
- Keying the answer on a guest write watch: a watch cannot tell a page whose bytes the tracing read
  from one it only asked was mapped.
