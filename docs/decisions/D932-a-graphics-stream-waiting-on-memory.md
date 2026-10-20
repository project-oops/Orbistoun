# D932 - A graphics stream waiting on memory goes on once the word holds

**Status:** decided
**Date:** 2026-10-20

A graphics stream that stops at a memory wait (`WAIT_REG_MEM`) not yet met is kept, with every
stream handed to the queue after it. Whenever no new work is waiting, the command processor polls
that wait again, every millisecond. Once the word holds, the stream goes on from the wait, and the
streams held behind it then run in order until one stops the queue again. A stream stopped for
any other reason still stops the queue for good (D710).

This amends D710's rule for memory waits. D710 assumed that nothing runs concurrently to satisfy a
memory wait. The title's own threads do, and so does a hardware command processor, which polls a
`WAIT_REG_MEM` until it holds. The poll interval is `assumed`; the console's is not measured, and
only how promptly the queue goes on depends on it.

**Why:** Grand Theft Auto V's graphics submission 7001 waits for a word its CPU sets
(0x7400_4314_5900, reading 1332 against 1333), and nothing on the GPU writes it. The render thread
waits for frame 7001 to finish, so no later submission arrived to carry the queue on. The run
ended there with every one of its 7000 earlier submissions carried out.

**Rejected:**
- Polling only as each new submission arrives. The title submits nothing more until the stopped
  frame finishes, so that poll never comes.
- Waiting on the command processor's thread until the word holds. A wait no store ever meets would
  then hold that thread forever, and the compute streams stepped aside (D891) would never be
  carried on.
