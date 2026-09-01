# D043 - The accuracy suite is a separate repository, `obSCEne`

**decided** · 2026-08-19

No equivalent of `pspautotests`, blargg's ROMs, or `dolphin-emu/hwtests` exists for
this target - confirmed by survey, not assumed. So we build one, and it does **not**
live in this repo.

**On the name.** `SCE` is the vendor's abbreviation and the prefix on every symbol in
the API, so this would cut against D015 - except D015 governs *this* repo, and the
suite is a separate one with its own conventions. "Obscene" is also an ordinary
English word; the abbreviation is only visible to someone looking for it. Chosen
deliberately, recorded so it does not read later as an oversight.

**Terminology, pinned because it matters.** What the suite exercises is **functions
and imports**, never CPU instructions. Instructions execute natively on this target,
so there is nothing there to test - the entire testable surface is the library API.
"Instruction coverage" would mean something different and unachievable.

Separate because it is a different artifact for a different audience: it is guest
software written in C against the open toolchain, useful to anyone emulating this
target or testing real hardware, whereas orbistoun is a host-side Rust emulator.
Mixing them would put a cross-compiler in orbistoun's build and tie a
potentially-general tool to one consumer.

**Purpose:** call as many known interfaces as possible and report what is present and
what actually works, sectioned by subsystem - memory, threading, filesystem, audio,
video, input, graphics - mirroring orbistoun's crate boundaries so a failing section
maps to one place.

**Reporting is layered, because the reporting channel is itself a thing under test.**
A suite that reports via stdout tells you nothing when stdout is unimplemented.

1. **Trace-as-report (default).** orbistoun already records every guest call with
   arguments and return values (D018), so a suite that calls each interface in a
   known order produces a trace that *is* the result. This needs **zero** I/O
   implemented and works from phase 4.
2. **Self-reporting**, added once the emulator is healthy enough to carry it, so the
   suite is useful to other emulators and on hardware - which is the end goal.
3. **Interactive tests** (controller input, button response) last, once boot is
   reliable.

Licensed permissively like orbistoun, since adoption is the point.

