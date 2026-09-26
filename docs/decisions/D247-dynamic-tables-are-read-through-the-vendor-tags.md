# D247 - Dynamic tables are read through the vendor tags

**Status:** decided
**Date:** 2026-08-25

The ELF loader locates the string, symbol, hash and relocation tables through the vendor dynamic
tags, as offsets into the dynamic data segment, whenever a module names all three core vendor
tables; otherwise it uses the standard tags. Presence is what the parser saw, never a test on
the value.

**Why:** the platform's loader ignores the standard tags, and a module built the way the platform
expects carries only the vendor set. A vendor tag is an offset where a standard tag is an
address, and resolving one as the other lands on plausible wrong bytes. Offset zero is a real
table position.

**Rejected:**
- Standard tags first: works only for modules that happen to carry both sets.
- Zero meaning absent: rejects a table at the start of the segment.
