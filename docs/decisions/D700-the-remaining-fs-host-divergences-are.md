# D700 - the remaining fs host-divergences are measured near-zero-reachability, so the verbatim chokepoint is declined

**Status:** decided
**Date:** 2026-09-16

## The choice

Worklog 624 left an action item: finish auditing the `orbistoun-fs` host-call sites for the remaining
known Windows/console divergences - path length (`MAX_PATH`), reserved device names (`CON`, `NUL`, …)
and trailing dot/space stripping - after case (624) and directory-open (615) were closed. The audit is
done, **by measurement** rather than by reading: a probe exercising Rust's own `std::fs` (the exact
API `orbistoun-fs` calls, not the MSYS layer) on this Windows 11 machine. The choice is what to do with
the result, and it is to **record the measurement and decline the `\\?\` verbatim-path chokepoint that
would close the two leaks that do reproduce**, because they are unreachable by any path a real title
uses.

## What was measured

Through `std::fs::write`/`read`/`create_dir_all` into a temp directory:

- **`MAX_PATH`: does not reproduce.** A 441-character path created and read back fine with no
  `\\?\` prefix - this machine has long-path support enabled, so the 260 limit does not bite.
- **Reserved names: almost entirely do not reproduce.** `con`, `prn`, `aux`, `com1`, `lpt1`, and every
  reserved name *with an extension* (`aux.txt`, `con.dat`, `nul.png`) each created a **real file** and
  read back its own bytes. Only **bare `nul`** leaked: the write went to the null device and read back
  empty. Windows 11's device redirection is far narrower than the classic list.
- **Trailing dot/space: reproduces.** Writing `trail.` created `trail` (the dot stripped), and both
  spellings then resolve to the one file - where the console (FreeBSD) keeps `trail.` and `trail`
  distinct.

So of the class, exactly two leaks are live here: **bare `nul`** and **trailing dot/space**. Both are
exotic: `/app0/il2cpp_data/Metadata/global-metadata.dat` and every other path the Unity titles walk
contain no bare `nul` component and no trailing dot or space. No current title reaches either.

## Why decline the fix rather than ship it

The one fix that closes both is the `\\?\` verbatim prefix, which passes a host path to the filesystem
unmangled. Applying it means threading a verbatim `PathBuf` through `mount::resolve`, whose result is
consumed by ~10 callers (`open`, `create`, `stat`, `opendir`, `statfs`, `is_directory`, …), some of
which do path arithmetic that a `\\?\` prefix would change. Gating it to only the exotic components
contains the blast radius, but:

- **The reachability is measured near-zero** - no title path hits either leak (principle 11: this is a
  shortcut in reverse, spending real risk on fidelity for inputs nothing produces; principle 12: a
  seam that pays off only hypothetically is speculation).
- **CI cannot verify it.** The full suite runs on Linux, where the whole concern is a no-op; only this
  Windows machine exercises it, and an unattended run cannot audit every caller's path arithmetic
  against a `\\?\` path with the confidence the change needs.

The measurement is the payoff: it converts an open, vague "audit ~90 sites" into a precise result, and
it **eliminates the fs-host-leak hypothesis for the `int 0x41` titles** - their paths do not hit the
two live leaks, so whatever sends GTA and Terminator to the il2cpp assertion (D699, worklog 620), it is
not `orbistoun-fs` passing a host answer through. That is a real narrowing of the `0x41` hunt, which is
what this tick was for.

## When this reverses

If a title is ever observed to create or open a path with a bare `nul` component or a trailing dot or
space, the fix is the gated `\\?\` chokepoint above, verified on this Windows machine with a made-to-
fail test (create `nul`/`trail.`, assert a real, distinct file). Filing that observation reopens this;
until then, plumbing it is the plausible-output principle 3 forbids one level up - fidelity claimed for
a case never exercised.

The durable structural fix from worklog 624 - **run the full `cargo test` on the Windows runner** so a
divergence in a *tested* path fails CI instead of a title - stands regardless, and is the higher-value
next step for this class because it needs interactive CI iteration this loop cannot do.
