# orbistoun-paths

Portable-first path resolution, under one rule: **orbistoun never writes outside its own
resolved root.**

It holds the resolution precedence (portable, then `ORBISTOUN_DATA_DIR`, then the OS standard
location), every writable location beneath the one root, and the portable sentinel.
`orbistoun-cli paths` prints the result.

## Rules

- **Portable outranks the environment override.** If an environment variable could escape the
  portable root, containment would be a suggestion rather than a guarantee.
- **The sentinel is a directory, never a file.** The sentinel and the data root are the same
  path, so a `.portable` file would make `create_dir_all` fail on first run.
  `enable_portable_sentinel` replaces a stale sentinel file with the directory.
- `resolve_with` takes its inputs rather than reading the world, so resolution is testable
  without touching real environment variables or the real binary location.
- **Containment is a test.** The suite writes through every location the API hands out and
  asserts nothing lands outside the root. `all_dirs()` drives it, so a new writable location
  that is not registered there fails a test rather than silently escaping.
