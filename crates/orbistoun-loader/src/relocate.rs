//! Applying relocations to a placed image.
//!
//! Writing a host address into a PLT slot is the interception (D005): the guest calls
//! whatever the slot contains, and the slot contains what this module put there.
//! `RELATIVE` writes an internal pointer adjusted by the placement base, `ABS64` and
//! `GLOB_DAT` a symbol's address, and `JUMP_SLOT` a function address. A relocation that
//! cannot be applied is counted and reported, never skipped silently: an unrelocated
//! pointer looks valid and fails somewhere unrelated.

use orbistoun_elf::reloc::{Elf64Rela, RelocationTally, kind, parse_table};
use orbistoun_elf::{Container, dynamic::DynamicInfo};

use crate::plan::SlotWrite;
use crate::tls::{self, TlsLayout};
use crate::{Image, LoadError};

/// The outcome of resolving a dynamic symbol index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Resolved to an address.
    Address(u64),
    /// The symbol is weak and unanswered, binding to zero under the ELF gABI.
    WeakZero,
    /// The symbol could not be resolved.
    Unresolved,
}

/// Resolves a dynamic symbol index to the address the guest should see.
///
/// `None` means the symbol is unresolved; the relocation is then counted rather than
/// applied, so the tally says how much of the image is not ready.
pub trait SymbolResolver {
    /// The address for a symbol index, or `None` if it cannot be resolved.
    fn resolve(&self, symbol_index: u32) -> Option<u64>;

    /// Resolves a symbol index to a [`Resolution`].
    ///
    /// The default implementation delegates to [`Self::resolve`].
    fn resolve_symbol(&self, symbol_index: u32) -> Resolution {
        match self.resolve(symbol_index) {
            Some(addr) => Resolution::Address(addr),
            None => Resolution::Unresolved,
        }
    }
}

impl<F: Fn(u32) -> Option<u64>> SymbolResolver for F {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        self(symbol_index)
    }
}

/// A table of per-import stubs resolves a symbol to that symbol's own stub.
///
/// A call trace then names which import the guest wanted, where a single shared address
/// says only that something unimplemented was called.
impl SymbolResolver for orbistoun_thunk::ThunkTable {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        self.address_of(symbol_index as usize)
    }
}

/// Data first, then thunks.
///
/// A function import misses in the data blocks and falls through to its own stub; an import
/// that names an object gets storage instead, so the guest dereferences a null it can check
/// rather than instruction bytes (D307).
#[derive(Debug, Clone, Copy)]
pub struct ImportResolver<'a> {
    /// Stubs, for imports that name code.
    pub thunks: &'a orbistoun_thunk::ThunkTable,
    /// Storage, for imports that name data.
    pub data: &'a orbistoun_thunk::DataBlocks,
    /// Imports to leave unresolved, by index.
    ///
    /// Every import otherwise resolves to a stub, so a guest cannot tell an implemented
    /// function from one the platform never exported, and a presence probe counts both as
    /// present. A refused import resolves to nothing, as on the hardware (D392). [`None`]
    /// refuses nothing and is the default.
    pub refuse: Option<&'a std::collections::BTreeSet<usize>>,
    /// Weak imports that are unanswered and should bind to zero, by index (D676).
    pub weak_zero: Option<&'a std::collections::BTreeSet<usize>>,
}

impl SymbolResolver for ImportResolver<'_> {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        match self.resolve_symbol(symbol_index) {
            Resolution::Address(addr) => Some(addr),
            Resolution::WeakZero | Resolution::Unresolved => None,
        }
    }

    fn resolve_symbol(&self, symbol_index: u32) -> Resolution {
        let index = symbol_index as usize;
        if self.refuse.is_some_and(|refuse| refuse.contains(&index)) {
            // Unresolved, which the relocation tally counts and reports.
            return Resolution::Unresolved;
        }
        if self.weak_zero.is_some_and(|wz| wz.contains(&index)) {
            return Resolution::WeakZero;
        }
        match self
            .data
            .address_of(index)
            .or_else(|| self.thunks.address_of(index))
        {
            Some(addr) => Resolution::Address(addr),
            None => Resolution::Unresolved,
        }
    }
}

