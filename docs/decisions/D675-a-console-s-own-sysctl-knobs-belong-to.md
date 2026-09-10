# D675 - A console's own sysctl knobs belong to its profile, and two belong to the platform

**Status:** decided
**Date:** 2026-09-10

## What looked like a gap was the wrong machine

`130-layout/system-software-version` read `partial` against a hardware leg that passes it, and it
was next on the list. It turned out not to be an implementation gap. `sceKernelGetSystemSwVersion`
was already written against the measured layout (D420): size word left alone, string at offset 8,
packed integer at `0x24`. It refuses on a machine that carries no software version.

That was the machine every conformance run this session presented. No `shell.toml` exists, and
`bin/orbistoun` never passes `--profile`. The hardware leg it was compared against,
`20260909-110725-eboot`, reports `firmware|known|12.40`. Re-run under `prospero-cex-12.40`,
exactly one row moved: `130-layout/system-software-version` went from `partial` to `pass`.

**Presenting the reference machine moved no title.** All six retail titles, re-run under the
profile, stopped where they had stopped before: identical imports, identical stops, call counts
within ±18. None of them calls `sceKernelGetSystemSwVersion`. So the unset default cost one
conformance row and nothing a title does, and that is the evidence for keeping the unset machine
as the sweep default.

## Why the header still said `unconfirmed`, and what was missing

Even under the profile, obSCEne's header read `firmware|unconfirmed|unknown`. Its `sysinfo.c`
reads the firmware from **`kern.version`**, taking the token after `releases/`. orbistoun did not
answer that knob at all.

`135-sysctl/names` asks 19 names. The console answers 12 and refuses 7, so the check reads
`partial` **on hardware too**. Partial is the conformant result, and nothing here is chasing a flip.
orbistoun answered 6, all within the console's 12. The six it lacked sort into three kinds, and the
kind decides where each value may come from.

**Published, and the same on every machine of this architecture.** Answered unconditionally, like
`kern.ostype`:
- `kern.osrevision` is `199506`: FreeBSD's `BSD` macro from `sys/sys/param.h`, which `KERN_OSREV`
  returns. The console wrote `52 0b 03 00`.
- `hw.machine` is `amd64`: the `MACHINE` string from `sys/amd64/include/param.h`. The console wrote
  `amd64`.

In both cases the published header and the measurement agree. Neither is a per-console value, so
neither belongs in a profile.

**One console's values.** Carried by `Machine` and set in the profile, verbatim:
- `kernel_version`, `kern.version`: the 43-byte banner `r226974/releases/12.40 Nov 27 2025
  02:23:38`. Only the release could be composed from `firmware`; the revision and the build date
  would be invented, so the string is stated whole.
- `kernel_sdk_version`, `kern.sdk_version`: the four-byte integer `0x12400009`, i.e. `12.400.009`.
  The knowledge base's earlier run names the same number as that console's system software. It is
  kept apart from `firmware` for the reason `software_version` is: the packing between them is not
  documented.
- `hardware_model`, `hw.model`: `100-000000189` followed by 34 spaces, 47 bytes. The padding is part
  of the answer. A caller comparing the whole string sees a different value if it has been trimmed.

**Live state.** `hw.availpages` stays refused. The console wrote `0x222270`, but that is available
memory at the moment it was asked, and orbistoun's memory model is not the console's. Transcribing
it would be a constant pretending to be a measurement of this run. `hw.physmem` stays refused
because the console itself refuses it.

## Unset refuses. It does not answer empty.

`kern.osrelease` answers an empty string when the machine carries none: D447's call, argued on
the grounds that the knob exists and "exists, no value" is true where "no such name" is false.
**The three new knobs refuse instead.**

Extending D447 to them would have the default machine report "exists, empty" for knobs a console
fills with real values. `135-sysctl/names` counts any answer as answered, so the default machine
would score three knobs it knows nothing about. D447 stays as it is for the one knob it was argued
for, and does not become the rule for every string a profile might carry.

## Shape

`answer_integer` and `answer_for` are now thin wrappers over `integer_knob(machine, name)` and
`text_knob(machine, name)`. The machine is an argument so the set and unset cases can both be
tested against constructed machines, without writing to the process-wide slot another test in the
same binary may already hold. That is principle 8: a pure decision plus a thin effectful wrapper.

## A correction to an earlier draft of this record

An earlier draft of this file cited D010 for refusing rather than inventing. D010 is the rule that
an image is entered only when fully linked. The rule meant here is **principle 3, honest failure
over plausible output**.

## Verified against the guest

obSCEne ran under orbistoun once per machine, compared against the console leg
`20260909-110725-eboot`:

| | console | orbistoun, unset machine | orbistoun, `prospero-cex-12.40` |
|---|---|---|---|
| `135-sysctl/names` answered | 12 (`0xc`) | 8 (`0x8`) | **11 (`0xb`)** |
| header `firmware` | `known 12.40` | `unconfirmed` | **`known 12.40`** |
| `130-layout/system-software-version` | pass | partial | **pass** |

Under the profile, all five new knobs came back **byte for byte** as the console wrote them:
`kern.version` in all three rows, `hw.model` including its padding, `kern.sdk_version` as
`09004012`, `kern.osrevision` as `520b03`, and `hw.machine` as `616d643634`. The one name short of
the console's twelve is `hw.availpages`, refused on purpose.

On the unset machine the two published knobs answer and the three profile knobs refuse, which
gives the eight the design says it should. `135-sysctl/names` stays `partial` on both machines,
as it does on the console.
