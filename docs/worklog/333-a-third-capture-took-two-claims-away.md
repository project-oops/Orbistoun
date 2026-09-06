# 2026-09-03 - (/loop) A third capture added 186 measurements and took two claims away

```
tests         1974  ->  1975
measurements    38  ->   224   (200 constant)
claimed         19  ->    41
outstanding      7  ->   147
```

A third hardware capture arrived - a **native title-eboot run**, 569 checks in 40 sections,
firmware 12.40, carrying 266 `measure` records against 34 and 73 in the two existing captures.
Regenerating the table turned three tests red within a minute, which is the coverage gate
doing the job it was built for (D478).

## The counter frequency is calibrated per boot (D485)

`the_counter_frequency_matches_the_console` asserted equality against one number and failed:

| runs | value | |
|---|---|---|
| the two earlier captures | `0x5f259b8e` | 1,596,300,174 Hz |
| this one | `0x5f259bb6` | 1,596,300,214 Hz |

Forty hertz in one and a half gigahertz. **Not jitter** - the two earlier runs agree with each
other to the hertz, and a noisy reading does not repeat. What repeats within a session and
moves between them is a calibration.

So the claim was the wrong *shape*, not the wrong number. Nothing may assert a value that
varies - and `nothing_claims_a_measurement_that_cannot_be_claimed` now refuses to let it be
listed even as outstanding, because there is no state in which orbistoun "gets it right".

But *"orbistoun answers a frequency no console ever reported"* is still a defect, so the
assertion became **membership**: the answer must be one of the values a run actually measured,
and the two names must still agree in every run. `Measurement::values()` was added for it, and
generalises to all twenty-four non-constant measurements.

## Two hand-written classifications became mechanical ones

`OPAQUE` carried this, reasoned rather than measured:

> `app0-libc` - 0x15 is the console loader's own handle numbering, not a property of the call

This run answered `0x14`. **The reasoning was right and is now measured** - and the entry is
gone, because the fold marks it non-constant on its own. `app0-fios2` went further and
answered `0x80020063` where it had answered a handle. Four entries left the lists this way,
replaced by a mechanism that updates itself when the next capture lands.

## The ctype tables, confirmed and now claimed

The committed tables came from a single capture. This one re-read all three from a **different
build form** and reproduced every one of the 22 spot-checks exactly.

So they moved from unasserted data into `CLAIMED`, with a test that reads them **the way a
guest does** - calling `_Getpctype()` and indexing the answer, rather than reading the data
file - which covers installation and the margin below zero as well as the values. Watched
failing by breaking `pctype[9]`:

```text
_Getpctype[9] is 0x4c1, the console carries 0x4c0
```

## 177 measurements classified

The gate demanded every new constant be asserted or declared with a reason. Notable groups:

- **105 encoder items** - path probes, symbol resolution and sysmodule loads for the video
  encoder libraries. Outstanding; orbistoun has no encoder subsystem, and principle 6 puts it
  well after the address space. The path probes are the interesting ones: orbistoun *does*
  now have the measured 537-module manifest, so answering them is real work rather than a
  missing subsystem.
- **12 cpuid items** - opaque, and for a reason that is a principle rather than an excuse:
  guest instructions run natively, so `cpuid` answers the host's CPU. Presenting the
  console's would need the instruction trapped, and interception here is linking rather than
  hooking (principle 7).
- **5 MXCSR items** - outstanding, and the most actionable thing in the capture. The console
  enters a title with **DAZ and FTZ both set** and all six exceptions masked; orbistoun enters
  the guest with whatever the host thread had. A denormal argument reads as zero on the
  console and does not here, so float results can differ without any function being wrong.
  Completion: set MXCSR before the entry jump.
- **3 memory-type refusals** - the console refuses onion/garlic combinations to a title-level
  process; orbistoun's direct-memory allocator does not model the split and accepts them.

## The capture itself is not committed

Written into `obscene/data/hardware/` first, then taken back out. A capture is an
**observation, not a source**: unlike the FreeBSD checkout the constants are harvested from,
it cannot be re-fetched at a pinned revision, and it is not deterministic - which is the whole
of D485. The generated table is the committed artefact, exactly as the harvested constants
table is, and `check` reads that rather than the captures.

Whether the captures should be committed *as evidence* under a naming standard is open with
the user, along with the observation that the two existing ones carry no `OBS|context` record
to name a file from.

## State

`cargo test --workspace` green - **117 suites, 1975 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. obSCEne left exactly as found.

Nothing committed. The day holds worklogs 292-333 and D466-D485.

**Next**: the shared stub table (D484) - `build_thunks` over a set of modules, the
`(module, symbol index)` to slot mapping carried to relocation, then the relocation pass.
