# 2026-09-03 - (/loop) The stub tables are global, so there is one of them (D484)

```
tests   1974  ->  1974   (an assertion inside an existing test, not a new one)
```

Started the relocation pass for the title's own modules, got two constants into
`orbistoun-worker` for per-module stub tables, and then read the dispatch layer properly and
took them straight back out.

## Why a table per module cannot work

Everything behind a stub is a process-global `OnceLock`, indexed by **the** module's dynamic
symbol index:

```text
HANDLERS   FLOAT_HANDLERS   STUB_RETURNS   FORCED_RETURNS   COUNTS
READABLE   WRITABLE         FORCED         DUMPED_PER_IMPORT DATA_SYMBOLS
```

`install_handlers` is documented as ignoring a second call - *two guests in one process is not
something this supports* - which is right for the main executable and fatal for a second
module. A per-module table would be **built and silently dropped**, and every count taken
afterwards would be indexed against the wrong module.

That is a stub-shaped failure one level up: plausible output from a table nobody installed.

## Which is now measured, not reasoned about

Reading it was enough to stop building, but not enough to write down as fact. `answers.rs` -
deliberately one test, because these `OnceLock`s forbid splitting it - now installs a handler
for a slot the first install left empty and asserts it does not take.

Then the assertion was inverted, and it failed:

```text
a second install took effect, so a per-module stub table would work and D484 is wrong
```

So the check is known to be able to reject. D484 went in as `assumed` and is now `measured`,
which is the whole point of the field.

**The test count did not move**, because this is an assertion inside an existing test rather
than a new one. Saying so beats a number that implies more than happened.

## The design that replaces it

**One table, with a slot range per module.** Module *M*'s symbol *i* occupies slot
`offset(M) + i`. Every global stays one array with one index space and one install, and gains
only the property that the index space now spans several modules.

- A call trace stays attributable - one index still names one symbol of one module (D179).
- `implemented_count` stays a single number, which is what worklog reporting compares turn
  to turn.
- No second address region; the table is simply longer.

The cost is that `build_thunks` takes a set of modules, and the `(module, symbol index)` to
slot mapping has to be carried through to relocation. That is the next unit's actual work.

The alternative - a map from module id to table - was rejected: those accessors run on a
`sysv64` frame with no room for a context, which is *why* they are globals, so a module id
would have to be threaded through the stub itself. Much larger, slower on the hot path, same
destination.

## What was taken back out

`TITLE_THUNK_BASE`, `TITLE_DATA_BASE`, `TITLE_TABLE_STRIDE` and `MAX_TITLE_MODULES` - all four
described per-module tables and are wrong under the shared one. `TITLE_MODULE_BASE`, where the
images themselves go, is correct and stays.

Removing them was the point of the tick: constants that describe a design nobody can build are
worse than no constants, because the next reader takes them as evidence the design was chosen.

## State

`cargo test --workspace` green - **117 suites, 1974 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-331 and D466-D484.

**Next**: the shared stub table - `build_thunks` over a set of modules, a slot mapping carried
to relocation, then the relocation pass itself. Only after that can a placed module be bound
into a run.
