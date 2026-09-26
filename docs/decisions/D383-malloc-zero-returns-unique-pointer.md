# D383 - A zero-sized allocation returns a unique pointer, not null

**Status:** decided
**Date:** 2026-08-29

A zero-sized allocation returns a real, unique pointer rather than null.

**Why:** The standard permits either answer, but this project emulates a
platform, and the platform returns a unique pointer; the near-universal caller
idiom checks only for null, so a null answer here reads as failure to callers
written against the platform, not against the standard.

**Rejected:**
- Returning null for a zero-sized request: conforms to the standard but
  disagrees with the platform being emulated, and a caller cannot tell it
  apart from real exhaustion.