/// Binds an import to a real address inside a module the title ships, before any stub.
///
/// A stub is right for a platform library, where orbistoun either implements the function
/// or says it does not. A module the title ships has the code placed and relocated, so a
/// stub would answer a placeholder for a function that is present (D483). `bound` holds
/// only the imports that take this path; whether orbistoun's own implementation wins is a
/// policy the caller decides.
#[derive(Debug, Clone, Copy)]
pub struct TitleResolver<'a, R> {
    /// Dynamic symbol index to an address inside a placed, relocated module.
    pub bound: &'a std::collections::BTreeMap<u32, u64>,
    /// What answers everything else.
    pub inner: &'a R,
}

impl<R: SymbolResolver> SymbolResolver for TitleResolver<'_, R> {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        self.bound
            .get(&symbol_index)
            .copied()
            .or_else(|| self.inner.resolve(symbol_index))
    }

    fn resolve_symbol(&self, symbol_index: u32) -> Resolution {
        if let Some(&addr) = self.bound.get(&symbol_index) {
            Resolution::Address(addr)
        } else {
            self.inner.resolve_symbol(symbol_index)
        }
    }
}

/// Shifts a module's symbol indices into the shared table's index space.
///
/// One stub table serves every module, and module M's symbol i lives at slot `offset(M) + i`
/// (D484). A relocation inside M names i, so this adds the offset and the resolvers
/// underneath see one table. The main executable is module 0 with offset 0, so wrapping it
/// is a no-op.
#[derive(Debug, Clone, Copy)]
pub struct OffsetResolver<'a, R> {
    /// Where this module's symbol zero sits in the shared table.
    pub offset: usize,
    /// The resolver holding the shared table.
    pub inner: &'a R,
}

impl<R: SymbolResolver> SymbolResolver for OffsetResolver<'_, R> {
    fn resolve(&self, symbol_index: u32) -> Option<u64> {
        // Refuses rather than wraps: a shifted index that does not fit means the table and
        // the module disagree about the symbol count, and a wrapped slot would bind the
        // relocation to an unrelated implementation.
        let shifted = u32::try_from(self.offset).ok()?.checked_add(symbol_index)?;
        self.inner.resolve(shifted)
    }

    fn resolve_symbol(&self, symbol_index: u32) -> Resolution {
        let Some(shifted) = u32::try_from(self.offset)
            .ok()
            .and_then(|off| off.checked_add(symbol_index))
        else {
            return Resolution::Unresolved;
        };
        self.inner.resolve_symbol(shifted)
    }
}

/// A resolver that answers every symbol with one address.
///
/// Every import points at one host function that reports being called, so the image is
/// complete and an unimplemented call says so rather than jumping to a zeroed slot.
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

/// The computed value to write for a relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocValue {
    /// Resolved to an address.
    Address(u64),
    /// Bound to zero because the symbol is weak and unanswered under the ELF gABI (D676).
    WeakZero(u64),
}

impl RelocValue {
    /// The numeric value to write into the relocation slot.
    #[must_use]
    pub const fn value(self) -> u64 {
        match self {
            Self::Address(addr) | Self::WeakZero(addr) => addr,
        }
    }
}

/// Computes the value one relocation should write, or why it cannot.
///
/// Separate from the writing so the arithmetic is testable without mapping anything
/// (D016). `tls` is the module's own thread-local layout, when it declares one; `None`
/// reports every thread-local relocation as deferred.
pub fn value_for(
    entry: &Elf64Rela,
    base: u64,
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
) -> Result<RelocValue, Outcome> {
    let addend = entry.addend.get();
    match entry.kind() {
        kind::RELATIVE => Ok(RelocValue::Address(base.wrapping_add(addend as u64))),
        kind::ABS64 => match resolver.resolve_symbol(entry.symbol_index()) {
            Resolution::Address(s) => Ok(RelocValue::Address(s.wrapping_add(addend as u64))),
            Resolution::WeakZero => Ok(RelocValue::WeakZero(addend as u64)),
            Resolution::Unresolved => Err(Outcome::Unresolved),
        },
        kind::GLOB_DAT | kind::JUMP_SLOT => match resolver.resolve_symbol(entry.symbol_index()) {
            Resolution::Address(s) => Ok(RelocValue::Address(s)),
            Resolution::WeakZero => Ok(RelocValue::WeakZero(0)),
            Resolution::Unresolved => Err(Outcome::Unresolved),
        },
        _ if entry.is_tls() => tls_value_for(entry, addend, tls).map(RelocValue::Address),
        _ => Err(Outcome::Unsupported),
    }
}

