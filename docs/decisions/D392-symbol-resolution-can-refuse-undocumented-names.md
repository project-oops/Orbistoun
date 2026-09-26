# D392 - Symbol resolution can refuse names this project cannot derive

**Status:** assumed
**Date:** 2026-08-30

A configurable resolution mode refuses to resolve any imported symbol whose
name this project's own database cannot produce, instead of answering every
import with a stub; it is not the default entry mode.

**Why:** Every import resolves to some address by default, so a guest asking
which symbols exist gets a yes for a name no console ever exported. Refusing
exactly the names with no supporting evidence makes a conformance probe about
symbol presence honest, at the cost of resolving fewer imports than the
default mode does.

**Rejected:**
- Making the refusing mode the default: measurements already taken assume
  every import resolves, and a console does not itself refuse to link.
