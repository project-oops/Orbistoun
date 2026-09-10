# D617 - A second batch, and the pipeline that was built for it

**Status:** measured
**Date:** 2026-09-08

## What arrived

Six files, timestamped `20260908-143022` - an hour after the batch D609 was built around, and
replacing it in the same directory. Three contexts again, and twelve sections nobody had captured
before:

```text
015-sync/mutex-unlock-unheld        102-net/listener
018-relational/allocations-do-not-overlap   102-net/nonblocking-option
018-relational/handle-fits-its-out-parameter 102-net/recv-would-block
080-video/attribute-block           102-net/resolve
101-input-ext/keyboard-held         102-net/sockaddr-bind
101-input-ext/mouse-moving          107-videodec/create-present
101-input-ext/reachability          108-audiodec/decode-present
110-modules/symbol                  135-sysctl/osrelease
```

**Nothing had to be built to read them.** `orbistoun-gen measurements` picked all three up from
the directory it already reads, `names --from-report` picked up the export table, and the coverage
gate failed with a list. That is what the morning's work was for, and this is the first time it
has been asked to do it unattended.

| | after D609 | now |
|---|--:|--:|
| distinct measurements | 374 | **429** |
| constant across every run | 262 | **304** |

Forty-two new constants, and **nothing previously claimed was demoted** - `nothing_claims_a_measurement_that_cannot_be_claimed` passed on the first attempt, which it did not last time.

## The nine worth having

Thirty-three of the forty-two are console addresses, that machine's handle numbering, or how long
the probe waited for an operator. Opaque, on the same terms as before.

The other nine are a subsystem orbistoun **has not started**. None of `sceNetSocket`, `sceNetBind`,
`sceNetListen`, `sceNetAccept`, `sceNetConnect`, `sceNetRecv`, `sceNetSend` or `sceNetSetsockopt`
is declared here, and the console answered for all of them:

```text
sceNetBind        bound          0x0
sceNetListen      listening      0x0
sceNetSetsockopt  0x1200-accepted 0x0
sceNetRecv        connected-return  0x80410123
sceNetRecv        listener-return   0x80410139
```

**That last pair is the reason to record a measurement before writing the function.** A socket
with nothing to read answers one code when it is connected and a *different* one when it is
listening - a distinction a reimplementation flattens without noticing, because both read as
"would block". And the `0x8041` prefix is libSceNet's own error space, not the `0x8002` kernel
one that every existing vendor code in this project uses.

Written into `libSceNet.toml` as `measured`, citing the check, for a function that does not exist
yet. That is the inversion this whole apparatus is for: the oracle arrives first and the
implementation is written against it, rather than being written against a guess and corrected
when a title disagrees.

## The export table did not move

2,443 exports in this batch, 215 of them unnamed - the same counts as the batch an hour earlier,
and the affix search found nothing new. Whether the *addresses* also match is unverified: the
previous batch was replaced rather than archived, so there is nothing left to compare against.

Worth noting as a limit of the arrangement rather than a result. A report directory that is
overwritten cannot answer "did this change", which is the question two captures exist to answer.
