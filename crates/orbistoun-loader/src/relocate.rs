//! Applying relocations to a placed image.
//!
//! This is where D005 stops being a design statement and becomes machine code: writing
//! a host address into a PLT slot **is** the interception. There is no hooking pass,
//! and there never will be one - the guest calls whatever the slot contains, and the
//! slot contains what this module put there.
//!
//! # What gets written
//!
//! - `RELATIVE`: an internal pointer, adjusted by the placement base.
//! - `ABS64` / `GLOB_DAT`: a symbol's address.
//! - `JUMP_SLOT`: a function address - the import case.
//!
//! # What deliberately does not
//!
//! TLS relocations need thread-local storage to exist. They are **counted and
//! reported**, never skipped silently: an unrelocated pointer looks valid and is not,
//! so a guest that limps past one fails somewhere unrelated and much later.

use orbistoun_elf::reloc::{Elf64Rela, RelocationTally, kind, parse_table};
use orbistoun_elf::{Container, dynamic::DynamicInfo};

use crate::tls::{self, TlsLayout};
use crate::{Image, LoadError};

/// Resolves a dynamic symbol index to the address the guest should see.
///
/// Returning `None` means the symbol is unresolved - the relocation is then counted
/// rather than applied, so the tally says exactly how much of the image is not ready.
pub trait SymbolResolver {
    /// The address for a symbol index, or `None` if it cannot be resolved.
    fn resolve(&self, symbol_index: u32) -> Option<u64>;
}

impl<F: Fn(u32) -> Option<u64>> SymbolResolver for F {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        self(symbol_index)
    }
}

/// A table of per-import stubs resolves a symbol to that symbol's own stub.
///
/// This is what makes a call trace say *which* import the guest wanted. A single
/// shared address answers only "something unimplemented was called", which is worth
/// very little by comparison.
impl SymbolResolver for orbistoun_thunk::ThunkTable {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        self.address_of(symbol_index as usize)
    }
}

/// Data first, then thunks.
///
/// **The composition is the point.** A function is unaffected by this existing: it misses
/// in the data blocks and falls through to its own stub, exactly as before. An import that
/// names an object gets storage instead of a stub, which is the difference between a guest
/// dereferencing a null it can check and a guest dereferencing x86 instruction bytes it
/// cannot (D307).
#[derive(Debug, Clone, Copy)]
pub struct ImportResolver<'a> {
    /// Stubs, for imports that name code.
    pub thunks: &'a orbistoun_thunk::ThunkTable,
    /// Storage, for imports that name data.
    pub data: &'a orbistoun_thunk::DataBlocks,
    /// Imports to leave unresolved, by index.
    ///
    /// # Why refusing is sometimes the accurate answer
    ///
    /// Every import gets a stub, so that a call to something unimplemented is *reported*
    /// rather than a jump into a zeroed slot. That is the whole interception model and it is
    /// right for measuring.
    ///
    /// It also means **the platform answers yes to every symbol that has ever been asked
    /// about**. A guest cannot tell a function this emulator implements from one no console
    /// ever exported, because both resolve to an address. The conformance probe caught it
    /// with a control - a symbol that cannot exist, reported present - and said the obvious
    /// thing: *every count in this section is meaningless*. It also explains two other
    /// findings at once, because a probe that infers a machine's kind or its generation from
    /// which symbols are there gets **both** answers from a loader that stubs everything
    /// (D392).
    ///
    /// [`None`] refuses nothing, which is the default and the behaviour every recorded
    /// measurement was taken under.
    pub refuse: Option<&'a std::collections::BTreeSet<usize>>,
}

impl SymbolResolver for ImportResolver<'_> {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        let index = symbol_index as usize;
        if self.refuse.is_some_and(|refuse| refuse.contains(&index)) {
            // Unresolved, which the relocation tally already counts and reports - refusing
            // is not a new outcome here, it is one that had no way of being chosen.
            return None;
        }
        self.data
            .address_of(index)
            .or_else(|| self.thunks.address_of(index))
    }
}

