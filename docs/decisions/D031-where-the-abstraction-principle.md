# D031 - Where the abstraction principle deliberately stops

**decided** · 2026-08-19

Recorded so a later session does not try to "fix" these:

- **No execution-backend abstraction.** The guest is x86-64 and runs natively. That
  is the architecture, not an implementation detail, and no second implementation
  will ever exist to swap in.
- **No container-format plugin layer.** One vendor ELF variant. If a second ever
  mattered it would be a new parser, not a trait.

Both fail D029's test: they pay no rent now and none later.

