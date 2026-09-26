# D707 - A scripted pad is a configuration source

**Status:** decided
**Date:** 2026-09-26

A scripted pad is a controller source, `Source::Script { path }`, read from a file relative to
the configuration, validated before entry, and installed on the entry path so its clock starts
at guest entry. A step sets the pad and holds until the next; steps out of order or at one
instant are refused, and a script carries typed pad state, never bytes.

**Why:** a scripted controller is a setting, which may come from a file, not a diagnostic, which
may not. A pad has a sampled position rather than events, so a held level matches how a guest
polls. Installing at entry keeps step times independent of placement and linking time, and typed
state keeps scripts valid whatever the byte layout turns out to be.

**Rejected:**
- An environment variable: diagnostics never come from a file and cannot travel with a title.
- An event queue: invents a sampling rate and drifts against the guest's polling.
- Sorting disordered steps: silently runs a different script.
- Installing at process start: steps fire during loading.
