# 2026-08-31 (later) - orbistoun runs the whole obSCEne suite; obSCEne becomes its conformance oracle


With the emulation changes in place, tested every local guest ELF. The result is the milestone the
firmware and handoff work was aiming at: **obSCEne runs against orbistoun exactly as it runs against
hardware, to `OBS|end`.**

Guests, all under `--profile ps5-cex-12.40` + `ORBISTOUN_ENTRY_ARGUMENT=handoff`:

- **prosperous/klog.elf** - the wall that was at `image+0x2708` last session is gone; klog now enters
  its server loop (read / setsockopt / mmap / munmap / vendor_system_version, 100+ syscalls) and runs
  to the time limit. For a socket server that is success, not a fault.
- **obscene.elf (27-section build)** - full suite to `OBS|end`: 498 pass, 10 partial, 7 skip, 2 fail.
- **obscene.eboot.elf (28-section)** - full suite to `OBS|end`, 183 distinct imports.
- **build/obscene.elf (37-section current build, with the new checks)** - full suite to `OBS|end`:
  516 pass, 12 partial, 8 skip, 5 fail, 197 distinct imports.
- **obscene-min.elf** runs to the time limit; the `.module.elf` files are libraries with no entry.

**obSCEne is now a conformance oracle for orbistoun**, and the divergences are the work list:

- **900-surface/control fails** - a symbol that does not exist reported present. orbistoun's resolver
  answers "present" for everything (stub-everything), which this control check exists to catch; it
  also drives the 005-generation partial (both PS4 and PS5 drivers resolve).
- **110-modules/names fails** - orbistoun describes no loaded module; module enumeration is unimplemented.
- **139-exports/enumerate fails** - the firmware base is a bare RWX region, not an ELF; a probe that
  walks it as an ELF finds no magic. orbistoun-firmware could place an ELF header at the base for fidelity.
- **sceKernelGetTscFrequency `0x1cf30`: orbistoun confirms it, hardware refuted it.** orbistoun's stub is
  a no-arg getter returning the frequency; the real function is not. This is the exact divergence that
  justified leaving the vaddr `candidate` (D410) - and now we know orbistoun's stub is over-simple.
- Softer partials worth a look: `000-boot/write-returns-count` (zero-length write claims bytes),
  `030-thread/join` (return value not preserved across join), `035-libc/rand-seeded` (two draws identical).
- **130-layout/memory-type passes on orbistoun** (third field tracks the type: onion=0, wc-garlic=3,
  wb-garlic=0xa) where the host stub returned a constant. Real-hardware ground truth is the pending eboot run.

These are cross-validated facts about orbistoun's HLE, produced by running a known guest that measures
it - the "debug orbistoun the same way as hardware" goal delivering an actual defect list.

