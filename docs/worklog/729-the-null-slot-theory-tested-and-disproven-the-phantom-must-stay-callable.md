# 729. The null-slot theory, tested and disproven: the phantom must stay callable

**2026-09-20** — a hardware-grounded theory said orbistoun might be walling PPSA02664 by answering a call
the console leaves null. Tested directly, it is false, and the test settled a paradox that had been open
since worklog 728. No code changed - the value is the ruling and what the knowledge now records.

## The theory, and why it was worth testing

obSCEne `e245` measured that NID `0x7d86501b8094ef57` is not a retail export: a title importing it has the
slot bound to null. PPSA02664 imports it, and renders on a console. orbistoun, unlike the console, answers
it - `agc_phantom_get_size` returns a size. The theory: PPSA02664 checks that slot for null and, on the
console, takes a fallback path when it is; orbistoun's non-null answer defeats the check and drives the
title into the `CreateWorkload` path that dies at the null-container `memcpy`. If true, binding the slot
null - exactly what the console does - would let the title's own null handling clear the wall. That is
"provide the same API the real device would" taken literally, and D632 already frames it as a flag to
measure rather than a change to argue.

## The test, in two steps

First, unregister the handler. The NID then resolves to a placeholder (it is a non-weak import, so
orbistoun does not bind it zero), and the fault was **identical** - `read of 0xa8`, verdict same. A
placeholder is non-null, so this only confirmed the phantom's binding does not matter *as long as it is
non-null* - a third independent ruling-out after worklog 725 killed its return value and its out-param.

Second, the real test: force the slot to exactly `0x0`, through the executable's `weak_zero` set in
`relocate_the_executable`. The fault **changed and moved backwards**: `instruction fetch from 0x0`,
verdict BACK. PPSA02664 `call`s the slot with no null-check of its own - a null slot sends it to address
zero and it dies at the call, earlier than before. The theory is disproven: the title does not guard this
call, so it cannot take a fallback on null; it needs the slot callable to get as far as it does.

## What that settles

The e245 paradox - the NID is null on retail, yet PPSA02664 renders - resolves cleanly. e245 measured a
*bare synthetic* import (a probe title declaring the NID and nothing else), and its probe declined to call
the null slot because obSCEne's probe null-checks first (that is the `call-executed 0x0, slot-is-null` row,
which last tick I read as "the platform does not make the call" - it is the probe's caution, not the
platform's). PPSA02664 does not null-check. So on the console PPSA02664's slot is **not** the bare null a
synthetic import gets - it reaches this size through working code, the SDK helper inlined or otherwise
available to it, and orbistoun answering the call with a size is what keeps the title on the path it takes
on hardware. Keeping `agc_phantom_get_size` callable is correct, not a divergence.

The phantom is now ruled out as the wall four independent ways - return value (725), out-param (725),
unregistration (this tick), and null binding (this tick). The wall is downstream of it, at the null
container that arrives as an argument to `0x42d90` from a caller one level up (worklog 725's open thread),
and that is where the next PEEK belongs.

## Why record a negative result this carefully

Because the null-slot idea is the obvious thing to try, e245 makes it look right, and it costs a full
experiment to find it wrong. The phantom's knowledge now carries the ruling - "MUST STAY CALLABLE, the
null-slot theory is disproven (729)" - so the next session reads the answer instead of rebuilding the
binding to rediscover it. That is the same reason worklog 727 wrote down which guest a syscall belonged
to: a wrong-but-tempting conclusion is worth the words that stop it being drawn twice.

## Gate state

No net code change: both diagnostics (the unregistration in `orbistoun-gpu/src/agc.rs`, the forced
`weak_zero` in `orbistoun-worker/src/lib.rs`) were reverted after measuring, and the run confirms
PPSA02664 back at its recorded `read of 0xa8`. The only durable change is
`crates/orbistoun-hle/data/knowledge/libSceAgc.toml` - the phantom entry's two edge cases refined to
record the ruling. `./bin/orbistoun check` green; identity scan clean. No commit.
