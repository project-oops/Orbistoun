# 2026-09-03 - (/loop) The queue had permanent residents, and two asks were about the run

```
OUTSTANDING       95  ->  69
OPAQUE            39  ->  58
CLAIMED                 +7
tests           1992  ->  1993
```

Asked whether obSCEne's backlog 022 covers what still needs data. Checking the 85 outstanding
measurements one blocker at a time answered a different question first.

## Ten of them were never going to become claims

`OPAQUE`'s own doc comment names the case that forced the table to exist - a module handle,
because the number reflects how many modules *that* loader had already placed. **That entry was
in `OUTSTANDING`.** So were nine of the same kind:

- `120-measure/identify-clocks:sceKernelUsleep:requested` is **the sleep the probe asked for**.
  A platform that ignores the request produces the same reading - D497's family, one layer up
  from an out-parameter.
- Three `clocks-advance` absolutes: a moment on one machine, and orbistoun cannot reproduce the
  console's epoch.
- Three `timer-ratio` deltas: a rate on one machine. The frequency they calibrate to is
  claimable and is asserted separately, so the useful half stays.
- `kern.osrelease`'s length: that console's configuration, empty by default.

Moved with longer reasons than they had. **This makes the project no more capable; it makes the
number mean what people read it as** - "95 outstanding" was a day's planning headline and
eleven per cent of it could not be worked. D502.

## And the answer to the question that started it

022 is organised by **which symbol to call**. Two of orbistoun's blockers are not about a
symbol at all, which is why they were missing from a list that looked complete:

- **A capture taken as application category 0.** Three `130-layout/memory-type` readings record
  an arbitrator decision, not a memory type: under category 65536 the console grants zero bytes
  of direct memory whatever is asked. obSCEne's own D301 measured the same call succeeding
  under category 0. Worth doing on the same visit as anything else, because **every measurement
  in a capture inherits the category** and a run taken under the wrong one has this problem
  everywhere and says so nowhere.
- **A mutex attribute round-tripped through one object.** Six readings set a type on one object
  and read it back from another; two disagree outright - the console rejects type 0 and answers
  `0xffffffff` where orbistoun accepts it. A check calling `Init`/`Settype`/`Gettype` on one
  object settles a live disagreement rather than adding coverage.

Both added to 022, plus the guard word for `018-relational` named as the concrete instance of
the out-parameter sweep already recorded there.

## What the rest of the 85 are blocked on, since it is not obSCEne

Counting by blocker rather than by group: **39 are `106-encoder/*` symbol resolution** with no
encoder subsystem (principle 6), **18 are `sysmodule-load`** where the data exists and
orbistoun's shim refuses everything, and the remainder are orbistoun-side - a path to reproduce,
a module to resolve against, a payload-argument block it does not present, a `cpuid` that
answers the host's.

**Almost none of the 85 is waiting on a capture.** That is the useful finding: the hardware
queue is orbistoun work wearing a hardware label.

## Does the rest need more probes? Almost none of it

Bucketing all 85 by what would actually unblock them:

| needs | count |
|---|---|
| a probe change (already asked in 022) | **7** - the mutexattr round-trip and the guard word |
| a capture condition (already asked in 022) | **3** - `130-layout` under category 0 |
| **orbistoun work** | **75** |

The 75 break down as 39 encoder symbol-resolution (no encoder subsystem, principle 6), 18
`sysmodule-load` where the data exists and orbistoun's shim refuses everything, 8 sysctl values
orbistoun does not source, 4 that need the check's exact path reproduced in a test, and six
singletons - no tiers modelled, `cpuid` answering the host's, no payload-argument block.

**So the answer is not "more probes".** obSCEne's asks are four checks and one rebuild; the rest
of the queue is this project's own.

## One new probe shape is worth having, though

"Needs a struct layout" reads as blocked on a document, and most of it is not.
**`obs_report_written` already measures an extent** - it diffs a buffer before and after a call
and reports the last byte that *changed*, so a trailing zeroed field is not read as absent.

Which splits `scePadReadState` in two: **the arity needs a lawful reference** (principle 2 is
strict there - a wrong one corrupts the stack), and then **the layout is measurable**. An
oversized poisoned buffer gives the structure's size; a second call with a control held gives
the field offsets, without anyone naming a field.

That is the general form of what the encoder out-parameter fix started: a probe that poisons and
reports the extent learns a structure without being told one, which is the only way this project
is allowed to learn one. Added to 022.

## Then worked the queue instead of counting it

Twenty-six moved in the end, none of it needing hardware. D503.

**`sceSysmoduleLoadModule` - 18 entries, and reading obSCEne's check settled both halves.** The
nine `:id` records are `venc_modules[i].id` reported back unchanged - the probe's own constant,
D497's family, opaque. The nine `:rc` records split: `VENC` and `VIDEOREC` answered `0`, the
other seven `0x805a1000`. **Two are now claimed** - orbistoun answers `0` to every identifier by
design (D125) and already agreed.

The seven are not, and **the reason in the queue was factually wrong about our own code**: it
said orbistoun's shim "refuses everything" and the shim refuses nothing. The real blocker is
that matching the refusals would pin orbistoun to one capture's application category, which
obSCEne's D301 records deciding an unrelated call. That question now sits in the entry instead
of a false statement.

**`110-modules/load` - the paths were recoverable.** Four entries needed "the check's exact path
reproduced"; the paths are in obSCEne's own `obs_module_quantity` table beside the subjects that
name them. All four claimed. **Two of them cannot detect a wrong path** - misspelling
`libSceSysmodule.sprx` leaves the test passing, because an unrecognised path falls through to
the same `0x8002_0002`. Found by trying it, recorded in the test, and the break was then done on
a `libkernel` entry where the codes differ.

**`kern.hostname` - D447's own argument, applied to a knob it had not reached.** The console
answered one byte: a NUL and nothing before it. D447 already says why an existing-but-empty knob
should answer an empty string rather than "no such name". Wired. The width is the claim and the
content is not - a hostname is per-machine and orbistoun has none, which is the state the
measured console was in.

## State

`cargo test --workspace` green - 117 suites, **1992 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-352 and D466-D502.

**Next**: the differential remains the item with headroom - `strtok_r`, `strtof`,
`sprintf`/`vsnprintf`; the wide-character family needs a wide text encoding in the record format
first.
