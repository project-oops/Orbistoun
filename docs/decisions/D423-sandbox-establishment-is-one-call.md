# D423 - Establishing a title's sandbox is one call in the filesystem crate

**Status:** decided
**Date:** 2026-08-31

Preparing a title's filesystem (clearing an ephemeral overlay, installing the base tree with its
writable device mounts, layering the title over its data directory) is one function,
`sandbox::establish`, owned by the filesystem crate. Retention is a typed enum with a persistent
default, not a string flag.

**Why:** The three steps have a required order, and a second consumer (a replay tool, a GUI, a
test) that re-derived that order by hand could get it wrong. A typed retention setting gives the
policy one meaning across every caller; the crate reads no environment itself, so a caller maps
its own configuration onto the type.

**Rejected:** leaving assembly in each consumer, with retention selected by comparing an
environment string - already cost a title its state once when the steps ran out of order.
