# D466 - Narrow typed-buffer formats decode through a dedicated packed path

**Status:** decided
**Date:** 2026-09-02

A typed-buffer access whose components are narrower than a word decodes through a separate
packed-buffer path, extended one component kind at a time, rather than through the plain
per-dword path or being refused outright.

**Why:** a packed format stores several components inside one word, which the plain path
cannot address, and a wrong conversion renders silently rather than failing. Each kind is
verified against a real device before the next is added, since that is the only way to
catch a wrong conversion here.

**Rejected:**
- Refusing every narrow format outright: correct but leaves a whole class of shaders
  unsupported with no path to close the gap.
- Extending the plain per-dword path to also unpack fields: conflates two different memory
  layouts in one function.
