# 498. Weak undefined symbols bind to zero

**2026-09-10** - loop, completing 496

Fixed orbistoun's handling of weak undefined symbols. `900-surface/control` on
`obscene-payload` now passes, matching hardware.

## The control and the fix

`900-surface/control` is obSCEne's sanity check on its census: it declares a defined symbol
`obs_census_control_present` and an undefined weak symbol `obs_census_control_absent`.
Hardware evaluates the second as absent (NULL) and passes `900-surface/control`.
Orbistoun was giving every unresolved import a callable stub, making the weak undefined symbol
read as present.

Under the ELF gABI, a weak undefined symbol that is not answered binds to zero and linking succeeds.

The fix followed TDD across the workspace:
1. `orbistoun-elf::dynamic::Binding`: parsed from `st_info >> 4`. Added `RelocationTally.weak_zero`.
2. `orbistoun-loader::relocate`: introduced `Resolution` (`Address`, `WeakZero`, `Unresolved`) on
   `SymbolResolver`. Relocations for `WeakZero` write 0 (plus addend for `ABS64`) and increment
   `tally.weak_zero`.
3. `orbistoun-service`: exposed `is_named_with` to check if a symbol is answered by the host platform.
4. `orbistoun-worker`: `unanswered_weak_imports` identifies imports that are `Binding::Weak`, not in
   `title.bound`, and not answered by `service.is_named_with`. These are passed to relocation as
   `weak_zero` and removed from `unnameable` refusals.

## Guest verification

Running `obscene-payload/eboot.bin`:
- Before:
  `OBS|sym|obscene|obs_census_control_absent|present|shared`
  `OBS|res|900-surface/control|fail`
- After:
  `OBS|sym|obscene|obs_census_control_absent|absent|shared`
  `OBS|res|900-surface/control|pass|||assumed`

## Surprises: Packaged titles and SELFish

The initial diagnosis assumed that fixing weak undefined symbol handling in orbistoun would make
`900-surface/control` pass for both `obscene-payload` and `PPSA99980`.

It did not pass for `PPSA99980`:
`OBS|sym|obscene|obs_census_control_absent|present|shared`

Inspecting `PPSA99980/eboot.bin`'s dynamic symbol table revealed that import 221
(`obs_census_control_absent`) has `st_info = 0x12` (`STB_GLOBAL` / bind = 1), not `STB_WEAK`
(`0x20` / bind = 2). In fact, all 244 imports in `PPSA99980` have `bind = 1`.

The packager tool `SELFish` (`crates/selfish-elf/src/dynlib.rs:556` in `mkmodule`) unconditionally
overwrites symbol bindings:
`set_binding(&mut rebuilt, GLOBAL);`

So `PPSA99980` had its weak annotations clobbered to `STB_GLOBAL` on disk by `mkmodule`. Orbistoun
faithfully obeys the ELF metadata it is given: global imports receive stubs; weak imports bind to
zero when unanswered.

## Status and gate

- D676 written and decided.
- `cargo test --workspace` all green.
- Conformance frontier diff clean.
- Identity guard clean.