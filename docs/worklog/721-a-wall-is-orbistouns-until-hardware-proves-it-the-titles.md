# 721. A wall is orbistoun's until hardware proves it the title's

**2026-09-19** — a session (this one) twice reached the same wrong conclusion about a title's wall:
that the *title* was at fault - a debug build, an unfinished path, a phantom symbol - and stopped
there. Both times the title runs on real hardware, so the wall was orbistoun's gap to close. This
adds the guardrail so it is structurally harder to make that mistake again, at the operator's
prompting ("how can we bolster orbistoun to make these less likely?").

## The failure, named

"It is the title's fault" is the most seductive wrong conclusion available, because it moves the
wall *out of orbistoun's scope* and ends the work - it is the emulator equivalent of a stub that
returns success. It is principle 3 turned on the tools: a conclusion that **reports more than its
measurement supports**. The measurements that were mistaken for proof:

- a guest `todo: void GfxDevicePS5SharedData::CreateWorkload()` print (worklog 720) - which is the
  title's own benign logging, not an unfinished path;
- an obSCEne "not a retail export on FW 12.40" for the symbol the guest faults into - which is
  orbistoun's HLE gap or a firmware/SDK-era question, not a broken title.

Neither is evidence a title is broken, and both were treated as if they were.

## The bolster

**Doctrine (the part that stops it):** principle 3 in `CLAUDE.md` gains a fourth rule - *a wall is
orbistoun's until hardware proves it the title's* - recorded as **D708**. The default attribution of
any guest fault is orbistoun's missing or wrong HLE; blaming the title's own code needs a hardware
observation that the same path fails the same way there; a guest `TODO` print and an unresolved
import are explicitly not that evidence. This is read at the start of every session, which is where
the mistake is actually prevented.

**Ground truth (the part that makes it visible):** the compat record gains a `[hardware]`
attestation - `orbistoun_overrides::Hardware { does, attested_by, on, note }` - recording what a real
console does with the title and who says so. `OverrideFile::fault_attribution()` turns it into the
framing, and `record_compat` prints it whenever a run faults. So the report now says, at the fault:

```
whose gap is this? orbistoun's, by default (D708): a work-in-progress HLE faulted a
  title that ships and runs on a console. Blaming the title's own code needs a hardware
  observation of the same failure - a guest TODO print or an unresolved import is not it.
  hardware: renders on a console (operator, 2026-09-19) - so this fault is orbistoun's to close.
```

A title with no attestation still defaults to orbistoun and says the hardware status is unknown -
never "the title is broken". Only a recorded hardware *failure* opens the title's own code to
suspicion. PPSA02664 is recorded as rendering on hardware (operator-attested), so its
`CreateWorkload` wall now reads unambiguously as orbistoun's.

**The negative case is the tested one** (`fault_attribution_defaults_to_orbistoun_and_reads_the_hardware_ground_truth`):
the guard must never let an absent attestation read as the title's fault, so that is what the test
asserts, per principle 3's own "a guard nobody has watched reject something".

## Two more report bolsters, at the operator's request

The framing above answers *whose* gap; these make the two specific mistakes harder still, in the
same fault block.

**The candidate causes, named at the fault.** `CallTrace::stubbed_imports()` returns the imports the
run answered with placeholders, most-called first, and the report lists them under *"orbistoun
answered these with placeholders this run - candidate causes, a real answer may avoid it"*, with the
call count and the guest-inferred shape. The upstream-divergence reading is now the default a reader
sees, not something to dig the worklist for - and it surfaced stubs a hand search had missed
(`sceAgcDriverSetHsOffchipParam`, `sceAgcDriverSetTFRing`, `sceAmprCommandBufferSetBuffer`) beside the
ones already known.

**The guest's log lines, labelled as the title's.** A line under the causes says the guest's own
output above - including any `TODO:`/`todo:` marker - is the title's logging, not orbistoun's, and
not a sign its code is unfinished. That is the exact misread that started worklog 720, contradicted
in the place it happens.

On a PPSA02664 run both now print beneath the attribution, listing `0x7d86501b8094ef57`, the two
`libSceAmpr` constructors and the rest as the candidate causes they are.

## What this does not do

It does not forbid ever finding a title genuinely at fault - a dump can be broken, a build can be a
dev build. It raises the bar for that claim to a hardware observation, and makes the cheap version
- a log line, an unresolved symbol - count for nothing.

## Gate state

`orbistoun-overrides` and `orbistoun-cli` compile; the new attribution test and the existing suite
pass; end-to-end, a PPSA02664 run prints the framing with the hardware attestation. `./bin/orbistoun
check` run below. No commit.
