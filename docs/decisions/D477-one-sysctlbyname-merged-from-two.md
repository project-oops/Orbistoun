# D477 - One `sysctlbyname`, merged from the two that were both live

**measured** - 2026-09-02 (user-directed plan, R0)

`sysctlbyname` was implemented **twice**: once in `orbistoun-libc` (D397, 2026-08-30) and again
in `orbistoun-kernel` (D447, 2026-09-01), the second written by somebody who did not know the
first existed. A NID is the hash of a name alone, so both claimed one identifier.

## Why nobody noticed for two days

Three guards trip on exactly this - `no_symbol_is_declared_twice`, `nids_differ_per_symbol`, and
the declared-symbol count in `every_subsystem_is_registered`. All three live in
`orbistoun-service`, because it is the only crate that can see more than one subsystem at once.

**Every batch since then ran `cargo test -p orbistoun-kernel`**, which never reaches them. The
guards were red from the day the duplicate landed and each batch reported "tests pass", which
was true of the command that was run and false of the thing it was taken to mean.

## What was actually broken

Not "one of them was dead code", which was the first guess and was wrong. `resolvable()` lays
out one stub slot per implementation and **both got a slot**, while `syscalls()` collects into a
map where the later registration wins. So the same name reached different code depending on how
a guest asked for it - an import relocation, a by-name resolve, or a syscall number.

That is worse than a duplicate. It is one symbol with two behaviours, selected by a path the
guest chooses.

## The merge, and the one place they genuinely disagreed

`orbistoun-libc`'s survives, because it is the richer of the two: three integer knobs measured
off a console (`hw.ncpu`, `hw.pagesize`, `machdep.tsc_freq`), `kern.ostype`, and unknown names
reported once so they become a work list. It absorbed what the kernel's had decided:

- **Failures answer vendor codes** rather than the crate's generic `FAILED`. A name that does
  not exist and a buffer too small for one that does are different things, and only the second
  is worth retrying with the length that is written back.
- **An unset `kern.osrelease` answers an empty NUL-terminated string** instead of refusing.

That last one is a real conflict of decisions, not an oversight: D397 refused it here and D447
answered it there. **D447 wins on the merits, not on being newer.** The knob *exists* on the
console - a run measured `0.0-prototype` in it - so refusing says "no such name", which is
false, where answering an existing knob with no value is exactly true. D447 also has the
conformance check `135-sysctl/osrelease` passing against it.

D397's reasoning is not discarded: orbistoun still does not invent a kernel version, and the
default release is still empty. What changed is what an empty one *reports*.

## The guard that was checking the wrong table, again

The path rule inside `cites` validation is now a named function,
`knowledge::fragment_is_a_path`, exported so a generator applies the same rule rather than a
second copy. The derivation had guessed at it and guessed wrong in the safe direction -
refusing any citation containing a slash, which rejects `ISO/IEC 9899`, the C standard's own
name. The rule is per whitespace-delimited fragment: a slash inside a word is ordinary, a
fragment that begins one is a path.

**The guard was not weakened to fit.** It was asked, instead of imitated.

## What to check first if this recurs

`cargo test --workspace`, from the repository root. Run from the parent directory it fails with
"could not find Cargo.toml" and exits in a way a grep pipeline swallows, which is its own small
lesson about a check that reports success for not having run.
