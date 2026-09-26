# D032 - The guest runs in a child process

**Status:** decided
**Date:** 2026-09-26

A guest executes in a worker process, never inside the GUI or CLI process. The worker renders
headless and hands the shim ordinary bytes.

**Why:** the guest demands fixed addresses, and in a shim's process those compete with the UI
toolkit, the graphics driver, loaded libraries and address randomisation - a conflict that is
nondeterministic. Process exit reclaims guest threads exactly, reload is teardown, and the
fault handlers belong to the guest alone.

**Rejected:**
- In-process execution: nondeterministic placement failures and leaked threads.
- A child-owned window reparented into the shim: couples the process boundary to a window system.
