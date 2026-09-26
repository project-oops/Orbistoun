# D486 - A measured register's configuration is reproduced, its status is not

**Status:** decided
**Date:** 2026-09-03

Where a hardware-measured register or word mixes configuration bits with status bits,
orbistoun installs only the configuration on guest entry and leaves the status clear, rather
than installing the value as measured.

**Why:** a status bit records something that happened before the measurement was taken.
Installing it verbatim reports the console's own startup history to the guest as a fact
about the guest's own execution, before the guest has run a single instruction.

**Rejected:**
- Installing the measured value verbatim: reproduces an artefact of when the reading was
  taken rather than a property of how the platform is configured.
