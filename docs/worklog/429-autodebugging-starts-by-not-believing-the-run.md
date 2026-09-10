# 429. Autodebugging starts by not believing the run

**2026-09-08** - directed, continuing 428

## What was done

The loop already dispatches: `orbistoun-turn` maps every finding the report can name to a fixed
step and runs the mechanical ones. What it did not do is **check that the run it is comparing
against is comparable**, and every step it takes is a comparison (D583).

`Step::CheckRepeats` now heads every plan. Two runs, nothing applied, compared on the two signals
every other step uses.

## It withdrew a finding on its first run

```text
14 finding(s), 8 step(s), 7 of them mechanical
  *** two runs of this build DISAGREE - 192 imports/0xa0 against 193 imports/0xa0;
      every comparison below measures that as well as its own intervention
  ...
  *** libc::memcpy answered the code the guest followed; zero reaches 193 against 192
```

The starred `memcpy` line **is** the disagreement. The dispatcher had been attributing the
guest's own 192-to-193 drift to forcing `memcpy` to zero, and printing it with the same emphasis
it gives a real result. One boot pair retired it.

## The shape this is settling into

Four things now happen without a person, in an order that is not arbitrary:

1. **Is the run readable?** Two runs, compared. Everything below is conditional on it.
2. **What was the guest given?** `ORBISTOUN_TRACE_OPENS` for files, `ORBISTOUN_TRACE_MAPS` for
   memory - each the *successes*, which is the half a run never reported.
3. **What did it do with it?** The argument sweep, the diagnostic axes, the watchpoint pipeline.
4. **What is left for a person?** Named, with a sentence saying why - never a shrug.

Steps 2 and 3 are where "more telemetry" belongs, and the pattern that keeps working is the one
D578 found first: **a run reports its failures and hides its successes**, and the successes are
where the answer is. That was true of the filesystem, then of memory. It is worth asking of every
subsystem that reports anything.

## Surprises

- **The check invalidated a finding the tool was already printing with stars.** Not a
  hypothetical risk; it was live, on the title under investigation, in the same output.
- **Three existing tests failed on the plan gaining a leading step**, which is the plan being
  pinned by contents rather than by shape - working as intended, and each was a one-line update
  that says what the new first element is.

## Next

- The step this most obviously wants next: **follow a pointer out of a dumped argument.** The
  report shows thirty-two bytes at an argument; the structure behind it needs an address typed in
  from a previous run. Every piece now exists - published mappings, a watch that survives an
  address appearing mid-run - and nothing joins them.
- A `Step::ReadStructure` would be that join, and it is mechanical in both directions: the address
  comes from the finding's own evidence, and what comes back is bytes.
- Asking "what does this subsystem report only the failures of?" of the remaining ones.
