# D008 - Unimplemented stubs refuse by default

**Status:** decided
**Date:** 2026-08-19

An unimplemented function answers `Unimplemented`, never success. The stub policy is a runtime
TOML file keyed by symbol name, so relaxing one function is an edit and a relaunch.

**Why:** a stub that reports success is indistinguishable from working code until the guest
corrupts something much later. Editing the policy and relaunching is the bisection workflow,
and it must not need a rebuild.

**Rejected:**
- Default success: plausible behaviour that hides the missing implementation.
- Policy compiled into the binary: every experiment becomes a rebuild.
