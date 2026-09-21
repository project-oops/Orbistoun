# 753. The title is heavily threaded, so the determinism path closes — and an oracle opens

**2026-09-21** — the last assessment 752 left open: could deterministic guest execution stabilise the
descriptor's address enough to watch its construction? The thread census answers no, and definitively.
But ruling that out surfaces an angle the whole investigation has not used — the reference emulator as
an oracle for the *value* orbistoun gets wrong, which is a lead rather than another wall.

## Determinism is not on the table

The title's run census: **178 `scePthreadCreate`**, and `pthread_mutex_lock`/`unlock` at **113,714 each**
(6.5% of all calls apiece), plus 22,561 `scePthreadGetthreadid`. This is Unity's IL2CPP job system in
full flight — dozens of threads contending on locks throughout. The descriptor's address varies run to
run (749) because the count of allocations before it varies with thread interleaving, and making that
deterministic would mean a deterministic scheduler for the entire engine — reproducing one exact
interleaving across dozens of racing threads. That is not a tick's work and may not be achievable at
all; it is certainly not the path to this one descriptor. The construction watchpoint is closed for
good, not merely deferred.

## The oracle angle, not yet tried

Every path so far has tried to read orbistoun's own run — the trace, the heap, the guest code. The one
source not consulted is **prosper**, the independent PS5 emulator credited in `ACKNOWLEDGEMENTS.md`. The
provenance rule is exact about how it may be used ([[emulator-is-oracle-not-source]]): never a source to
transcribe, but a legitimate oracle for *what a correct value is*. And this wall is precisely a
wrong-value question — Unity's inlined loader relocates three of five because it reads some orbistoun
input that hardware answers differently.

If prosper runs PPSA02664 past `CreateWorkload`, then at the point orbistoun's loader produces a raw
`0xa8`, prosper's produces a real pointer — which means prosper answered whatever the loader reads to
decide the relocation count differently from orbistoun. That difference is the misread input, and
finding *which orbistoun-provided value* differs (a `stat` size, a header field the loader trusts, an
allocation result) would name the gap without ever reaching the inlined loader's code. orbistoun would
then fix that one value from its own measurement, prosper having only pointed at *which* value to
re-check — the oracle discipline exactly as 740 practised it on the AGC hypothesis.

This is the next concrete lead: determine whether prosper runs this title, and if so, compare the two
emulators' answers around the shader-header construction to isolate the input. It is more promising than
the closed watchpoint and cheaper than a hardware probe, and it keeps within the boundary — a number to
re-measure, never a line to copy.

## State

`create_shader`'s full-array relocation is landed and tested (751); the probe that would confirm it is
written for a person (752); the construction watchpoint is closed (this tick); and the remaining
root-cause question for PPSA02664's inlined path has a fresh, in-boundary lead in the oracle comparison.

## Gate state

No code changed — a thread-census read and analysis. `./bin/orbistoun check` unchanged from 752 (green
but for the same three generated-doc drifts from a prior session's uncommitted
`compat/PPSA02664-app0.toml` edit, not this tick). Identity scan: this worklog's first title used the
title's proper-noun name, whose surname is on the identity deny list (only the full game name is
allow-listed), so the generated index line tripped the guard; renamed and reworded to the neutral "the
title", and re-scanned clean. No commit.
