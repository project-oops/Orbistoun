# D315 - A submission carries measurements and title results only

**Status:** decided
**Date:** 2026-08-27

`orbistoun-submit` bundles measurements and title results and nothing else, and depends only on
the measurement format and the title record. The receiver counts what a bundle holds and
reports any disagreement with the manifest.

**Why:** both kinds of claim come from running a binary the submitter owns and are
reproducible and falsifiable by a command. Traces and run reports are inputs, large, and carry
far more of a title than a result needs. A dependency-free crate cannot carry behaviour. A
manifest count is the sender's arithmetic, not the receiver's measurement.

**Rejected:**
- Including traces or run reports: large, and they carry title material.
- Trusting the manifest's counts: quotes the sender's claim as a measurement.
