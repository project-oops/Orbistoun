# D696 - A producer builder reserves with a zeroed body

**Status:** assumed
**Date:** 2026-09-15

A command builder whose job in the guest's path is to reserve space for the patch calls to fill
is wired once its header and extent are measured: it writes the measured header, reserves the
measured length, zeroes the body and returns the real cursor.

**Why:** the guest fills the body through the patch calls; the reservation and the returned
address are what it depends on. Filling the body from a single capture would encode the probe's
arguments as the guest's. An unwired producer hands the guest the placeholder as a packet
address, which it then copies through.

**Rejected:**
- Leaving it unwired until the argument mapping is measured: the placeholder travels into a
  memory copy.
- Filling the body from one capture: another caller's arguments presented as the guest's.
