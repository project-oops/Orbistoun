# D447 - sysctlbyname answers the knobs it can source, and kern.osrelease stops being refused


**measured** - 2026-09-01 (user-directed /loop; obSCEne oracle)

`135-sysctl/osrelease` failed under orbistoun: `sysctlbyname` had no implementation, so it hit the default
stub and refused, and a refused `kern.osrelease` is precisely what turns firmware detection off in a title
that reads it (the sibling-emulator wall obSCEne was built to avoid). Implemented it.

`sysctl_value(name, kernel_release)` decides what each knob answers, and refuses the rest rather than
inventing:

- `kern.ostype` → `"FreeBSD"`. The one fact this project rests on - the target kernel is FreeBSD-derived
  (`docs/REFERENCES.md`) - stated back to the guest, a citable constant, not a guess.
- `kern.osrelease` → the configured [`machine`]`().kernel_release`, NUL-terminated. Empty until a machine
  sets one, because orbistoun does not invent a kernel version (why `Machine::default().kernel_release` is
  empty). A console measured `"0.0-prototype"` (length `0xe`, obSCEne `135-sysctl/osrelease`), which a
  machine profile may carry; the default answers an empty knob rather than pretending to know it.
- everything else → refused. `kern.version` is a build banner with a measured revision and date, and
  `hw.ncpu` a core count obSCEne itself flags the sibling emulator for inventing; orbistoun carries neither
  honestly yet, so it refuses rather than answers plausibly (principle 3).

The handler follows the POSIX/FreeBSD contract - copies what fits, updates the length, answers the size
alone when handed no destination, and never writes past the length it was given (the overrun obSCEne's
guard byte catches). A too-small buffer earns `ENOMEM`, for which `errno::NO_MEMORY` (12) was added.

Verified against the oracle: `135-sysctl/osrelease` now **passes** (as on the console); `135-sysctl/names`
improves from refused to `partial` - it answers `kern.ostype` and `kern.osrelease` and honestly refuses the
two it cannot source, where hardware answers all four. obSCEne's distinct failures drop from five to four,
no new ones. A unit test pins `sysctl_value` (ostype, a set and an unset release, two refusals). Kernel and
core tests pass, clippy clean, new code fmt-clean.

**Not the same class, and left:** `137-kernelcall/system-version` fails under orbistoun but **skips on
hardware** - its gadget needs `getpid` resolved, which the console refused (`0x80020003`) so the probe never
built it; orbistoun does resolve `getpid`, reaches the raw syscall and fails it (`-78`). That is a
divergence about resolving and raw syscalls, not a knob to answer, so it waits. Remaining: `110-modules`
(the one-module gap) and `900-surface/control` (the resolver reports a non-existent symbol present).

