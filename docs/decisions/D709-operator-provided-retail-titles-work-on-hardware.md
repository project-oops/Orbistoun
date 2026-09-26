# D709 - Operator-provided retail titles work on hardware

**Status:** decided
**Date:** 2026-09-21

Every retail title the operator provides is taken as working on the hardware, and every wall in
one is orbistoun's with no attestation needed. Before instrumenting a fault, check the upstream
fact it rests on: whether the file exists in the data directory the run reads from.

**Why:** the operator does not hand over broken titles, so "a broken dump or a dev build" never
applies to them. "Debug build", "devkit path", "faithful failure" and "the title's own
assertion" restate a symptom and never close a wall. A fatal trap chased with heavy machinery
followed from a file the run sought in a mount it did not serve, while the file sat in the
title's own root.

**Rejected:**
- Requiring hardware proof first, as for titles of unknown provenance (D708): leaves open the
  door that ends the work.
- Diagnosing the downstream fault before the file route: expensive and misdirected.
