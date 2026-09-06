# D497 - Half the encoder probes measure the probe's own initialiser

**measured** - 2026-09-03 (48 measurements, 24 of them claims)

`106-encoder/path-probe` carries 48 constant measurements - 24 paths, each with a `handle` and
a `res`. They looked like 48 things orbistoun could be checked against. **Twenty-four of them
are not claims about the platform at all.**

## What the probe does

```c
int res = 0;
int h = sceKernelLoadStartModule(search_paths[i], 0, (void *)0, 0, (void *)0, &res);
obs_report_measure(..., "handle", (uint64_t)(uint32_t)h, "handle");
obs_report_measure(..., "res",    (uint64_t)(uint32_t)res, "code");
```

`res` is **initialised to zero by the probe** and then reported. Every one of the 24 reads
`0x0`, and a console that leaves the out-parameter alone produces exactly that reading, as does
one that writes zero into it. The two are indistinguishable, so the measurement separates
nothing.

Recorded in `OPAQUE` with that reason rather than left in the work queue, where it would have
read as twenty-four things still to do.

**This is the same shape as D485 and D486**: a value that was measured is not automatically a
property of the platform. There the confounds were a per-boot calibration and a sticky status
bit; here it is an out-parameter the caller pre-filled. Three different mechanisms, one
question - *could this reading have arisen without the platform doing anything?*

## The other 24 are real, and orbistoun already agrees

Every path answers `0x80020002` - the vendor encoding of `ENOENT`. Asserted by calling
`sceKernelLoadStartModule` with each path and comparing **at thirty-two bits**, which is the
width the probe took it at: it reports `(uint64_t)(uint32_t)h` from a prototype returning `int`,
so the upper half is obSCEne's cast rather than the console's answer (D480).

### Two of the four directories are right for the wrong reason

`/system/common/lib/` and `/system/priv/lib/` are in `FIRMWARE_MODULE_DIRECTORIES` and are
refused *because this project knows what they hold*. `/system/sys/lib/` and `/system/lib/` are
in no table and reach the same code by falling out of the bottom of the function into the
unrecognised-path refusal.

Same value, different reason - and that constant's own comment already records the hazard
biting once, when `/system_ex` was refused by luck across 234 modules. Asserted anyway, because
a guest cannot tell the two apart and the answer is what it sees; noted in the test so the next
reader is not surprised when a table change moves half of them.

## The guard was watched failing

Changing the unrecognised-path refusal from `ENOENT` to `EINVAL`:

```text
/system/sys/lib/libSceVencCore.sprx answered 0x80020016, the console answered 0x80020002
```

Twelve of the twenty-four, which is the half that reaches the fallback - so the failure is
attributable as well as present. The test also asserts it compared exactly 24, because a loop
that quietly compared fewer would pass by doing less.

## Closed on the other side

obSCEne now poisons that out-parameter with `0xC7C7C7C7` rather than zeroing it (its D303), so
"the platform never wrote here" and "the platform wrote zero" stop reading the same. The next
capture turns twenty-four non-claims into twenty-four measurements, whichever way they fall.

The twenty-four here stay `OPAQUE` until that capture exists. A reading is not retroactively
informative because the probe that produced it has since been fixed.
