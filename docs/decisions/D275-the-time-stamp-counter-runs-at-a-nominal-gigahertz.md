# D275 - The time-stamp counter runs at a nominal gigahertz

**Status:** assumed
**Date:** 2026-08-25

The guest time-stamp counter advances at a nominal one billion ticks a second and reports that
frequency; process time is in microseconds.

**Why:** a nominal rate keeps the arithmetic exact and makes two host machines comparable, and
a counter that does not advance makes every sleep look instantaneous. The hardware's real
frequency is a different number, so a title deriving a frame budget from it may pace itself
wrongly; both units stay assumed until a hardware probe measures them.

**Rejected:**
- The host's own counter frequency: differs between machines, so runs are not comparable.
- A constant counter: indistinguishable from a stopped clock.
