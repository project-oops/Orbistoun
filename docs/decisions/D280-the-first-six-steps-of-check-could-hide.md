# D280 - The first six steps of `check` could hide the other ten


**decided** · 2026-08-25 · found by one guard failing and taking the gate with it

D192 established the shape: a tree with six problems should report six, not one per run.
`must` implements it - each step runs in a tested context, failures accumulate, the exit
status still reflects them.

**Six steps were not going through it.** `provenance`, `decisions`, `prose`,
`generated_numbers`, `symbols_audit` and `tables` are called bare, so under `set -e` the
first one to fail ends the script - and `cargo fmt`, `clippy`, `check` and the entire test
suite never run.

That is worse than a missing report. It is a gate that **exits having tested almost nothing**,
after printing one line about one file, and the run reads as though the tree was examined. A
session ended with a watchpoint implementation whose tests had not been run, believing the
gate had looked at it, because a symbol database in a directory the implementation never
touches was a megabyte too large.

`also` wraps a bare step the way `must` wraps a command, for the steps that print their own
heading. Same accumulation, same exit status, no stopping.

**And that alone was not enough**, which is worth recording because the half-fix looked like
a whole one. Every one of those six steps reports failure with `die`, and `die` calls `exit`
- so wrapping the *call* changes nothing, because there is no return value to catch. The
gate stopped at exactly the same place with `also` in front of it.

So `bad` sits beside `die`: same red line, `return 1` instead of `exit 1`. `die` stays for
the cases where stopping is right - an unknown verb, a missing binary, a `run` with no such
title - because those are setup problems where continuing would produce noise rather than
findings. A gate step is the opposite: continuing is the entire point.

