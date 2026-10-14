# D840 - A resolution is checked against the rows of the logs it cites

**Status:** decided
**Date:** 2026-10-14

`orbistoun-cli resolutions <worklog> --reports <obSCEne repository>` reads every resolved request
orbistoun filed and checks the outcome against the `OBS|` rows of the logs the request cites,
taken together:
- Each status code the outcome names (`0x8` and seven more hex digits) must be held by a row.
- No platform function it names (`sce` and a capital) may have only rows saying it did not bind.

A resolution failing either test is printed and filed again with the rows that contradict it,
not ingested, as is one that cites no log or whose log cannot be read. A function no row names
is printed as a note only. A probe may label a call's rows by a key of its own
(`ime-dialog|abort-rc`), so the absence of the name is not by itself evidence.

**Why:** on 2026-10-14 a single sweep resolved 72 requests. Sixty-six of them carried one of four
pasted outcome paragraphs, and the log they cite backs none of those paragraphs:
- No row holds the NP refusal codes `0x80550001` / `0x80550002`.
- Every GPU claim (HTILE, DCC, BVH, `SubmitAcb`) is absent from the rows.
- `sceAvPlayerInit`, said to answer 0, is `bound 0x0`.

The check had been done by hand, log by log; as a command, the loop's bus step performs it on
every resolution (D832).

**Rejected:**
- Checking only that the cited log exists: every one of the 66 cited a log that exists.
- Checking claims log by log: a request citing two sweeps would be reported for the claim
  the other sweep backs.
- Treating a function no row names as decisive: md02's dialog rows are keyed `ime-dialog` and
  `error-dialog`, and they back its outcome.
- Ingesting a resolution's prose as `assumed`: an assumption is written by orbistoun with its
  reason, not taken from an outcome that names no row.
