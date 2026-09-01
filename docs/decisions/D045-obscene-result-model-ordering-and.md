# D045 - obSCEne result model, ordering, and coverage generation

**decided** · 2026-08-19

The core of the suite: exercise every known function, report what is present and what
works, grouped into sections and ordered base-to-high-level.

**Four result states, not three.**

| State | Meaning |
|-------|---------|
| **grey** - absent | the import did not resolve at all |
| **red** - failed | resolved and called; returned an error or crashed |
| **amber** - partial | reported success, but the side effect did not happen or the value is out of spec |
| **green** - pass | did what it claims |

Grey versus red is the split that earns its keep: *not implemented* and *implemented
wrong* are completely different bug reports. Amber catches precisely the failure D008
exists to prevent - a stub that lies about succeeding. Allocation returning OK while
the memory is not writable, or an address coming back unaligned, is not a pass.

**Ordering short-circuits.** Sections run base-to-high-level, mirroring orbistoun's
dependency spine (D004): memory, threading, filesystem, time, then video, audio,
input, graphics. A failure low down invalidates everything above it, so a failed
foundational section either halts the run or marks all downstream results
**untrusted - dependency failed**. Without that, one broken allocator produces four
hundred cascading failures and the report becomes unreadable exactly when it matters
most.

**Coverage is generated, not hand-written.** The open toolchain's headers and stubs
(D044) *are* the list of what is known, so one presence test per symbol can be emitted
automatically - thousands of them, no authoring. Hand-written behavioural tests sit on
top for the few dozen things worth testing properly. That is what makes "every known
function" achievable rather than aspirational.

**Colour is presentation, not the result.** The underlying value is a state code;
colour applies where there is a display. In trace-as-report mode (D043) the states are
reconstructed from the recorded calls, so the model works with no video, no output,
and nothing implemented beyond load-and-execute.

