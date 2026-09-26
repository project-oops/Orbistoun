# D661 - Titles, payloads and packages roots

**Status:** decided
**Date:** 2026-09-09

The data directory has three roots: `titles/` for installed titles, `payloads/` for single
executables run directly, and `packages/` for uninstalled inputs. A corpus source names its
`target` root, defaulting to `titles`, and the other two roots derive from the titles root.

**Why:** the three are different things, and one root made one-file payloads list as installed
titles. Registering the roots through the path model's single registration point makes portable
mode and every directory listing follow automatically. Deriving them from the titles root means an
override of that root moves all three together.

**Rejected:**
- One `titles/` root for everything: payloads and packages read as titles.
- Resolving each root independently: an override moves one and the others keep writing to the real
  data directory.