/// The thread-local cases, split out to keep the main match readable.
///
/// Only the module's own block is handled. A relocation naming another module needs a
/// descriptor table and a second loaded image, so it is reported rather than answered.
fn tls_value_for(entry: &Elf64Rela, addend: i64, tls: Option<&TlsLayout>) -> Result<u64, Outcome> {
    let Some(layout) = tls else {
        return Err(Outcome::TlsDeferred);
    };
    if entry.symbol_index() != 0 {
        return Err(Outcome::TlsDeferred);
    }
    match entry.kind() {
        // Which module the variable belongs to: the main module.
        kind::DTPMOD64 => Ok(tls::MAIN_MODULE_ID),
        // An offset within that module's block, so the addend needs no adjustment.
        kind::DTPOFF64 => Ok(addend as u64),
        // Measured from the thread pointer, so negative: the block sits below it.
        kind::TPOFF64 => Ok(layout.tp_offset(addend as u64) as u64),
        _ => Err(Outcome::Unsupported),
    }
}

/// Why a relocation was not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Needs another module's thread-local storage, or a layout the module does not declare.
    TlsDeferred,
    /// A relocation type this loader does not implement.
    Unsupported,
    /// The symbol could not be resolved.
    Unresolved,
}

/// Applies every relocation in a container to its placed image.
///
/// Returns a tally of how many entries were left unapplied and why, separating "this image
/// is not ready" from "this image loaded and then behaved strangely".
pub fn apply(
    image: &Image,
    whole: &[u8],
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
) -> Result<RelocationTally, LoadError> {
    apply_recorded(image, whole, resolver, tls).map(|applied| applied.tally)
}

/// What relocating an image did: the tally, and every value written, for the link plan (D724).
#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// How many entries were applied, and why the rest were not.
    pub tally: RelocationTally,
    /// Every write, in the order it was made.
    pub writes: Vec<SlotWrite>,
}

/// [`apply`], also returning every write it made.
pub fn apply_recorded(
    image: &Image,
    whole: &[u8],
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
) -> Result<Applied, LoadError> {
    let container = Container::parse(whole)?;
    let Some(dyn_bytes) = container.dynamic_bytes(whole)? else {
        // No dynamic table: a static binary has no relocations.
        return Ok(Applied::default());
    };
    let info = DynamicInfo::parse(dyn_bytes);

    let mut tally = RelocationTally::default();
    let mut writes = Vec::new();
    for (addr, size) in [(info.rela, info.relasz), (info.jmprel, info.pltrelsz)] {
        if addr == 0 || size == 0 {
            continue;
        }
        // Through the container rather than by virtual address: under the vendor dynamic
        // tags these are offsets into the data segment (D247).
        let Some(at) = container.table_offset(whole, &info, addr)? else {
            continue;
        };
        let len = usize::try_from(size).unwrap_or(0);
        let Some(table_bytes) = whole.get(at..at.saturating_add(len)) else {
            continue;
        };
        apply_table(
            &parse_table(table_bytes),
            image,
            resolver,
            tls,
            &mut tally,
            &mut writes,
        )?;
    }
    Ok(Applied { tally, writes })
}