/// A resolver that answers every symbol with one address.
///
/// Used before per-import thunks exist: every import points at a single host function
/// that reports being called. Crude, but it makes the image *complete* - and a guest
/// that reaches an unimplemented stub and says so is far more useful than one that
/// jumps to a zeroed slot and dies with no explanation.
#[derive(Debug, Clone, Copy)]
pub struct SingleTargetResolver {
    /// Address every symbol resolves to.
    pub target: u64,
}

impl SymbolResolver for SingleTargetResolver {
    fn resolve(&self, _symbol_index: u32) -> Option<u64> {
        Some(self.target)
    }
}

/// Computes the value one relocation should write, or why it cannot.
///
/// Split out from the writing so the arithmetic is testable without mapping anything -
/// the pattern D016 exists to encourage.
///
/// `tls` is the module's own thread-local layout, when it declares one. Passing `None`
/// makes every thread-local relocation report as deferred rather than guessing at an
/// offset into a block that does not exist.
pub fn value_for(
    entry: &Elf64Rela,
    base: u64,
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
) -> Result<u64, Outcome> {
    let addend = entry.addend.get();
    match entry.kind() {
        kind::RELATIVE => Ok(base.wrapping_add(addend as u64)),
        kind::ABS64 => resolver
            .resolve(entry.symbol_index())
            .map(|s| s.wrapping_add(addend as u64))
            .ok_or(Outcome::Unresolved),
        kind::GLOB_DAT | kind::JUMP_SLOT => resolver
            .resolve(entry.symbol_index())
            .ok_or(Outcome::Unresolved),
        _ if entry.is_tls() => tls_value_for(entry, addend, tls),
        _ => Err(Outcome::Unsupported),
    }
}

/// The thread-local cases, split out to keep the main match readable.
///
/// Only the module's **own** block is handled. A relocation naming another module
/// needs a descriptor table and a second loaded image, neither of which exists - so it
/// is reported rather than answered with a plausible number.
fn tls_value_for(entry: &Elf64Rela, addend: i64, tls: Option<&TlsLayout>) -> Result<u64, Outcome> {
    let Some(layout) = tls else {
        return Err(Outcome::TlsDeferred);
    };
    if entry.symbol_index() != 0 {
        return Err(Outcome::TlsDeferred);
    }
    match entry.kind() {
        // Which module the variable belongs to. Ours is the only one loaded.
        kind::DTPMOD64 => Ok(tls::MAIN_MODULE_ID),
        // An offset within that module's block, so the addend needs no adjustment.
        kind::DTPOFF64 => Ok(addend as u64),
        // Measured from the thread pointer, and therefore negative - the block sits
        // below it. Writing the unadjusted offset here is the classic variant II
        // mistake and reads memory that belongs to the control block.
        kind::TPOFF64 => Ok(layout.tp_offset(addend as u64) as u64),
        _ => Err(Outcome::Unsupported),
    }
}

/// Why a relocation was not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Needs thread-local storage, which does not exist yet.
    TlsDeferred,
    /// A relocation type this loader does not implement.
    Unsupported,
    /// The symbol could not be resolved.
    Unresolved,
}

/// Applies every relocation in a container to its placed image.
///
/// Returns a tally rather than a bare success: knowing *how many* entries were left
/// unapplied, and why, is the difference between "this image is not ready" and "this
/// image loaded and then behaved strangely".
pub fn apply(
    image: &Image,
    whole: &[u8],
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
) -> Result<RelocationTally, LoadError> {
    let container = Container::parse(whole)?;
    let Some(dyn_bytes) = container.dynamic_bytes(whole)? else {
        // No dynamic table means nothing to relocate. That is a legitimate image, not
        // a failure - a static binary has no relocations.
        return Ok(RelocationTally::default());
    };
    let info = DynamicInfo::parse(dyn_bytes);

    let mut tally = RelocationTally::default();
    for (addr, size) in [(info.rela, info.relasz), (info.jmprel, info.pltrelsz)] {
        if addr == 0 || size == 0 {
            continue;
        }
        // Through the container rather than by virtual address: under the vendor's dynamic
        // tags these are offsets into the data segment, and reading them as addresses
        // produced two relocations whose types decoded as unsupported (D247).
        let Some(at) = container.table_offset(whole, &info, addr)? else {
            continue;
        };
        let len = usize::try_from(size).unwrap_or(0);
        let Some(table_bytes) = whole.get(at..at.saturating_add(len)) else {
            continue;
        };
        apply_table(&parse_table(table_bytes), image, resolver, tls, &mut tally)?;
    }
    Ok(tally)
}

