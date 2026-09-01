# D260 - The audit reads the grammar the binary was built with, not the one on disk


**decided** · 2026-08-25 · time lost to a stale executable

`vendor.toml` reaches the code through `include_str!`, so a release binary carries the
grammar as it stood when it was compiled. Editing the file and running `audit` against the
old binary compares a current database with a stale grammar, and every difference is
reported as an unaccounted name.

Observed directly: a binary built four minutes before a promotion reported ten unaccounted
names, of which two were the very names the promotion had just made derivable. Rebuilding
first changed the answer completely.

**Any change to `crates/orbistoun-names/data/*` requires a rebuild before the audit means
anything.** `./orbistoun.sh names` does this already; running the CLI by hand does not.

