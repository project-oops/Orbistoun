# 2026-09-03 - (/loop) R9 was already closed, and twenty-one new differential cases

```
tests             1974  ->  1974   (cases are data, not test functions)
differential cases 109  ->   130
```

## R9, which turned out to be done

The plan item read: *move `learn`'s D180 refusal into `check()` so hand-edited TOML cannot
bypass it.* Reading it, the refusal did look like it lived only on the `learn` path -
`KnowledgeFile::merge` returns faults and the command declines.

It does not. `FunctionKnowledge::provenance_faults` carries the same rule -

```text
records behaviour but does not say how it is known
```

- and `the_shipped_files_account_for_everything_they_claim` asserts it is empty across
`Knowledge::builtin()`, which is every committed file. `check` runs `cargo nextest run`, so
the gate is in `check` already.

**Asked rather than imitated.** A behaviour claim with no `known_by` was appended to
`libc.toml` and the suite was run:

```text
"handEditedBypass: records behaviour but does not say how it is known"
test result: FAILED. 21 passed; 1 failed
```

Restored, green again. R9 is closed and the plan was wrong about it - which is worth a line
rather than a code change, because the next reader of that plan would otherwise spend the
afternoon re-deriving it.

## Twenty-one differential cases, chosen for the bugs they catch

`strncasecmp` had one case and `strcasecmp` one. `strstr` had four, none with a repeated
prefix. `memset` had never been given a value above 127.

**Case folding, where the trap is that `[` and `{` differ by exactly 0x20.** So do `@` and the
backtick. An implementation that ORs 0x20 instead of asking whether the byte is a letter calls
them equal; glibc answers *negative*:

```text
REF|out|strcasecmp/bracket-and-brace|sign|negative
REF|out|strcasecmp/high-byte-is-greater|sign|positive
```

The second is the other half: beyond ASCII the comparison is on `unsigned char`, so `\x80` is
**greater** than `a` - backwards for anything folding through a signed `char`.

**`strstr` with repeated prefixes**, where a search that resumes from where it stopped rather
than one past where it started misses the match:

```text
strstr/repeated-prefix   aab  in  aaab       -> offset 1
strstr/false-start       abcabd in abcabcabd -> offset 3
```

Plus the bounds: needle longer than subject, a match at the final possible offset, empty
subject against a non-empty needle, and both empty.

**`memset` conversion.** The value is converted to `unsigned char`, so 255 writes `ff` and
**321 writes `41`** - a truncation, not a clamp, and not a range error:

```text
REF|out|memset/high-value|buffer|ffffff646566004040404040
REF|out|memset/value-above-a-byte|buffer|414163646566004040404040
```

Also six more `strncasecmp` bounds: a zero bound compares nothing however different the
strings, differing exactly at the bound against one past it, and a bound running past the NUL.

## All 130 agree, and that is checked rather than assumed

Orbistoun matches the reference on every one. Which on its own is the weak claim - so one of
the *new* records was corrupted and the suite run again:

```text
"strcasecmp/bracket-and-brace: sign is negative, expected zero"
```

Named, not merely counted. So the new cases are genuinely compared, and orbistoun's
`strcasecmp` really does ask whether a byte is a letter rather than reaching for 0x20.

The two standing gates did their job unchanged: `every_recorded_case_can_be_rebuilt` would
have failed had any new shape been unbuildable, and `agreed + diverges == rebuilt` ties the
comparison count to the case list, so a checker that quietly stopped comparing would fail.

**The test count did not move**, because these are data rows and not `#[test]` functions.
Saying so beats a number that implies otherwise.

## State

`cargo test --workspace` green - **117 suites, 1974 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-332 and D466-D484.

**Next**: the shared stub table (D484) - `build_thunks` over a set of modules, the
`(module, symbol index)` to slot mapping carried to relocation, then the relocation pass.
