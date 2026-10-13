# D828 - The host GPU is chosen by ORBISTOUN_GPU, discrete first by default

**Status:** assumed
**Date:** 2026-10-13

The Vulkan session opens the device `ORBISTOUN_GPU` names: `discrete`, `integrated`, a device's
place in the host's enumeration (as `vulkaninfo --summary` lists it), or part of a device's
name, compared without case. Unset, it opens the first discrete GPU, or the first device where
there is none. A choice the host has no device for refuses to open one, saying so, rather than
running on another. The device opened is logged with its place, name and type.

**Why:** the session opened whichever device the host listed first. On a desktop with an
integrated GPU beside a discrete one that is an accident of driver order, and a measurement
taken on it says nothing certain about which GPU it measured. Choosing on purpose also makes
the integrated GPU a low-end host on demand: a desktop's integrated RDNA2 GPU is weaker than a
handheld's, and a run on it shows what a GPU-bound host costs without leaving the desk. A
handheld's only GPU is integrated, so the default falls back to the first device rather than
refusing.

**Rejected:**
- The first device, as before: the default changes nothing on hosts that list the discrete GPU
  first, and on the rest it was the integrated GPU by accident.
- Falling back to another device when the named one is absent: a run would measure a GPU nobody
  asked for, and say it measured the one they did.
