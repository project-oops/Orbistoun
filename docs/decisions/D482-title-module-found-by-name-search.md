# D482 - A title's own module is found by the name that imports it

**Status:** decided
**Date:** 2026-09-03

When a guest imports from a module the title ships rather than the platform, the loader
finds the file by searching the title's own tree for a file whose name matches the imported
library name, case preferred but not required.

**Why:** the executable's import tables carry a bare library name and no path, so nothing in
the executable can be followed directly to a file. A fixed directory convention observed in
some titles is not guaranteed by the platform, and a loader that assumes it fails silently
for a title that differs.

**Rejected:**
- Assuming a fixed directory, such as a title's own media folder: observed in several
  titles, but not a platform guarantee.
