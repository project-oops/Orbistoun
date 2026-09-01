# D438 - Cross-section diff against hardware: placeholder error codes replaced with the measured ones


**measured** - 2026-09-01 (user-directed, /loop)

Ran obSCEne's own probe (`titles/obscene/obscene.elf`) under orbistoun and diffed its report against the
hardware one (`reports/hardware/console-report-klog.txt`), section by section. The `020-memory` cycle now
matches on `direct-size` and `allocate` (the latter thanks to D437's `ReservedLow`). The diff also
surfaced a class of divergence: several "rejects-bad-input" checks answered a `0x7fff…` **placeholder**
where hardware answers a real vendor errno - the D125 shape, one level up: a guest testing for
`SCE_KERNEL_ERROR_EINVAL` never matches `0x7fff0002`.

Fixed the three that are kernel-base (`0x8002_0000`) errnos, each now verified equal to the hardware
figure by re-running the probe:

| check | was | now / hardware | errno |
|---|---|---|---|
| `020-memory/unmap-rejects-null` | `0x7fff0002` | `0x80020016` | `INVALID` (EINVAL) |
| `015-sync/event-flag-rejects-bad-handle` | `0x7fff0003` | `0x80020003` | `NO_SUCH` (ESRCH) |
| `040-file/close-rejects-bad-fd` | `0x7fff0003` | `0x80020009` | `BAD_DESCRIPTOR` (EBADF) |

The event-flag family (poll/set/clear/delete) all took `NO_SUCH`, since one member was measured and they
fail a handle lookup identically.

**Left for follow-ups, deliberately:**
- `090-audio` (`0x80260003`) and `100-input` (`0x80920003`) reject bad handles too, but in their own
  subsystem error bases (`0x8026`, `0x8092`), and `GuestError::vendor` only builds the kernel base.
  Needs a subsystem-aware constructor before those can be answered honestly.
- `060-module/dlsym-rejects-bad-handle` wants `NO_SUCH`, but orbistoun's `dlsym` ignores the module
  handle and resolves globally (by design, to bootstrap the resolver), so it has no handle to reject.
  Matching hardware needs handle validation, a design change rather than a code swap.
- The direct-map virtual base is `0x2_0000_0000` on hardware and `0x7200…` here - system-wide and worth
  applying, but a fixed low-address reserve needs its own test on Windows.

