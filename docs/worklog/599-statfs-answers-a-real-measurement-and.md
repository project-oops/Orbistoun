# 599. `statfs` answers a real measurement, and the netctl loop was neither

**2026-09-15** - the two pieces of unblocked work worklog 598 turned up while proving step 9 was
unreachable

## The loop is not a loop

`sceNetCtlInit`, `sceNetCtlGetInfo` and `statfs` are each called **54** times by PPSA99980, which
reads as a guest retrying something it never gets. It is not:

| limit | calls to each |
|---|--:|
| 20s | 54 |
| **120s** | **55** |

Six times the wall clock, one extra call. It is a bounded sequence that runs once and stops - the
probe enumerating its report fields - and the guest spends the remaining hundred seconds
elsewhere. Worth recording because "called 54 times and nothing implements it" looks exactly like
a stall in the findings list, and the only thing that separates them is running the same title at
two limits.

## `statfs`, and why a cheap answer was worse than none

The guest passes `statfs("/download0", buf)` - the POSIX signature exactly, arg2 onward being
register noise. It was unimplemented, so obSCEne reported:

```
OBS|sysinfo|storage|unconfirmed|unknown
```

**Answering the call successfully makes that worse, and it takes one environment variable to
prove it.** `ORBISTOUN_RETURN=statfs:0x0` returns zero without filling the buffer, obSCEne reads
the zeroes its own memset left, and the report becomes:

```
OBS|sysinfo|storage|known|0M
```

An honest "I do not know" replaced by a confident wrong number, in one command. That is principle
3's failure demonstrated rather than argued, and it is the whole reason this is implemented rather
than answered.

## What it writes, and the provenance of every part

Four fields, at offsets from FreeBSD's `sys/mount.h` - `f_bsize` at `0x10`, `f_blocks` at `0x20`,
`f_bfree` at `0x28`, `f_bavail` at `0x30`. The target kernel is FreeBSD-derived, which makes the
layout **published** rather than inferred (principle 1's first oracle), and obSCEne independently
reads two of those offsets on real hardware and gets a believable number back (obSCEne D272) -
a second check that the layout in use is the one written here.

The *values* are a measurement of the volume actually backing the mount, via
`GetDiskFreeSpaceExW` on the host path `mount::resolve` gives.

Verified end to end rather than assumed:

| | storage reported |
|---|---|
| before | `unconfirmed \| unknown` |
| answered `0x0` | `known \| 0M` |
| implemented | `known \| 2621428M` |

Host `C:` free at the same moment: **2,621,409 MiB**. The nineteen-mebibyte difference is the
Rust builds running between the two readings.

## What it refuses

- A path under no mount answers `ENOENT`. A sandbox that does not have `/whatever` is not a
  filesystem with no room.
- A host that will not say answers `EACCES`, not zero. "I cannot tell you" and "there is no room"
  are different answers and only one of them is true.
- **It stops writing at `0x38`.** The structure is longer; its real length on the target has never
  been measured, and zeroing out to FreeBSD's full length would overrun a caller whose buffer is
  sized by the *target's* header. Every field past the last one it has a real value for is left
  exactly as the caller had it.

## Two mistakes the guards caught

**The extent test could not fail.** It asserted that everything past `statfs_at::WRITTEN` was
untouched - using the same constant the write length comes from, so widening the write to `0x80`
moved the boundary and the assertion together and still passed. The invariant is "nothing past the
last field this has a value for", so the bound is now that field's end, and the mutation fails it.
A guard that cannot fail is worth less than no guard, because it is believed.

**The test bypassed this crate's own lock.** `orbistoun-fs` has `exclusively()` for tests touching
the process-wide mount table, and D241 records what happens without it: two tests unmounting each
other, failing "about twice in five runs - too rare to be believed, too common to ignore". My test
called `mount::clear()` and installed a mount on its own. Fixed to take the guard, which also
resets the tables.

## Nothing done for netctl, deliberately

`sceNetCtlGetInfo(14, info)` answers the console's own IP address. **orbistoun has no console
network identity to report**, and inventing one would be exactly the fabrication `statfs` was
implemented to avoid. obSCEne already handles the honest case and says so in its own source: a
console that declines is *"offline, which is itself a fact and not a fault"*, reported as
`unconfirmed`.

Which is what the report says today:

```
OBS|sysinfo|ip|unconfirmed|unknown
```

So the current answer is already the correct one - arrived at because the loud stub returns
non-zero and the probe tests for that. Implementing a function whose only honest behaviour is to
decline would change no observable and cost a maintained shim. The layout is also only known via
OpenOrbis's headers, which is external provenance this project would have to record as such.

Left alone, and recorded so the next reader does not mistake "called 54 times, unimplemented" for
work waiting to be done.

## Gate state

fmt clean, clippy `--workspace --all-targets -D warnings` clean, `cargo test --workspace` 2,340
pass / 0 fail, worklogs unique, identity scan clean.
