# D484 - One stub table across every module, not one table each

**measured** - 2026-09-03 (falsification test in `orbistoun-thunk/tests/answers.rs`)

D483 leaves the title's own modules placed and unrelocated. Relocating them needs each one's
own imports answered, which means each one needs stubs. The obvious shape - a thunk table per
module, at its own base - **cannot work**, and the reason is worth writing down before anything
is built on it.

## What the dispatch layer actually is

Every table behind a stub is a process-global `OnceLock`, indexed by **the** module's dynamic
symbol index:

```text
HANDLERS          FLOAT_HANDLERS    STUB_RETURNS      FORCED_RETURNS
COUNTS            READABLE          WRITABLE          FORCED
DUMPED_PER_IMPORT DATA_SYMBOLS
```

`Service::build_thunks` fills them, and `install_handlers` is documented as ignoring a second
call: *two guests in one process is not something this supports*. That was written for the
main executable and it is right for that.

It means a second `build_thunks`, for a title module, would **silently do nothing** - its
handlers dropped, its `is_implemented` answers coming from the executable's table, indexed by
a symbol index belonging to a different module. Every count the run reports would be wrong,
and nothing would say so.

That is a stub-shaped failure one level up: plausible output from a table that was never
installed.

## The design

**One table, with a slot range per module.** Module *M*'s symbol *i* occupies slot
`offset(M) + i`. Every global table stays exactly as it is - one array, one index space, one
install - and gains only the property that its index space now spans more than one module.

What that buys, beyond working at all:

- **A call trace stays attributable.** One index still names one symbol of one module, so
  "called and not implemented" keeps meaning what D179 made it mean.
- **`implemented_count` stays a single number** over the whole run, which is what worklog
  reporting has always compared across turns.
- **No second address region.** The bases stay as they are; the table is simply longer.

What it costs: `build_thunks` takes a set of modules rather than one, and the mapping from
`(module, symbol index)` to slot becomes a thing that has to be carried to relocation. That is
the actual work of the next unit.

## Why not the alternative

Making each global per-module - a map from module id to table - touches every accessor on the
guest call path. Those run on a `sysv64` frame with no room for a context (which is why they
are globals in the first place), so a module id would have to be threaded through the stub
itself. It is a much larger change to reach the same place, and it makes the hot path slower
to answer a question that a flat index already answers.

## Status

**Measured, and the guard has been watched failing.** `answers.rs` now installs a handler for
a slot the first install left empty and asserts it does not take. Inverting the assertion
fails with the message naming this decision, so the check is known to be able to reject - it
is not a test that passes because nothing happened.
