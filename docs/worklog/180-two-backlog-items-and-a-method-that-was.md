# 2026-08-27 - Two backlog items, and a method that was quietly unsound


### Nobody picks a test address any more (D324)

`docs/BACKLOG.md` had the cross-crate half of the fixed-address hazard open, with the fix
already named: *a per-crate range*. It got done because the hazard bit a third time, and
the third time was **me** - four new `orbistoun-thunk` tests took the shipped base and
raced, and my first repair picked offsets by hand, which is precisely what the entry
describes as the way to reintroduce it.

`orbistoun-mem::test_bases` hands out a range per crate and an address per test. Six
consecutive runs of the four affected crates, no failures. Entry closed.

### The third place nobody wrote (D325)

`PPSA28061` reads `0x0` and dies, with three classes eliminated and *"a heap or
direct-memory slot"* left. Stack had a poison, heap had a poison, **direct memory had
none** - and direct memory is the allocator this title actually uses, 18 mapping calls in
the first five hundred.

`ORBISTOUN_DIRECT_FILL` added. Result byte-identical, so a fourth class falls.

### The part that nearly went in the log wrong

**An unchanged run is not an elimination until the intervention is shown to have
intervened.** Three runs across two titles all came back identical, and every one was
equally consistent with the fill never having fired - a `None`, a protection check
rejecting everything, a mapping path that was not the one in use. I was one command away
from recording a class as tested when it might never have been.

Both fills now count what they did and the run says so:

```
orbistoun: direct-memory fill: 17 mapping(s), 71368704 bytes
orbistoun: heap fill: 5 allocation(s), 20320 bytes
```

A run that asks for a fill and reports none says **"asked for and never fired - nothing was
tested"**, because that is the sentence a reader must never have to infer.

This retro-validated the backlog's *existing* heap elimination too, which had rested on the
same unproven assumption. It stands - but it stood on luck.

With all three poisons at once, all three shown firing, the fault is byte-identical. The
memory class is not narrowed, it is **closed**. What remains is what the run report has been
printing all along: `sceSysmoduleLoadModule` called three times with nothing implementing
it. Answering it `Ok` was tried and changed nothing, which is the point - the title needs
the module *present*, not the call to succeed. A return value cannot supply a side effect.

### Workspace

Three of the concurrent session's crates now fail: `orbistoun-submit` (missing `use`),
`orbistoun-gui` (`Preferences::load` gained an argument, `app.rs` not updated), and
`orbistoun-shell` (a clippy doc lint). The tree compiles and tests pass; `-D warnings` does
not. Verified this session's crates directly - the only clippy error in the workspace is in
`crates/orbistoun-shell/src/cross.rs`, which this session has never opened.


### The three open things, closed

