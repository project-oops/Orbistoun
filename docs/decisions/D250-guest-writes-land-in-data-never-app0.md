# D250 - Guest writes land in /data, never /app0

**Status:** decided
**Date:** 2026-09-26

`/app0` mounts the title's own directory read-only; path escapes are refused by walking
components, and `..` is refused outright. `/data` resolves into storage the installation owns
and is writable. `mount::is_writable` is the whole distinction, and write intent under a
read-only mount is ignored rather than refused.

**Why:** a guest writing through `/app0` would edit the user's own title - the material being
measured. A guest chooses its path strings, and cancelling `..` against a preceding component is
unsound once symbolic links exist.

**Rejected:**
- A writable `/app0`: modifies the title under test.
- Resolving then checking a path: a guest-built path escapes the mount.
- Refusing opens that ask for write access: stops guests that asked for more than they use.