/// Applies one parsed table, accumulating into `tally`.
fn apply_table(
    table: &[Elf64Rela],
    image: &Image,
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
    tally: &mut RelocationTally,
) -> Result<(), LoadError> {
    let (span_base, span_len) = image.span();
    let base = image.base();

    for entry in table {
        let value = match value_for(entry, base, resolver, tls) {
            Ok(v) => v,
            Err(Outcome::TlsDeferred) => {
                tally.tls_deferred += 1;
                continue;
            }
            Err(Outcome::Unsupported) => {
                tally.unsupported += 1;
                continue;
            }
            Err(Outcome::Unresolved) => {
                tally.unresolved += 1;
                continue;
            }
        };

        let target = base.wrapping_add(entry.offset.get());
        // Every write must land inside the span this image owns. A relocation pointing
        // outside it is a corrupt or hostile table, and honouring it would scribble on
        // unrelated memory.
        if target < span_base || target.saturating_add(8) > span_base.saturating_add(span_len) {
            return Err(LoadError::RelocationOutOfBounds {
                target,
                span_base,
                span_len,
            });
        }

        let ptr = usize::try_from(target).map_err(|_| LoadError::AddressTooLarge(target))?;
        // SAFETY: `target` was just checked to lie wholly inside the image's span,
        // which the image holds a live reservation for and exclusively owns. The write
        // is unaligned-safe by construction.
        unsafe {
            std::ptr::with_exposed_provenance_mut::<u64>(ptr).write_unaligned(value);
        }
        tally.applied += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Addresses these tests reserve, taken rather than chosen.
    ///
    /// # Why this is one static for the module and not one per test
    ///
    /// [`Range::take`] hands out an address no other caller **of that instance** will get -
    /// its cursor is atomic, so concurrent takes are safe. Two tests each declaring their own
    /// function-local `static RANGE` get two instances, two cursors, both starting at zero,
    /// and therefore the *same* addresses. Whichever ran second failed to reserve its pages.
    ///
    /// It failed only when those two happened to overlap, so it passed alone and passed most
    /// of the time in a suite - which is the shape this project already refuses elsewhere: an
    /// intermittently-failing gate teaches you to re-run until green, and that is how a real
    /// failure gets waved through.
    static RANGE: orbistoun_mem::test_bases::Range =
        orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::LOADER);

    /// **Data wins, and a function is untouched by the composition existing.**
    ///
    /// The whole safety of D307's fix is that it changes nothing for code: an import that
    /// names a function misses in the data blocks and falls through to its own stub,
    /// exactly as before. If that stopped being true, every guest would break at once.
    #[test]
    fn a_data_import_resolves_to_storage_and_a_function_still_resolves_to_its_stub() {
        use super::{ImportResolver, SymbolResolver};

        let thunks =
            orbistoun_thunk::ThunkTable::build(RANGE.take(), 8, 0x1000).expect("a thunk table");
        let data =
            orbistoun_thunk::DataBlocks::build(RANGE.take(), &[(5, "optarg".to_owned())], 0x1000)
                .expect("storage");

        let resolver = ImportResolver {
            thunks: &thunks,
            data: &data,
            refuse: None,
        };

        assert_eq!(
            resolver.resolve(5),
            data.address_of(5),
            "an import naming data gets storage, never a stub"
        );
        assert_eq!(
            resolver.resolve(2),
            thunks.address_of(2),
            "an import naming code is unaffected by any of this"
        );
        assert_ne!(
            resolver.resolve(5),
            thunks.address_of(5),
            "and the two are emphatically not the same address"
        );
    }

    /// **A refused import resolves to nothing**, which is what a console does with a symbol
    /// no library exports.
    ///
    /// The failure this protects against is the silent direction: an import that resolves
    /// anyway tells a guest the symbol is present, and a presence census then counts it
    /// (D392).
    #[test]
    fn a_refused_import_resolves_to_nothing() {
        use super::{ImportResolver, SymbolResolver};

        let thunks = orbistoun_thunk::ThunkTable::build(RANGE.take(), 8, 0x1000).expect("stubs");
        let data = orbistoun_thunk::DataBlocks::build(RANGE.take(), &[], 0x1000).expect("storage");
        let refuse: std::collections::BTreeSet<usize> = [3].into_iter().collect();

        let refusing = ImportResolver {
            thunks: &thunks,
            data: &data,
            refuse: Some(&refuse),
        };
        assert_eq!(refusing.resolve(3), None, "the refused one");
        assert!(refusing.resolve(4).is_some(), "and only that one");

        let permissive = ImportResolver {
            thunks: &thunks,
            data: &data,
            refuse: None,
        };
        assert!(
            permissive.resolve(3).is_some(),
            "refusing nothing is the default, and every recorded measurement is under it"
        );
    }

    use super::{Outcome, SingleTargetResolver, SymbolResolver, value_for};
    use crate::tls::{MAIN_MODULE_ID, TlsLayout};
    use orbistoun_elf::reloc::{kind, parse_table};

    /// Builds a relocation entry. **Generated, never extracted** (D051).
    fn rela(offset: u64, sym: u32, k: u32, addend: i64) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&offset.to_le_bytes());
        v.extend_from_slice(&((u64::from(sym) << 32) | u64::from(k)).to_le_bytes());
        v.extend_from_slice(&addend.to_le_bytes());
        v
    }

    fn entry(k: u32, sym: u32, addend: i64) -> orbistoun_elf::reloc::Elf64Rela {
        parse_table(&rela(0, sym, k, addend))[0]
    }

    /// Resolves nothing, so the unresolved path can be exercised.
    struct Nothing;
    impl SymbolResolver for Nothing {
        fn resolve(&self, _: u32) -> Option<u64> {
            None
        }
    }

    #[test]
    fn a_relative_relocation_adjusts_for_the_placement_base() {
        // The commonest kind by far - 172,790 of them in one real executable. Getting
        // the base arithmetic wrong here corrupts an image comprehensively.
        let got = value_for(
            &entry(kind::RELATIVE, 0, 0x2000),
            0x4000_0000,
            &Nothing,
            None,
        )
        .expect("relative needs no symbol");
        assert_eq!(got, 0x4000_2000);
    }

    #[test]
    fn a_negative_addend_subtracts_rather_than_wrapping_enormously() {
        // Addends are signed. Reading one as unsigned turns a small backwards offset
        // into an address near the top of the address space.
        let got = value_for(&entry(kind::RELATIVE, 0, -8), 0x4000_0000, &Nothing, None)
            .expect("relative");
        assert_eq!(got, 0x3FFF_FFF8);
    }

    #[test]
    fn a_jump_slot_takes_the_symbol_address_and_ignores_the_addend() {
        // This is the import case: the value written here is what the guest calls.
        let resolver = SingleTargetResolver {
            target: 0xDEAD_BEEF,
        };
        let got = value_for(&entry(kind::JUMP_SLOT, 7, 0x1234), 0x1000, &resolver, None)
            .expect("resolved");
        assert_eq!(
            got, 0xDEAD_BEEF,
            "a PLT slot is the address, not address+addend"
        );
    }

    #[test]
    fn an_absolute_relocation_adds_the_addend_to_the_symbol() {
        let resolver = SingleTargetResolver { target: 0x1_0000 };
        let got = value_for(&entry(kind::ABS64, 7, 0x40), 0, &resolver, None).expect("resolved");
        assert_eq!(got, 0x1_0040, "ABS64 does use the addend");
    }

    #[test]
    fn an_unresolvable_symbol_is_reported_rather_than_written_as_zero() {
        // Writing zero would leave a slot that looks like a valid null pointer, and the
        // guest would fault somewhere unrelated and much later.
        assert_eq!(
            value_for(&entry(kind::JUMP_SLOT, 3, 0), 0, &Nothing, None),
            Err(Outcome::Unresolved)
        );
    }

    #[test]
    fn tls_is_deferred_distinctly_from_unsupported() {
        // Two different problems: one waits for a feature, the other for a decision.
        // Collapsing them would hide which.
        for k in [kind::DTPMOD64, kind::DTPOFF64, kind::TPOFF64] {
            assert_eq!(
                value_for(
                    &entry(k, 0, 0),
                    0,
                    &SingleTargetResolver { target: 1 },
                    None
                ),
                Err(Outcome::TlsDeferred),
                "type {k}"
            );
        }
        assert_eq!(
            value_for(
                &entry(0xFF, 0, 0),
                0,
                &SingleTargetResolver { target: 1 },
                None
            ),
            Err(Outcome::Unsupported)
        );
    }

    #[test]
    fn a_closure_can_serve_as_a_resolver() {
        // Keeps the common case light: a test or a caller with a map needs no type.
        let resolver = |index: u32| if index == 5 { Some(0x999) } else { None };
        assert_eq!(
            value_for(&entry(kind::GLOB_DAT, 5, 0), 0, &resolver, None),
            Ok(0x999)
        );
        assert_eq!(
            value_for(&entry(kind::GLOB_DAT, 6, 0), 0, &resolver, None),
            Err(Outcome::Unresolved)
        );
    }
    #[test]
    fn a_thread_local_offset_is_measured_downwards_from_the_thread_pointer() {
        // Variant II: the block sits below the pointer, so the offset is negative.
        // Writing the raw module offset instead reads the control block, which holds
        // plausible pointers - so the guest misbehaves rather than faulting.
        let layout = TlsLayout::new(16, 64, 8);
        let got = value_for(&entry(kind::TPOFF64, 0, 8), 0, &Nothing, Some(&layout))
            .expect("a local thread-local needs no symbol");
        assert_eq!(got as i64, 8 - 64);
    }

    #[test]
    fn the_module_id_is_answered_for_the_only_module_loaded() {
        let layout = TlsLayout::new(0, 32, 8);
        assert_eq!(
            value_for(&entry(kind::DTPMOD64, 0, 0), 0, &Nothing, Some(&layout)),
            Ok(MAIN_MODULE_ID)
        );
    }

    #[test]
    fn an_offset_within_the_module_block_is_the_addend_unchanged() {
        // DTPOFF64 is module-relative, unlike TPOFF64. Adjusting it as well would
        // double-count the block size.
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(
            value_for(&entry(kind::DTPOFF64, 0, 24), 0, &Nothing, Some(&layout)),
            Ok(24)
        );
    }

    #[test]
    fn a_thread_local_naming_another_module_is_deferred_rather_than_guessed() {
        // It needs a descriptor table and a second loaded image, neither of which
        // exists. A plausible number here would be silently wrong.
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(
            value_for(&entry(kind::TPOFF64, 9, 0), 0, &Nothing, Some(&layout)),
            Err(Outcome::TlsDeferred)
        );
    }

    #[test]
    fn thread_local_relocations_without_a_layout_are_deferred_not_zeroed() {
        // A module declaring no PT_TLS but carrying TLS relocations is odd enough that
        // answering it with an offset into a block that does not exist would be a lie.
        assert_eq!(
            value_for(&entry(kind::TPOFF64, 0, 0), 0, &Nothing, None),
            Err(Outcome::TlsDeferred)
        );
    }
}
