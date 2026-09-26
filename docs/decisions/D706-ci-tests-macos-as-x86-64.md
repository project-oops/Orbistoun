# D706 - CI tests macOS as x86-64

**Status:** assumed
**Date:** 2026-09-19

The CI test job on the macOS runner builds and tests `x86_64-apple-darwin`, the triple the
release workflow ships, with the test binaries running under the runner's x86-64 translation
layer. The ABI boundary is not gated off aarch64.

**Why:** guest code is x86-64 and runs natively, so the only macOS build that can run a guest is
x86-64, and the boundary's `sysv64` functions do not compile for aarch64. If the translation
layer is unavailable to the job, an x86-64 runner tests the same triple natively.

**Rejected:**
- Gating the 28 `sysv64` sites and the entry assembly by architecture: real work whose product
  is a build that cannot run a guest.
- Building natively for aarch64: does not compile.
