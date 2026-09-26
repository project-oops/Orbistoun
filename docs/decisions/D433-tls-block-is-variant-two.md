# D433 - Guest thread-local storage is variant-II, self-pointer at the thread pointer

**Status:** decided
**Date:** 2026-09-01

A guest thread's TLS block places its initialised data at the bottom, its zero-initialised data
above that, and a self-referential pointer at the thread pointer itself, so a guest reading its
own thread pointer gets the pointer back.

**Why:** This is the variant-II layout the guest's own compiled code expects when it dereferences
its thread pointer to find its TLS block; a guest's initialised-data image is copied from where
the loader already placed it, so no separate copy of that data is kept.

**Rejected:** a variant-I-style indirect layout - does not match what a thread-relative access
compiled for this convention expects.
