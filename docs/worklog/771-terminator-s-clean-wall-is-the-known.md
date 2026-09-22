# 771. Terminator's clean wall is the known int 0x41 chain, not the recorded AGC site; the one active stub is sceUserServiceGetAgeLevel, a real export now probed

**2026-09-21** — the retail-frontier loop's rotation reached PPSA25872 (Terminator: Resistance). Checking
the existing record before re-deriving (the lesson from the redundant AGC requests) showed its wall is
already understood - so this is a short confirmation plus the one concrete, non-redundant step it leaves:
a probe for the single real export the run still stubs.

## What the clean run shows

`docs/compat-frontier.txt` records PPSA25872 at `image+0x3b383b` (339,539 calls) - the AGC vertex
attribute-marshalling loop worklog 529 advanced it into. A clean run today instead faults at
`image+0x17554a3` (321,970 calls), a `read of 0xffffffffffffffff` the report reads as an **int 0x41
Il2Cpp assertion** (D384: a ud2/privileged abort, not a real read of -1). Fewer calls than the record,
so it is a **regression** the "keep best" record refuses - the working tree's compat is unchanged, and
this matches the PPSA25872 regression already flagged in worklog 673.

The report's own guidance holds: `int 0x41` is not orbistoun's to implement (obSCEne measured it fatal on
hardware too, REQ-...b3c2); the guest reaches it via an upstream wrong value. The calls just before are a
`strlen`/`memcpy` of a 0x308-byte string into an error-string format (`"%s"`, "Hostname Lookup...").

## This wall is already mapped

`crates/orbistoun-hle/data/knowledge/libSceAppContent.toml` and worklog 673 already state it: the assert
at `image+0x17554a3` is "reached through a chain of unimplemented stubs (`sceUserServiceGetAgeLevel` and
others) and an error-string format - a cause deeper than any one placeholder." Implementing any single
stub (AppContent was tried) closes an honest gap but does not move this wall. So the wall is a multi-stub
chain plus an error path, not a one-symbol fix.

## The one concrete step

The clean run's single stubbed call is `libSceUserService::sceUserServiceGetAgeLevel` (declared arity 2
in `orbistoun-systemservice`, no handler). Unlike the AGC cluster's inline non-exports, this is a **real
libSceUserService export**, so a call-probe reaches it - and it is the age-check class we agreed hardware
should answer rather than a guess. It is not in the knowledge files or on the bus, so the request is not
redundant. An obSCEne request is filed for its rc and the age-level it writes, so orbistoun answers the
measured value and the chain advances by one honest step (the next stub then surfaces).

## Gate state

No code changed - a confirmation that spends the "check first" lesson, records the clean wall as the known
`image+0x17554a3` chain (worklog 673) rather than the recorded AGC site, and files the one warranted probe
(`sceUserServiceGetAgeLevel`). `./bin/orbistoun check` green, worklog index regenerated, identity scan
clean. No commit.