/// Applies one parsed table, accumulating into `tally` and `writes`.
fn apply_table(
    table: &[Elf64Rela],
    image: &Image,
    resolver: &impl SymbolResolver,
    tls: Option<&TlsLayout>,
    tally: &mut RelocationTally,
    writes: &mut Vec<SlotWrite>,
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
        // Every write lands inside the span this image owns; a relocation outside it is
        // a corrupt table.
        if target < span_base || target.saturating_add(8) > span_base.saturating_add(span_len) {
            return Err(LoadError::RelocationOutOfBounds {
                target,
                span_base,
                span_len,
            });
        }

        let ptr = usize::try_from(target).map_err(|_| LoadError::AddressTooLarge(target))?;
        // SAFETY: `target` lies wholly inside the image's span, which the image holds a
        // live reservation for and exclusively owns; the write is unaligned.
        unsafe {
            std::ptr::with_exposed_provenance_mut::<u64>(ptr).write_unaligned(value.value());
        }
        writes.push(SlotWrite {
            at: target,
            value: value.value(),
        });
        match value {
            RelocValue::Address(_) => tally.applied += 1,
            RelocValue::WeakZero(_) => tally.weak_zero += 1,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OffsetResolver;

    /// A resolver that answers the slot it was asked for, so a shift is visible as a value.
    struct Echo;
    impl SymbolResolver for Echo {
        fn resolve(&self, symbol_index: u32) -> Option<u64> {
            Some(u64::from(symbol_index))
        }
    }

    /// A bound import answers the module's address, not the stub underneath.
    #[test]
    fn a_bound_import_beats_the_stub() {
        let bound = std::collections::BTreeMap::from([(7_u32, 0x5000_1234_u64)]);
        let resolver = super::TitleResolver {
            bound: &bound,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(7), Some(0x5000_1234));
    }

    /// An unbound import falls through to the stub table.
    #[test]
    fn an_unbound_import_falls_through() {
        let bound = std::collections::BTreeMap::from([(7_u32, 0x5000_1234_u64)]);
        let resolver = super::TitleResolver {
            bound: &bound,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(8), Some(8), "the stub table still answers");
    }

    /// An empty binding changes nothing.
    #[test]
    fn binding_nothing_is_the_identity() {
        let bound = std::collections::BTreeMap::new();
        let resolver = super::TitleResolver {
            bound: &bound,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(3), Some(3));
    }

    /// The main executable is module 0, and wrapping it is exactly the identity.
    #[test]
    fn module_zero_is_unshifted() {
        let resolver = OffsetResolver {
            offset: 0,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(0), Some(0));
        assert_eq!(resolver.resolve(583), Some(583));
    }

    /// A later module's symbol lands in that module's own range.
    #[test]
    fn a_later_module_is_shifted_by_its_offset() {
        let resolver = OffsetResolver {
            offset: 583,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(0), Some(583));
        assert_eq!(resolver.resolve(4), Some(587));
    }

    /// A shift that does not fit refuses rather than wraps into another module's slot.
    #[test]
    fn a_shift_that_overflows_answers_nothing() {
        let resolver = OffsetResolver {
            offset: usize::try_from(u32::MAX).expect("fits on a 64-bit host"),
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(1), None, "an overflow was wrapped");
    }

    /// An offset too large to be a slot index at all is refused, not truncated.
    #[test]
    fn an_offset_beyond_the_index_space_answers_nothing() {
        let resolver = OffsetResolver {
            offset: usize::MAX,
            inner: &Echo,
        };
        assert_eq!(resolver.resolve(0), None);
    }

    /// Addresses these tests reserve, taken rather than chosen.
    ///
    /// One static for the module: each `Range` instance has its own cursor, so two
    /// function-local statics would hand out the same addresses to concurrent tests.
    static RANGE: orbistoun_mem::test_bases::Range =
        orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::LOADER);

    /// A data import resolves to storage and a function import still resolves to its stub
    /// (D307).
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
            weak_zero: None,
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

    /// A refused import resolves to nothing, as for a symbol no library exports (D392).
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
            weak_zero: None,
        };
        assert_eq!(refusing.resolve(3), None, "the refused one");
        assert!(refusing.resolve(4).is_some(), "and only that one");

        let permissive = ImportResolver {
            thunks: &thunks,
            data: &data,
            refuse: None,
            weak_zero: None,
        };
        assert!(
            permissive.resolve(3).is_some(),
            "refusing nothing is the default, and every recorded measurement is under it"
        );
    }

    use super::{Outcome, RelocValue, SingleTargetResolver, SymbolResolver, value_for};
    use crate::tls::{MAIN_MODULE_ID, TlsLayout};
    use orbistoun_elf::reloc::{kind, parse_table};

    /// Builds a relocation entry, generated (D051).
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

    /// `RELATIVE` adds the placement base.
    #[test]
    fn a_relative_relocation_adjusts_for_the_placement_base() {
        // The commonest kind; wrong base arithmetic corrupts the whole image.
        let got = value_for(
            &entry(kind::RELATIVE, 0, 0x2000),
            0x4000_0000,
            &Nothing,
            None,
        )
        .expect("relative needs no symbol");
        assert_eq!(got, RelocValue::Address(0x4000_2000));
    }

    /// A negative addend subtracts rather than wrapping.
    #[test]
    fn a_negative_addend_subtracts_rather_than_wrapping_enormously() {
        // Addends are signed.
        let got = value_for(&entry(kind::RELATIVE, 0, -8), 0x4000_0000, &Nothing, None)
            .expect("relative");
        assert_eq!(got, RelocValue::Address(0x3FFF_FFF8));
    }

    /// `JUMP_SLOT` writes the symbol address and ignores the addend.
    #[test]
    fn a_jump_slot_takes_the_symbol_address_and_ignores_the_addend() {
        // The import case: the value written here is what the guest calls.
        let resolver = SingleTargetResolver {
            target: 0xDEAD_BEEF,
        };
        let got = value_for(&entry(kind::JUMP_SLOT, 7, 0x1234), 0x1000, &resolver, None)
            .expect("resolved");
        assert_eq!(
            got,
            RelocValue::Address(0xDEAD_BEEF),
            "a PLT slot is the address, not address+addend"
        );
    }

    /// `ABS64` adds the addend to the symbol address.
    #[test]
    fn an_absolute_relocation_adds_the_addend_to_the_symbol() {
        let resolver = SingleTargetResolver { target: 0x1_0000 };
        let got = value_for(&entry(kind::ABS64, 7, 0x40), 0, &resolver, None).expect("resolved");
        assert_eq!(
            got,
            RelocValue::Address(0x1_0040),
            "ABS64 does use the addend"
        );
    }

    /// An unresolvable symbol is reported, not written as zero.
    #[test]
    fn an_unresolvable_symbol_is_reported_rather_than_written_as_zero() {
        // Zero would look like a valid null pointer.
        assert_eq!(
            value_for(&entry(kind::JUMP_SLOT, 3, 0), 0, &Nothing, None),
            Err(Outcome::Unresolved)
        );
    }

    /// A weak unanswered symbol binds to zero and is reported as such (D676).
    #[test]
    fn a_weak_unanswered_symbol_binds_to_zero_and_is_distinguished() {
        struct WeakUnanswered;
        impl SymbolResolver for WeakUnanswered {
            fn resolve(&self, _: u32) -> Option<u64> {
                None
            }
            fn resolve_symbol(&self, _: u32) -> super::Resolution {
                super::Resolution::WeakZero
            }
        }

        let got_glob = value_for(&entry(kind::GLOB_DAT, 3, 0), 0, &WeakUnanswered, None)
            .expect("weak unanswered binds to zero");
        assert_eq!(got_glob, RelocValue::WeakZero(0));

        let got_jump = value_for(&entry(kind::JUMP_SLOT, 3, 0), 0, &WeakUnanswered, None)
            .expect("weak unanswered binds to zero");
        assert_eq!(got_jump, RelocValue::WeakZero(0));

        let got_abs = value_for(&entry(kind::ABS64, 3, 0x20), 0, &WeakUnanswered, None)
            .expect("weak unanswered binds to zero with addend");
        assert_eq!(got_abs, RelocValue::WeakZero(0x20));
    }

    /// The import resolver distinguishes weak-zero from refused.
    #[test]
    fn an_import_resolver_distinguishes_weak_zero_from_refused() {
        use super::{ImportResolver, Resolution, SymbolResolver};

        let thunks = orbistoun_thunk::ThunkTable::build(RANGE.take(), 8, 0x1000).expect("stubs");
        let data = orbistoun_thunk::DataBlocks::build(RANGE.take(), &[], 0x1000).expect("storage");
        let refuse: std::collections::BTreeSet<usize> = [3].into_iter().collect();
        let weak_zero: std::collections::BTreeSet<usize> = [5].into_iter().collect();

        let resolver = ImportResolver {
            thunks: &thunks,
            data: &data,
            refuse: Some(&refuse),
            weak_zero: Some(&weak_zero),
        };
        assert_eq!(resolver.resolve_symbol(3), Resolution::Unresolved);
        assert_eq!(resolver.resolve(3), None);
        assert_eq!(resolver.resolve_symbol(5), Resolution::WeakZero);
        assert_eq!(resolver.resolve(5), None);
        assert!(matches!(resolver.resolve_symbol(1), Resolution::Address(_)));
        assert!(resolver.resolve(1).is_some());
    }

    /// Thread-local relocations report as deferred, not unsupported.
    #[test]
    fn tls_is_deferred_distinctly_from_unsupported() {
        // Two different problems; collapsing them would hide which.
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

    /// A closure serves as a resolver.
    #[test]
    fn a_closure_can_serve_as_a_resolver() {
        let resolver = |index: u32| if index == 5 { Some(0x999) } else { None };
        assert_eq!(
            value_for(&entry(kind::GLOB_DAT, 5, 0), 0, &resolver, None),
            Ok(RelocValue::Address(0x999))
        );
        assert_eq!(
            value_for(&entry(kind::GLOB_DAT, 6, 0), 0, &resolver, None),
            Err(Outcome::Unresolved)
        );
    }

    /// `TPOFF64` is measured downwards from the thread pointer.
    #[test]
    fn a_thread_local_offset_is_measured_downwards_from_the_thread_pointer() {
        // Variant II: the block sits below the pointer. The raw module offset would read
        // the control block's plausible pointers instead of faulting.
        let layout = TlsLayout::new(16, 64, 8);
        let got = value_for(&entry(kind::TPOFF64, 0, 8), 0, &Nothing, Some(&layout))
            .expect("a local thread-local needs no symbol");
        assert_eq!(got.value() as i64, 8 - 64);
    }

    /// `DTPMOD64` answers the main module's id.
    #[test]
    fn the_module_id_is_answered_for_the_only_module_loaded() {
        let layout = TlsLayout::new(0, 32, 8);
        assert_eq!(
            value_for(&entry(kind::DTPMOD64, 0, 0), 0, &Nothing, Some(&layout)),
            Ok(RelocValue::Address(MAIN_MODULE_ID))
        );
    }

    /// `DTPOFF64` is the addend unchanged.
    #[test]
    fn an_offset_within_the_module_block_is_the_addend_unchanged() {
        // Module-relative, unlike TPOFF64; adjusting it would double-count the block size.
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(
            value_for(&entry(kind::DTPOFF64, 0, 24), 0, &Nothing, Some(&layout)),
            Ok(RelocValue::Address(24))
        );
    }

    /// A thread-local relocation naming another module is deferred.
    #[test]
    fn a_thread_local_naming_another_module_is_deferred_rather_than_guessed() {
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(
            value_for(&entry(kind::TPOFF64, 9, 0), 0, &Nothing, Some(&layout)),
            Err(Outcome::TlsDeferred)
        );
    }

    /// Thread-local relocations without a layout are deferred, not zeroed.
    #[test]
    fn thread_local_relocations_without_a_layout_are_deferred_not_zeroed() {
        assert_eq!(
            value_for(&entry(kind::TPOFF64, 0, 0), 0, &Nothing, None),
            Err(Outcome::TlsDeferred)
        );
    }
}
