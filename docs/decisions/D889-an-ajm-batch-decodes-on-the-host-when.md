# D889 - An Ajm batch decodes on the host when it is started

**Status:** decided
**Known by:** assumed
**Date:** 2026-10-15

`sceAjmBatchStart` runs every decode job its batch holds to completion before it answers: each
job's input is read from guest memory, its MPEG audio frames decoded on the host, and as much of
the result as fits written to the job's output as interleaved 16-bit PCM. The batch's identifier
goes into the first four bytes of the start's out block, numbered from 0, and `sceAjmBatchWait`
answers `0x0` the first time for an identifier handed back, since the batch it names has already
finished, and `0x80930004` after.

**Why:** Grand Theft Auto V decodes its audio through Ajm: codec 0, one batch per job, started and
waited on in turn. With the batch set-up answered as measured (REQ-ai02) it retries
`sceAjmBatchStart` on the placeholder; answered `0x0` under a diagnostic override, it goes on to
wait on the identifier the out block still held, `0xffffffff` - so the start's out block carries the
identifier the title waits on.

The output format is the title's own arithmetic. Its input begins `ff fa 64 c0`, an MPEG-1 Layer
III header for 80 kbit/s, 48 kHz, mono (ISO/IEC 11172-3), whose frames are 240 bytes: the 960
bytes it passes are four of them, and four frames of 1152 mono samples at two bytes each are the
9216 bytes of output it passes. Decoding happens through `symphonia-bundle-mp3`, a pure-Rust MPEG
audio decoder, MPL-2.0.

Measured since (REQ-gt02, `20261016-000300-eboot.log` 4956-4962): in the title's shape the start
answers `0x0` and writes identifier 0 for the first batch, and a second wait on a batch answers
`0x80930004`. The harness's job failed - its first wait answered `0x80930011` with an error block -
so what the first wait answers for a job that decoded, and that samples are 16-bit, are still
assumed. ai02's refused start was a three-argument call, not a missing privilege.

**Rejected:**
- Answering the start and the wait with no decoding: the title would play silence it believes is
  sound, and the next wall would be one orbistoun made.
- Decoding on a thread of its own, finishing later: nothing measured says when a batch finishes,
  and finishing it at the start makes every wait answerable at once. A title that polls a wait
  sees the batch done the first time, which a real batch could also be.
