# D478 - A differential result is its own knowledge tier

**Status:** assumed
**Date:** 2026-09-02

A fact confirmed by running the same call against a published implementation of the same
interface, rather than against the target itself, is recorded under its own tier, distinct
from measured, published, guest-observed and assumed, and it names which implementation it
ran against.

**Why:** a differential result establishes agreement with another implementation, not that
the target behaves the same way. Conflating it with a hardware measurement would inflate the
count this project uses to know how much of itself is unverified against real hardware.

**Rejected:**
- Recording it as measured: it was not observed on the target and would misstate what the
  measured count means.
- Recording it as published or assumed: too coarse, and discards that something was actually
  executed and agreed.
