# 2026-09-03 - (/loop) Eight bytes where documentation said four

```
CLAIMED   77  ->  84        OUTSTANDING   67  ->  60
tests           1998
```

Asked whether we are waiting on anything from obSCEne. **We are not, and finding that out
closed seven measurements.**

## Both of my asks in backlog 022 were already answered

I had written two entries asking obSCEne for work: a mutex attribute round-tripped through one
object, and a guard word after the handle out-parameter. **Both checks already existed and the
capture already carried their results.** I wrote the asks from the outstanding *reasons* rather
than from the capture, and those reasons were written when the checks were younger.

Withdrawn from 022 with the correction stated.

## The console writes eight bytes, and D210 said four

`018-relational/handle-fits-its-out-parameter` plants `0xA5A5A5A5` in the word after an
`int handle`, calls `sceKernelCreateSema` on the `int`, and reads the guard back as **`0x0`** -
obSCEne's own verdict being *"the call wrote past the end of the int it was given"*.

D210 had narrowed orbistoun's write to four bytes **from public interface documentation**. The
console does the thing that decision called a bug, so orbistoun now writes eight. That is the
oracle ordering working: `Measured` outranks `Published` because a documented layout and a real
one have already diverged here once (D468, the ctype tables). A guest is built against the
console, so four bytes leaves a neighbour holding a value the console would have cleared.

The test asserts the **guard**, not the handle - the handle is orbistoun's own number and says
nothing about the platform. Watched failing by narrowing the write back. Three guest runs after:
the same bimodal 2077/2080, no regression.

## The mutex round trip needed no capture at all

Six measurements said they *"need settype/gettype round-tripped through one attribute object"*.
The check does that, and the answer was in the file: `default 1`, `0` refused, `1`-`4` reading
back as themselves.

**Orbistoun accepted type 0 and stored it** - no range check at all - where the console refuses
it. A guest asking for a type the platform will not give it was told it had one. Now refused;
the refusal is measured, the code is not, so orbistoun answers its own placeholder.

**The refusal is asserted as behaviour, not as its recorded value.** `type-0-read-back` reads
`0xffff_ffff_ffff_ffff`, and the probe's comment says that is its marker for *"`Settype` refused
the type or `Gettype` failed"*. Asserting orbistoun returns `-1` would be asserting against the
instrument - D497's mistake - so the test asserts that the round trip must not succeed.

## State

`cargo test --workspace` green - 119 suites, **1998 tests**, 0 failures. Differential 247 cases.
clippy `--tests` clean, fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-358 and D466-D509.

**Next**: `strtok_r` and the `sprintf` family in the differential; the 13 imports that name data
rather than a function.
