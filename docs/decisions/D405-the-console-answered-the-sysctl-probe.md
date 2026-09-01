# D405 - The console answered the sysctl probe, and it says 12.40


**measured** - 2026-08-30

The sysctl section written for the hardware handover (the sibling probe's `135-sysctl`) ran on a
target console - PS5, system software **12.400.009**, CFI-1116A - and the results were already
sitting in `data/hardware/ps5-imports.txt` when this went looking. The probe was the right
instrument and the answers were waiting.

### What it settled

| knob | answer | what it closes |
|---|---|---|
| `kern.osrelease` | **`0.0-prototype`** | D397 - the value `zftpd` reads |
| `kern.ostype` | `FreeBSD` | the platform naming its own lineage |
| `kern.version` | `r226974/releases/12.40 Nov 27 2025` | the firmware, stated outright |
| `hw.ncpu` | 16 | a core count that was invented |
| `hw.pagesize` | `0x4000` | 16 KiB, assumed everywhere, now confirmed |
| `machdep.tsc_freq` | `0x5f25_9b8e` | the counter frequency, by a **third** route |
| `hw.physmem` | refused, *not approved* | some knobs are gatekept, not absent |

`kern.osrelease` is the one that mattered, and it is not what the guessing would have produced.
It is `0.0-prototype` - a development tag, not a version number. `zftpd` reads it, tries to parse
a version out of it, finds none, and reports detection failed. So the payload was behaving
correctly all along against a value nothing here knew, and D397 was right to refuse rather than
invent a plausible `9.00` that would have sent it somewhere else.

`machdep.tsc_freq` answering `0x5f25_9b8e` is worth its own line: the time stamp counter, the
process-time counter, and now a named sysctl all report the same frequency by three independent
paths. A value measured three ways is a different thing from one measured once.

### The version reconciliation

An earlier reading had this console at 13.09, from `sceKernelGetSystemSwVersion`. Both are true
and they are different numbers: `kern.version` says the **system software** is 12.40, while the
SDK/kernel version that other call reports is 13.09. The platform keeps them apart, so this
project must too - the firmware a payload branches on (D403) is the one it reads from call 649,
and which of the two that is remains a question for the `137-kernelcall` probe, not yet run.

### What was implemented

`sysctlbyname` now answers the measured strings (`kern.osrelease` from the configurable machine,
`kern.ostype` as the platform's own constant) and the measured integers (`hw.ncpu`,
`hw.pagesize`, `machdep.tsc_freq`) each at the byte width the platform uses - a width that is
part of the answer, since a caller reading four bytes of an eight-byte value reads a different
number. The values are pinned as a test that names which observation it contradicts if changed.

### The 12.40 profile

The user's console is the reference target. Its measured profile is `firmware = 0x1240`,
`kernel-release = "0.0-prototype"`, generation PS5, kind CEX. The default machine still refuses
both (an unconfigured orbistoun does not claim to be a specific console, D397), so this is a
configuration to apply rather than a default to assume - which is what makes it a measurement of
one machine rather than a fact about all of them.

