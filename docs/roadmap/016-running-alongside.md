# Running alongside


Not phases, but tracked so they do not get lost:

- **Arity verification.** 37 provisional argument counts across the subsystem crates.
  Degrades trace fidelity rather than correctness, so it is worth a pass once phase 4
  makes wrong ones visible.
- **Housekeeping.** Unused workspace dependencies, placeholder directories, and the
  target-specific dependencies that currently compile for nothing. Listed in
  [BACKLOG.md](../BACKLOG.md).

