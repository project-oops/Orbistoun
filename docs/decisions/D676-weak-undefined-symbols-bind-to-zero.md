# D676 - Weak undefined symbols bind to zero when unanswered

**Status:** decided
**Date:** 2026-09-10

## The control, and why it failed

obSCEne's census control (`src/probe/sections/surface.c`, `check_control` in the sibling `obscene`
repo) verifies the integrity of the census mechanism. It takes the address of two symbols obSCEne
declares: `obs_census_control_present`, which it defines, and `obs_census_control_absent`, which it
declares `__attribute__((weak))` and deliberately leaves undefined.

Using `obs_address_is_callable(&symbol)`, the check asserts that the defined symbol evaluates to
present and the weak undefined symbol evaluates to absent.

On hardware (`reports/hardware/20260909-110725-eboot.obs.log`, in the same `title/unknown-gpu` leg
context), the check passes: `obs_census_control_absent` evaluates to NULL, and the report records
`OBS|res|900-surface/control|pass`.

In orbistoun prior to this decision, the check failed:
- by default, every unresolved import was given a callable stub so guest calls could be caught and
  reported, which made `obs_census_control_absent` read as **present**;
- under `ORBISTOUN_RESOLVE=named`, orbistoun refused the unnameable symbol and refused to enter the
  image at all - *"not entered - the image is not fully linked"* (D010).

Neither behavior is what dynamic linking on ELF platforms specifies.

## The ELF rule

The System V ELF gABI is unambiguous regarding weak references:
A weak undefined symbol (`STB_WEAK` in `st_info`, section index `SHN_UNDEF`) that is not satisfied
by any definition during dynamic linking resolves to **address zero** (NULL). The link succeeds;
it is neither an unresolved link error nor an abort condition.

Dynamic linking sorts weak symbols into three cases:
1. **Defined by a loaded module or the title:** Binds to the actual address of the definition.
2. **Defined by the platform runtime:** Binds to the host implementation or callable stub (e.g.
   named in the symbol database).
3. **Unanswered anywhere:** Binds to **zero** (with addend, if any, for `R_X86_64_64`; 0 for
   `R_X86_64_GLOB_DAT` and `R_X86_64_JUMP_SLOT`).

Binding to zero is the foundation of standard C feature detection:
```c
extern void optional_feature(void) __attribute__((weak));
if (optional_feature) {
    optional_feature();
}
```
If an emulator or dynamic linker treats every unanswered weak reference as a valid stub, every
feature detection guard resolves to true and enters code paths the platform does not provide.

## Implementation

The fix spans four layers:

1. **`orbistoun-elf` dynamic symbol parsing:**
   `Binding` (`Local`, `Global`, `Weak`, `Unspecified`) is parsed from `st_info >> 4` in
   `crates/orbistoun-elf/src/dynamic.rs`. Previously `st_info` was ignored when building `RawImport`.
   `RelocationTally` gains a `weak_zero: usize` counter. A relocation resolved to weak zero counts
   toward `total()` and is considered complete in `complete()`.

2. **`orbistoun-loader` relocation resolution:**
   `SymbolResolver` introduces `resolve_symbol(&self, symbol_index: u32) -> Resolution`, where
   `Resolution` distinguishes `Address(u64)`, `WeakZero`, and `Unresolved`.
   `value_for` returns `Result<RelocValue, Outcome>` where `RelocValue::WeakZero(u64)` writes zero
   (plus addend for absolute relocations, 0 for jump/glob slots).
   `apply_table` writes the value and increments `tally.weak_zero`.

3. **`orbistoun-service` interface:**
   `Service` exposes `is_named_with(&self, nid: Nid, symbols: Option<&SymbolDb>) -> bool` so callers
   can inspect whether a symbol is answered by the host platform.

4. **`orbistoun-worker` link policy:**
   `unanswered_weak_imports` collects any import where:
   - `imp.binding == Binding::Weak`;
   - `!title.bound.contains_key(&imp.symbol_index)`;
   - `!service.is_named_with(...)`.
   These imports are removed from the `unnameable` refusal set and passed into relocation as
   `weak_zero`.

## The Contradiction: Packaged Titles and SELFish

The initial diagnosis assumed that fixing weak undefined symbol resolution in orbistoun would make
`900-surface/control` pass for both `obscene-payload` and `PPSA99980`.

Testing the fix against the guest revealed a striking contradiction:
- For `obscene-payload/eboot.bin` (the raw ELF payload), `900-surface/control` **passes**:
  `OBS|sym|obscene|obs_census_control_absent|absent|shared` and `OBS|res|900-surface/control|pass`.
- For `PPSA99980/eboot.bin` (the packaged title), `900-surface/control` **still failed**:
  `OBS|sym|obscene|obs_census_control_absent|present|shared` and `OBS|res|900-surface/control|fail`.

Direct inspection of `PPSA99980/eboot.bin`'s dynamic symbol table explained why:
In `PPSA99980`, import 221 (`obs_census_control_absent`) has `st_info = 0x12` (`STB_GLOBAL` / bind = 1),
**not** `STB_WEAK` (`0x20` / bind = 2). In fact, every single one of the 244 imports in `PPSA99980`
has `bind = 1`.

The cause lies in the packager tool `SELFish` (`crates/selfish-elf/src/dynlib.rs:556` in `mkmodule`):
```rust
set_binding(&mut rebuilt, GLOBAL);
```
`mkmodule` unconditionally forces `GLOBAL` binding on every symbol it encodes into a packaged module.
Because `PPSA99980` was packaged by `mkmodule`, its weak annotations were erased on disk before
orbistoun ever received the binary.

Orbistoun's loader faithfully respects the metadata on the binary it executes: when presented with an
unstripped ELF with `STB_WEAK` preserved, it binds unanswered symbols to zero. When presented with a
binary whose imports are marked `STB_GLOBAL`, it generates stubs as required for global imports.
