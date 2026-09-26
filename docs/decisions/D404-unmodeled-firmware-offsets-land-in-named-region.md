# D404 - Unmodeled firmware-offset addresses land in a named region

**Status:** assumed
**Date:** 2026-08-30

An address a guest computes as a fixed, firmware-specific offset from a
resolved symbol - reaching for a real system layout this project has no
lawful way to reproduce - is mapped into a dedicated region that is otherwise
unmapped, so that any access through it names the offset the guest wanted
rather than faulting at an anonymous address or succeeding against invented
content.

**Why:** Reproducing an unpublished, firmware-specific memory layout would
mean inventing addresses this project cannot derive from its own inputs, and
a guest acting on a fabricated layout is the exact failure this project exists
to avoid. Naming the offset a guest reached for, without answering it, turns
"expects a layout this project does not have" into a concrete, itemized list
of exactly what is missing.

**Rejected:**
- Reproducing a guessed firmware layout: not derivable from any lawful
  source, and a guest acting on it would be acting on a fabrication.
- Leaving the addresses unmapped and anonymous: a fault says only that
  something failed, not which offset the guest wanted.
