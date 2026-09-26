# D296 - Automatic fixes are data, and writes need a conformance check

**Status:** decided
**Date:** 2026-08-26

A fix the loop proposes is a policy entry, from a rule or from a model, never Rust; Rust is
reserved for real logic and reviewed by a person. A trial that changes only a return value may
be kept on `FURTHER`; one that writes guest memory needs a conformance check. Learned entries
live in `learned.toml` beside `config.toml` and lose to anything the config states.

**Why:** a data entry is one line, reverts by deletion and needs no rebuild, which is what lets
it run unattended. `FURTHER` shows the guest got past something, not that the behaviour is
right, and a wrong memory write corrupts state that surfaces elsewhere. A separate file keeps
the loop's guesses apart from a person's decisions, and deleting it is a complete undo.

**Rejected:**
- A model writing Rust: no rebuild-free undo, whatever the model's quality.
- An open effect grammar: a program in a data file.
- Accepting writes on reach alone: a wrong length still moves a wall.
