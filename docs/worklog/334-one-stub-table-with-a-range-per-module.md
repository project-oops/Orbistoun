# 2026-09-03 - (/loop) One stub table with a range per module

```
tests   1975  ->  1979
```

D484 established that a stub table per module cannot work - every table behind a stub is a
process-global `OnceLock` indexed by one module's symbol index, and a second install is
ignored. This builds the replacement: **one table, a slot range per module**.

## Two steps, and the first one deliberately changed nothing

`build_thunks` was a hundred and fifty lines doing four things. It was split first, with
single-module behaviour bit-identical, and the suite run before anything new was added:

```
117 suites, 1975 tests, 0 failures   - the same numbers as before the split
```

Then the driver gained a module list. Doing it the other way round would have meant a refactor
and a feature landing together, with no run in between that says which one moved a test.

What came out:

- `bind_handlers` - one module's implemented imports into the shared handler tables
- `fill_stub_returns` - one module's unimplemented answers, still by D166's three sources
- `slot_ranges` - the layout, and the only genuinely new logic

`build_thunks(bytes, base)` is now `build_thunks_for(&[("", bytes)], base)` and nothing else.

## The policy writes were the trap

`install_policy_writes` did its own collection *and* its own install, and both installs are
`OnceLock`s. Called once per module it would have kept the first module's plants and **silently
discarded every other module's** - which is D187 exactly: a setting consulted nowhere, in the
branch nobody re-read.

So it became `collect_policy_writes` filling a `PolicyPlants` value, installed once at the end
where the install can be *seen* to happen once. The region cursor moved into that value too:
two modules reserving from independent cursors would have been handed the same addresses.

This is the failure mode D484 warned about, found in the one place that would have been easy to
skip past. Naming it here because the next module to gain multi-module support will have the
same shape.

## `slot_ranges`, pure, and watched failing twice

The arithmetic is the whole of what makes one table serve several modules, so it is separated
from the parsing - principle 8's "pure decision function plus a thin effectful wrapper" - and
testable without an ELF, without the address space, and without touching the globals that can
only be filled once.

Four tests, and the two that matter were made to fail before being believed:

**Offset origin.** Starting at 1 instead of 0:

```text
left: [1, 5, 8]   right: [0, 4, 7]
```

**The advance.** Stepping by one instead of by the symbol count:

```text
left: [0, 1, 2]   right: [0, 4, 7]
```

The second is the dangerous one. A gap wastes slots harmlessly; an **overlap** binds two
modules' symbols to one stub, so a call trace names the wrong function and an implementation
answers for a symbol nobody wrote it for. The test asserts the exact boundary rather than
merely that offsets increase, for that reason.

The third test covers a module reporting zero symbols - it can happen, and giving it a slot
anyway would shift every module after it by one and misbind all of them.

## The main executable is module 0 at offset 0

Not an accident of iteration order, and worth stating because it is what makes this change
invisible to everything already measured: every recorded call index, every `implemented_count`
compared turn to turn, and every hardware measurement taken before today names a slot in the
executable's range. Any other offset would renumber all of them silently.

## State

`cargo test --workspace` green - **117 suites, 1979 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-334 and D466-D485.

**Next**: the relocation pass. Each title module relocates against a resolver mapping its symbol
*i* to slot `offset(M) + i`, in D482's order - place all, collect all exports, relocate all,
main executable last. `install_data_symbols` is the remaining `OnceLock` on that path, so every
module's named data symbols have to be merged the way the policy plants now are.
