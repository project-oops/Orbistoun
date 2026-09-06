# D503 - Seven hardware claims from data already in hand, and nine more non-claims

**measured** - 2026-09-03

Working the outstanding queue after D502 rather than waiting for a capture.

```text
OUTSTANDING 95 -> 69        OPAQUE 39 -> 58        CLAIMED +7
```

## `sceSysmoduleLoadModule`: nine identifiers the probe chose, and two answers it got

Eighteen entries, and reading obSCEne's check settled both halves at once:

```c
int rc = sceSysmoduleLoadModule(venc_modules[i].id);
obs_report_measure("106-encoder/sysmodule-load", venc_modules[i].name, "id",
                   (uint64_t)venc_modules[i].id, "id");
```

**The nine `:id` records are `venc_modules[i].id` reported back unchanged** - the probe's own
constant, which no platform behaviour could alter. D497's family exactly, so they are opaque.
They remain worth having beside the return codes; they are not claims about orbistoun.

The nine `:rc` records are real, and they split:

| | answered |
|---|---|
| `VENC` (0xa0), `VIDEOREC` (0x81) | `0x0` |
| the other seven | `0x805a1000` |

**Two are claimed.** orbistoun answers `0` to every identifier by design (D125 - every library
a title imports is resolved before the guest runs), so it already agrees with the console on
those two, and now says so under a test.

**Seven are not, and the reason in the queue was wrong.** It read *"orbistoun has a sysmodule
shim that refuses everything"*; the shim refuses nothing. The real blocker is that matching the
refusals would mean refusing seven identifiers because one capture, taken at one application
category, was refused them - and obSCEne's D301 records that category deciding an unrelated
call. A capture at a different category is what says whether this belongs to the module or to
the asker. That question is now written in the entry instead of a false statement about our own
code.

## `110-modules/load`: the paths were recoverable

Four entries said *"needs the check's exact path reproduced to claim it"*. The paths are in
obSCEne's own `obs_module_quantity` table beside the subjects that name them, in order:
`/system/common/lib/libSceLibcInternal.sprx`, `/system/common/lib/libSceSysmodule.sprx`,
`libkernel.prx`, `libkernel.sprx`. All four now claimed.

**Two of the four cannot detect a wrong path, and that was checked rather than assumed.**
Misspelling `libSceSysmodule.sprx` leaves the test passing: an unrecognised path falls through
to the same `0x8002_0002` the firmware directories answer. So for the `/system/` entries this
pins the answer and not the route - the caveat the neighbouring firmware-directory test already
records, now recorded here too. The `libkernel` entries answer `0x2001` and do fail on a wrong
path, which is the break this was watched failing on.

## `kern.hostname`: D447's own argument, applied to a knob it had not reached

The console answered **one byte** - a NUL and nothing before it. The entry said orbistoun
*"could answer it once the knob is wired"*, and D447 already states why it should:

> the knob exists on the console, so refusing it says "no such name" - false - where an empty
> NUL-terminated string says "exists, no value", which is exactly true.

Wired to an empty string. **The width is the claim and the content is not** - a hostname is a
per-machine setting, and orbistoun has none to report, which is the state the measured console
was in. Watched failing by answering `"orbistoun"` instead.

The other seven `135-sysctl` entries stay: `hw.model` and `kern.version` measure non-empty
per-machine strings, so an empty answer would not match; `hw.machine` would need `amd64`
confirmed rather than assumed; `kern.osrevision`, `kern.sdk_version` and `hw.availpages` are
values orbistoun does not source, which D397 settles as refuse-rather-than-invent.

## What this says about the queue

Twenty-six entries moved in a day with no new capture: ten that were never claimable, nine that
were the probe's own input, and seven that only needed somebody to read the check.
**None of it needed hardware, and one of the reasons in the queue was factually wrong about our
own code** - which is the argument for working a queue rather than counting it.
