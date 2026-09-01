# D017 - Handles are per-subsystem and never recycled

**decided** · 2026-08-19

Per-subsystem so passing an audio handle to a file call is catchable rather than
silently plausible. Never recycled because reuse makes a stale-handle bug look like
a valid access to the wrong object, which is far harder to diagnose than exhaustion.

