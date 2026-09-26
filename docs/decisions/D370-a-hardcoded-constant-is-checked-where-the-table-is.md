# D370 - A hardcoded constant is checked where the table is

**Status:** decided
**Date:** 2026-08-29

A crate that cannot reach the harvested ABI table names the constant it needs as a `pub const`,
and a crate that can reach the table holds a test asserting the two agree.

**Why:** the dependency graph forbids some crates from reading the table, and a constant written
unchecked is how another platform's number goes unnoticed. One constant and one test fail loudly
the day they disagree.

**Rejected:**
- An unchecked literal: silently wrong on the first differing value.
- Moving the table: rearranges a subsystem's dependencies for one number.
