# orbistoun-paths

Portable-first path resolution. One rule: **orbistoun never writes outside its own
resolved root.**

**Models:** the resolution precedence (portable → `ORBISTOUN_DATA_DIR` → OS standard),
every writable location beneath one root, and the portable sentinel.

**Deliberately fakes:** nothing.

**Design note.** Portable outranks the environment override on purpose - if an env var
could escape the portable root, containment would be a suggestion rather than a
guarantee.

The sentinel is a **directory, never a file**. The sentinel and the data root are the
same path, so a `.portable` *file* makes `create_dir_all` fail on first run - a
sibling project shipped exactly that bug. The directory's own existence is the
sentinel, and `enable_portable_sentinel` heals a stale file left by the older scheme.

`resolve_with` takes its inputs rather than reading the world, so resolution is fully
testable without touching real environment variables or the real binary location.

**Containment is a test, not a convention** - the suite writes through every location
the API hands out and asserts nothing landed outside the root. `all_dirs()` drives it,
so a new writable location that forgets to register there fails a test rather than
silently escaping.

**Status:** complete.