**The data directory was a sandbox.** Every run this session wrote into
`...\Packages\Claude_*\LocalCache\Roaming\orbistoun\` - measurements, traces, title data - so
the machine turning the loop and the person reading the repository had different data roots
and neither could see the other's. `C:\orbistoun-data` is outside the profile, confirmed not
redirected, and `ORBISTOUN_DATA_DIR` points there now. 9.4 MB moved.

**A run records itself.** `compat record` printed the command and waited for somebody to type
it, and `compat/` then sat untouched for four days while the loop kept finding things - the
repository's own record of a title disagreeing with every run anybody had done. It writes now,
into the slot its policy belongs to, only when it improves on what is there. The first run
under the new behaviour put PPSA02664 at **25 imports against the recorded 23**, and moved the
wall from `image+0xafc959` to `image+0xafcc08`: the reserve-range finding, days old, finally in
the repository.

**And the loop generates its own promotion.** A measurement implies a knowledge-file entry -
that is what turning a local cache into something the emulator ships means - so `submit export`
writes one as a unified diff. Applied here with `git apply`, it took a measurement made on
2026-08-26 into `libkernel.toml` and the tool reads it back correctly.

### The surprise, and it is the good kind

The generator invented a field. It wrote `found_by = "generated"`, which describes how a
*name* was found rather than how a *behaviour* was measured, and the shipped-files provenance
check refused it within a minute of the patch being applied:

```
sceKernelReserveVirtualRange: found_by = generated but symbols/generated.json re-derives it as static
```

Not caught by review - the wrong line had a confident comment above it saying the function
invents nothing. Caught by a guard written long ago that re-derives every name and compares.

**A generator is exactly as safe as the guards around it, and that is the first real evidence
that these ones are.** It is also the argument for generating data rather than code: the
entry went through a checker that understands the format, which is a thing no amount of care
while writing Rust would have got.

### The last table nobody could keep current

`PROJECT_STATUS.md` opened with a per-title table typed by hand. It said PPSA02664 reached 23
imports and ended at `image+0xafc959`; `compat/` had said 25 and `image+0xafcc08` since a run
earlier the same day. Four days of drift in the table a reader looks at first.

D240 is why it was typed - a generated block may hold only what the tool can recompute
anywhere, and the corpus is not tracked. But that rule is about needing a **run**, and
`compat/` is committed: reading it works in CI and works for somebody with no titles. So it is
generated now, from the honest slot, with any experiment named underneath rather than folded
in.

The first generated version printed `98957030` where the typed one had `98,957,030`. Nobody
would call that a bug and everybody would notice it - and a generated artefact that loses
something a reader can see gets blamed for the loss and reverted, taking the fix with it.

### The corpus command was a landmine

`sweep` starts by calling `names`, which harvests strings into `learned` - the thing that grew
the list to 11,842 words in the first place. The mangling filter catches 6,253 of 6,451
fragments and **5,592 would still get through**, taking one `learned x2` shape from 169
million candidates to 169 billion. Running the project's own corpus command would have undone
the day's work, silently, exactly as before.

The filter was never going to be enough: what matters is not whether a word looks like a
fragment but what the vocabulary costs. `learn_words` now costs a round before writing and
refuses a set that would push it past a ceiling, naming the numbers and the choice - curate
the words, or drop a shape that uses the slot twice. `Refused` is a variant rather than a
`None`, because a silent refusal is indistinguishable from "nothing was new".

Watched refusing a flood of two thousand words end to end, and watched accepting one.

### The bug the ceiling found on its way in

The first ceiling refused the *shipped* grammar: costing it gave 27.9 billion where hand
arithmetic said 367 million. Seventy-six times, and the factor was the answer.

`find_list` found a vocabulary list by searching for `\n]`. A one-line list - `prefix =
["sce"]` - has none, so the span ran on into the next list and swallowed it: `current_words`
answered 76 words for a list holding one, the prefix plus all 75 modules.

Live and invisible, because every existing caller passed multi-line lists. It surfaced only
because a new caller needed a one-line list's size, and because the number was checked against
arithmetic instead of believed. Had the ceiling been raised to accommodate 27.9 billion, the
bug would still be there and the ceiling would be eleven times too loose to stop anything.

### The loop turned end to end, unattended

Six titles plus the probe module, every one recording itself without anybody typing a command.
**Every change to `compat/` was an addition** - not one `[status]` line was modified, which is
the slot routing doing exactly what it was built for.

The finding generalised. `sceKernelReserveVirtualRange` took PPSA02664 from 23 imports to 25;
PPSA03416 went 23 to 25 on the same policy. One title getting further is ordinary and a wrong
answer can buy it. Two agreeing is a different class of claim.

The probe module reaches **177 imports at 100% standing** against 47 for the best commercial
title - which is what a conformance probe built against what exists should look like, and a
useful reminder of how far apart the two numbers are.

`worklist`: 328 distinct imports across 15 runs, and `sceKernelDirectMemoryQuery` is 98.7% of
all calls.

### And the sweep said nothing about any of it

Six records written, and the sweep's own output filter dropped every line saying so - it
greps for `^reached|^outcome|^halted|^failed`, and the record line is indented. A person
running the corpus command would have seen no evidence that the thing they had just enabled
had done anything.

Not a bug in the recording. A bug in the report of it, which is the same failure this log
keeps finding one level down: the work happened and the instrument did not say so.

### A turn against a real title, and what its own report was hiding

The first turn taken after the loop was wired end to end. It behaved correctly - swept every
argument, found `Unmoved`, and stopped at the step that is a person's, saying why. Then:

```
asked 6 other diagnostics; 5 changed nothing
```

Six asked, five silent, **the sixth unnamed**. The one reason to run a diagnostic is the
answer that is not `Nothing`, and the summary was counting the negative space.

It was hiding this:

```
Fill { region: Bss, byte: 165 }: broke it earlier: 0xffffffffffffffff,
  reaching 8 against 25 - says nothing about the original wall
```

**And reporting the address alone would have been worse than the silence.** A fault at a new
address reads as a lead; this one broke the guest before it reached what was being asked
about. `Change` already split `MovedTo` from `BrokeEarlier` for exactly that reason (D129) and
the summary discarded both together, so the fix had to carry the kind, not just the number.

The machinery was right. Its account of itself was not - which is the third time today that
distinction has been the finding.

### The gate caught what I called green

Three failures on a run I had already reported as done: two clippy findings and a drifted
status table. I had run `cargo check` and the unit tests after the last edit and **not
clippy**, then said the work was finished. The scoped gate exists so that guess is unnecessary
and I skipped it anyway - the same shape as everything else here, one level up.

The status drift was the gate working: the sweep and the turn moved `compat/`, and the
generated title table can no longer silently disagree with the records.

### A record with no honest baseline

The fourth failure, found on the re-run, was real. The probe module was recorded for the first
time under a measured policy, so it went to `[experiment]` and has no `[status]` at all -
correct, and it broke a test that compared the file count with the count of *honest* records.

Those two numbers had been equal since the day the test was written, so the count read as a
check on parsing. It was a check on a coincidence, and it failed naming something that was not
the cause. It counts slots now.

**Three times today a proxy has stood in for the thing it measured**: a manifest quoted
instead of counted, a filter that dropped the lines proving work happened, and a file count
standing in for parsing. Each agreed with reality until the first case where it did not.

