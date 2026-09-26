# D238 - A call budget, with the clock as backstop

**Status:** decided
**Date:** 2026-09-26

A run stops after a fixed number of guest calls, twenty million by default, checked on the call
path. A wall-clock watchdog, started before entry, stops a guest that makes no calls. The exit
status says which fired, and both limits are run conditions.

**Why:** a clock fixes the duration and lets the call count vary with the host, so a verdict
would measure the machine; a budget fixes the count. A guest waiting on something that never
happens makes no calls, so the clock remains. Killing a guest from outside loses its trace.

**Rejected:**
- Wall-clock limit alone: verdicts vary with host load.
- A watcher thread polling the counter: stops at about the budget.
- No limit: a spinning guest never yields its trace.
