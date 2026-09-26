# D374 - stat and dirent use the FreeBSD 11 layout

**Status:** assumed
**Date:** 2026-08-29

`stat` and `dirent` are written in the older layout (`freebsd11_stat`, `freebsd11_dirent`)
by default, selected by `ORBISTOUN_STAT_LAYOUT`, with the current layout one setting away. Octal
constants are harvested in TOML's `0o` form.

**Why:** the reference checkout is a newer FreeBSD than the target's user space, and these two
structures changed width and order between the two. Both layouts are in the same headers, so
only the choice is open, and the guest is the only oracle for it. The default follows the age of
the target's user space, which is a reason rather than a measurement. Numbers stable across
releases carry no such question; structure layouts do.

**Rejected:**
- The checkout's current layout: wrong file sizes with nothing failing.
- A compiled-in choice: a hypothesis nobody can test.
