# D375 - The payload SDK is a build tool, never a source

**Status:** decided
**Date:** 2026-08-29

The open payload SDK is used only to compile payloads whose `main` and entry code are written
here, which are then run and observed. Nothing is read from its runtime or headers, and nothing
here is written while reading them.

**Why:** the SDK is GPL-3.0 and this project is MIT/Apache-2.0. A payload built with it is
observed exactly as a commercial title is loaded and observed, and a probe whose code is ours
separates the runtime's behaviour from any one payload's.

**Rejected:**
- Reading the SDK's runtime or headers: copies licensed source into the project's reasoning.
