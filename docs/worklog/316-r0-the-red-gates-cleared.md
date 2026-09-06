# 2026-09-02 - (/loop) R0: the red gates cleared, and the knowledge file catches up

```
cargo test --workspace   4 failures   ->   115 suites, 1921 tests, 0 failures
knowledge entries              429    ->   551 (+122)
measured/published/assumed tiers unchanged in kind; nothing was lost
```

First unit of the combined plan. The gap number did not move and was not meant to: this is the
work that makes the next gap-closing worth doing.

## Why this came first

The bulk port wrote ~215 functions across fourteen batches and every worklog since 309 said
"clippy clean, fmt clean, kernel/posix tests pass". All true. **None of it ran
`cargo test --workspace`**, where the cross-crate guards live - and those had been failing since
2026-09-01.

A narrower check reported as a broader one is the same over-claim principle 3 keeps catching,
one level up from the code. Worklogs 310-315 should be read as "the crate tests passed".

## The four failures, from two causes

### `sysctlbyname` was implemented twice, and both were live (D477)

`orbistoun-libc` (D397) and `orbistoun-kernel` (D447), the second written without knowing the
first existed. Three guards trip on it.

The first guess - "one of them is dead code" - was wrong. `resolvable()` gives **each** a stub
slot while `syscalls()` keeps the later one, so the same name reached different code depending
on whether a guest arrived by relocation, by name, or by syscall number. One symbol, two
behaviours, chosen by a path the guest picks.

Merged into one, in `libc`, keeping every deliberate behaviour from both. The one genuine
conflict - what an unset `kern.osrelease` answers - went to D447's empty-string, and **on the
merits rather than on being newer**: the knob exists on the console, so refusing it says "no
such name", which is false.

### 112 implementations had no knowledge entry

`every_implemented_function_is_written_down` exists because implementing something without
recording what was learned is how the knowledge ends up existing only in a conversation. It was
red for 112 functions - 84 added by today's batches, 28 predating them.

**Not backfilled by hand.** 112 formulaic rows would satisfy the guard and defeat it: a row
saying "POSIX function, follows POSIX" records nothing a reader did not already know. The
material was already written - each of those functions carries a doc comment stating its
contract, usually citing the specification, because that is the house style. So
`orbistoun-gen knowledge` derives the entry from that.

It reads the doc comment for the purpose, the `Reference:` line or a named standard for the
citation, and the **bolded** paragraphs for edge cases - bold being the house signal for the
load-bearing caveat, which is exactly what the edge-case field wants. Then every record goes
through `KnowledgeFile::merge`, so the format's own provenance rules decide admissibility, and
**a single fault stops the whole write** rather than being printed and ignored (D180).

122 entries derived, 429 -> 551, nothing lost (checked by name against `HEAD`).

## What the derivation refused to do

Two places where it would have been easy to manufacture provenance, and did not:

- **Documentation citing no standard lands as `assumed`**, never `published`, with an
  assumption saying it was derived from the implementation's own words. 39 of the 122.
- **A reference is not a purpose.** Plenty of these are documented as
  `` `pthread_mutex_timedlock(mutex, abstime)` - POSIX.1-2008. `` - the whole content is "it is
  the standard one". Recording that as what the function is *for* would put a clause number in
  the field a reader goes to for behaviour, so 20 entries have a citation and no purpose. That
  is what is actually known.

## Four parser bugs, each found by a guard rather than by reading

Worth listing because they are the same bug in four costumes - **a rule that matched on shape
rather than on meaning**:

1. Scanning whole files for `("name", thing)` found sixty tuples that are not registrations:
   fixture data, test vectors, a pair of column headings. Bounded to slices of
   `(&str, GuestFn)`.
2. **A row is not a line.** `cargo fmt` breaks a long one across four, so the reader silently
   lost exactly the rows with the longest names - the attribute accessors. Only the downstream
   guard noticed. This is the *second* time multi-line formatting has cost a table read today.
3. A `//` block above a macro invocation swallowed the section banner above *it*, so ten maths
   functions each inherited "Added in bulk from ISO/IEC 9899 7.12" as their purpose. A blank
   line ends a comment paragraph.
4. `inet_pton` and eleven others are **delegations** - a name resolving to another name's
   function pointer, so implemented at run time and absent at compile time. Resolved
   transitively, with a hop limit, and each records that it describes the target.

## And a guard imitated instead of asked

The derivation guessed at the rule for "a citation must not be a filesystem path" and guessed
wrong in the safe direction: it refused anything containing a slash, which rejects `ISO/IEC
9899` - the C standard's own name. The real rule is per whitespace-delimited fragment.

It is now `knowledge::fragment_is_a_path`, exported from the crate that owns the format, and
the generator calls it. **The guard was not weakened to fit**; it was asked instead of copied.

## State

`cargo test --workspace` green - 115 suites, 1921 tests. clippy `--tests` clean across the
workspace, fmt clean, identity scan clean.

**Nothing is committed.** It is past 17:00 UK, so the window has closed on a day holding
worklogs 292-316 and D466-D477.

**Next**: R1 - why only 34 of ~130 `measure` sites fired in the hardware run. Source analysis,
no console needed, and it decides whether R5's ceiling is 34 records or considerably more.
