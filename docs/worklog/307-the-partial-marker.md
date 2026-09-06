# 2026-09-02 - (/loop) A partial marker, and the inconsistency the audit found

The user asked how "implemented but not complete" gets caught. It was not being caught: the gap
report counts whether a symbol resolves to code, and three functions that say in their own doc
comments what they do not do were counted as finished. D474 has the argument; this is what
changed.

## The marker

`partial = "what is not done"` on `FunctionKnowledge`, parsed from the knowledge files, and a
fourth row in `worklist --static-gap`:

```
3 implemented but declared partial - present, and not finished:
  getopt        An option that is actually present is not parsed ...
  unsetenv      Removes only what this layer holds ...
  scePthreadMutexInit  The attribute block is not parsed, so the recursion mode is the default ...
```

Prose rather than a boolean, because the *shape* of each gap differs and a flag would flatten
"does not parse attributes" and "cannot remove what came from the process image" into one thing
with one fix. And **empty means undeclared, not complete** - the report says so in those words
when the list is empty, rather than announcing that everything is finished.

## What the audit turned up

Grepping the implementation crates for comments admitting incompleteness found the three above
and one more that was no longer a caveat but a **bug**: `vfprintf` ignored its stream argument
entirely and always wrote to the host's error stream.

That was defensible when both of a guest's standard streams landed there anyway - and stopped
being defensible in worklog 304, when `_Stdout` and `_Stderr` became real wrapped descriptors and
`fprintf` started routing by them. From that moment `fprintf(stdout, ...)` reached descriptor 1
and `vfprintf(stdout, ...)` did not. **One of a pair honouring a distinction the other ignores is
worse than neither doing it**, because the output looks right until it does not, and nothing in
the report would ever have said so. Fixed: `vfprintf` now routes exactly as `fprintf` does.

That is the argument for the marker in miniature. The prose caveat was accurate when written,
went stale when something else changed, and nothing was watching it. A counted field at least
puts it in front of a reader every time the report runs.

## State

clippy `--tests` clean, which needed three fixes of my own making: doc comments on macro
invocations do not attach (the math batch's per-function references), a doc comment orphaned from
`implementations()` by an inserted block, and `float_cmp` in the math tests - the last silenced
with `#[expect(..., reason = ...)]` rather than loosened, because **exactness is the property
under test**: an epsilon would let exactly the wrong answers through.

fmt clean, orbistoun-hle and orbistoun-libc tests pass, identity scan clean, nothing committed.
Gap unchanged at 374 - this tick added no implementations, it made an existing number honest.

**Next**: the 257 genuinely-absent libScePosix names, the largest remaining block.
