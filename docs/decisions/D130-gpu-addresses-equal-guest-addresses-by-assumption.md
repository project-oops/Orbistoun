# D130 - GPU addresses equal guest addresses, by assumption

**Status:** assumed
**Date:** 2026-09-26

`pipeline::guest_address_of` converts a GPU virtual address to a guest one and is the identity.
Every submission counts addresses that resolved and addresses that did not, separately from the
shader outcome.

**Why:** the hardware shares one memory pool between processors, which is the reason to expect
identity, but expecting is not knowing. A named function makes the assumption one edit to
change and the first thing a failed read points at. Each resolved address is a data point about
the assumption.

**Rejected:**
- Silently treating the two spaces as one: nothing to suspect when it fails.
- A trait: one implementation and no evidence of a second.
