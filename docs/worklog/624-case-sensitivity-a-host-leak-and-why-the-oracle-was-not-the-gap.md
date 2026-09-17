# 624. Case-sensitivity was a host leak, and why having the FreeBSD oracle did not prevent it

**2026-09-16** - a second fs host-leak of the `/app0`-directory class, found by auditing the surface
the directory bug pointed at; and the reframe it forces about where the remaining fidelity gaps are

## The bug, and that it is the same class as the directory one

The console's filesystem is FreeBSD-derived and **case-sensitive**: `GAME.BIN` is a different name
from `game.bin`, and a guest that opens one when only the other exists is answered `ENOENT`. On a
**Windows** host `Path::exists` and `File::open` are case-*in*sensitive, so orbistoun opened the
wrong-case file and handed the guest a descriptor the console never would. A confirmed leak - a test
that opens `/app0/GAME.BIN` against a `game.bin` returned `Some(4)` before the fix.

This is the **same class** as PPSA04263's `/app0` directory open (worklog 615): a place where
orbistoun's guest-facing behaviour passes the host OS's semantics through instead of the console's. It
was found by doing what the directory bug called for - auditing the fs surface for host divergence -
and the grep found it in one line: **zero case handling anywhere in `orbistoun-fs`.**

## The fix, and why it needed a resolver split rather than a one-liner

`mount::resolve` is a **path mapper**: it answers where a guest path lives whether or not the file
exists yet, because a *create* needs the writable location for a not-yet-there file. So the naive fix
(make resolve existence-checking) broke creates and the mapping tests. The correct shape is two
resolvers:

- `resolve` - the mapper, unchanged, for creates (`open`-for-write, `mkdir`, `rename`'s destination).
- `resolve_existing` - answers only a path that **exists case-sensitively**, `None` otherwise, for
  every read (`open`, `stat`, `opendir`, `access`, `statfs`, `is_directory`). On Windows it
  `canonicalize`s and compares the guest's trailing components against their real on-disk case; on
  Unix `exists` is already case-sensitive, so it is a plain check.

Reads route to `resolve_existing`; writes stay on `resolve`. The case check touches only the guest's
own components - orbistoun's host root case is orbistoun's concern.

## RE #2: we have the FreeBSD oracle - so why is this happening?

The fair challenge: if the FreeBSD source (and the public PM4 format) are already done, how are there
still fidelity gaps? The answer is the distinction that matters for where effort goes:

**The oracle gives the specification; the bug is the implementation violating a spec we already know.**
FreeBSD told us the filesystem is case-sensitive - we were never missing that. The gap is that our
implementation, running on a *non-FreeBSD host*, leaked the host's case-insensitivity. Same for the
directory open, and the same shape it would take for PM4: knowing the packet format does not stop a
builder being wired to emit the wrong one. So the remaining fidelity gaps are **not** missing oracle
knowledge. They are two much narrower things:

1. **Host-semantic leaks** - a known spec, implemented in a way that passes the host OS through
   (case, directory opens, path length, reserved names). These are found by auditing where
   `orbistoun-fs`/etc. call host APIs, and closed by translating to console semantics.
2. **Genuinely vendor functions** - `sceAgc`, `sceNp`, … with no public source, where probing is the
   only oracle.

The `int 0x41` on the Unity titles is most likely (1) or (2), *not* a POSIX call we got wrong from the
source - because for POSIX-shaped calls the source is definitive and we have it. That sharpens the
hunt: audit the host-leak surface first, because it is finite and mechanical, before assuming a vendor
measurement is needed.

## The structural fix for the class (RE #1): CI parity, not vigilance

This class is invisible to CI because **the full test suite runs on `ubuntu-latest`** (ci.yml:50-192)
and only a smoke matrix touches Windows - and on Linux both of these bugs *pass* (the host is
case-sensitive and opens directories). The durable fixes, logged here as the action items:

- **Run the full `cargo test` on the Windows runner**, so any host divergence in a *tested* path fails
  CI instead of a title. (The dev machine is Windows, which is why these surface at all - CI does not
  see them.)
- **Finish auditing the ~90 host-call sites in `orbistoun-fs`** for the remaining known divergences:
  path length / `MAX_PATH`, Windows reserved names (`CON`, `NUL`), trailing dot/space stripping.
- **Structural rule**: `orbistoun-fs` must translate console semantics, never pass host behaviour
  through - a one-line "this matches the console because…" at each host call turns an invisible
  assumption into a checkable claim, the way `// SAFETY:` does for `unsafe`.

## Made to fail

- `a_wrong_case_path_is_not_found_because_the_console_is_case_sensitive` (fs) - opening `/app0/GAME.BIN`
  against a `game.bin` answers `None`, not a descriptor. Green on Unix before and after (case-sensitive
  host); it pins the fix on Windows, where the bug lived. Confirmed it failed (`Some(4)`) before the
  fix and passes after.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,368 pass / 0 fail, worklogs unique, identity scan clean.
