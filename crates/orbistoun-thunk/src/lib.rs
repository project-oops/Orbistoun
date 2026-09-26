//! Per-import thunks: the machine code a guest lands on when it calls something orbistoun has
//! not implemented.
//!
//! Relocation writes a stub's address into a procedure linkage table slot, so the guest calls
//! what the linker put there. One stub per import says which import was called, in order
//! and with counts, at 64 bytes each (D062). The dispatch path is `mov r10, <index>`;
//! `mov r11, <trampoline>`; `jmp r11`: `r10` and `r11` are the scratch registers that carry no
//! argument, and the absolute jump reaches a trampoline in a separate allocation. The table is
//! written, then flipped to read-execute before any guest can reach it.

pub mod dispatch;
pub mod syscall;

pub use dispatch::{
    ArgumentDump, CallerStack, DUMP_BYTES, ForcedWrite, GuestFn, Plant, Pointing, RecordedCall,
    SHAPE_OTHER, SHAPE_POINTER, SHAPE_SCALAR, SHAPE_ZERO, abi_conformance, arg_shapes,
    argument_dumps, call_counts, caller_stacks, classify_arg, current_call, current_thread,
    describe_shape, dropped_ranges, dumps_dropped, entry_alignment_conforms, forced_return_count,
    forced_write_counts, host_thread, implemented_count, implemented_count_within,
    install_call_budget, install_float_handlers, install_forced_dumps, install_forced_returns,
    install_forced_writes, install_handlers, install_policy_returns, install_policy_writes,
    install_readable_ranges, install_stub_returns, install_writable_ranges, is_implemented,
    is_mapped, last_call, note_readable_range, opening_calls, opening_sequence, ranges_known,
    readable_span, recorded_calls, stack_arguments, total_calls,
};

use orbistoun_mem::{AddressSpace, MemError, Protection};

/// Bytes each stub occupies: the dispatch instructions plus the landing zone below, a power of
/// two so an index multiplies into an offset.
pub const THUNK_SIZE: u64 = 64;

/// Where the landing zone begins.
///
/// Open-toolchain payloads resolve an ordinary function by name and call a small offset into it
/// to reach its `syscall` instruction (`elfldr` adds ten). The offset depends on the C library
/// build, so every byte from [`LANDING_START`] to [`LANDING_END`] is a one-byte `nop` that slides
/// into a jump to the syscall gadget, and any such offset arrives there rather than skipping the
/// index load into dispatch (D400).
pub const LANDING_START: usize = 2;
/// One past the last byte of the sled. See [`LANDING_START`].
pub const LANDING_END: usize = 16;

/// Where the dispatch path begins, jumped to over the landing zone.
pub const DISPATCH_AT: usize = 32;

/// The dispatch instructions are this long, from [`DISPATCH_AT`].
pub const THUNK_CODE_LEN: usize = 23;

/// `mov r10, imm64` - REX.W + REX.B, then `B8 + r10 & 7`.
const MOV_R10_IMM64: [u8; 2] = [0x49, 0xBA];
/// `mov r11, imm64`.
const MOV_R11_IMM64: [u8; 2] = [0x49, 0xBB];
/// `jmp r11` - REX.B, `FF /4`.
const JMP_R11: [u8; 3] = [0x41, 0xFF, 0xE3];
/// Padding that halts rather than running on, should execution ever reach it.
const PADDING: u8 = 0xCC;
/// `nop`, one byte, so a sled of them can be entered at any offset.
const NOP: u8 = 0x90;
/// `jmp rel8` - the short hop over the landing zone to the dispatch path.
const JMP_REL8: u8 = 0xEB;

/// Encodes one stub.
///
/// Pure, so the bytes are asserted directly without mapping anything: an encoding wrong by one
/// bit is a plausible different instruction, not an error.
pub fn emit(index: u64, trampoline: u64, syscall_gadget: Option<u64>) -> [u8; THUNK_SIZE as usize] {
    let mut code = [PADDING; THUNK_SIZE as usize];

    // Over the landing zone to the dispatch path. `jmp rel8` counts from its own end, hence the
    // distance from byte two.
    code[0] = JMP_REL8;
    code[1] = u8::try_from(DISPATCH_AT - LANDING_START).unwrap_or(0);

    // The sled, then the jump it slides into. `jmp`, not `call`: the guest's return address is on
    // the stack, so the gadget's `ret` goes back to it. With no gadget the region stays `int3`, so
    // a payload using this convention stops where it went wrong.
    if let Some(gadget) = syscall_gadget {
        for byte in &mut code[LANDING_START..LANDING_END] {
            *byte = NOP;
        }
        code[LANDING_END..LANDING_END + 2].copy_from_slice(&MOV_R11_IMM64);
        code[LANDING_END + 2..LANDING_END + 10].copy_from_slice(&gadget.to_le_bytes());
        code[LANDING_END + 10..LANDING_END + 13].copy_from_slice(&JMP_R11);
    }

    code[DISPATCH_AT..DISPATCH_AT + 2].copy_from_slice(&MOV_R10_IMM64);
    code[DISPATCH_AT + 2..DISPATCH_AT + 10].copy_from_slice(&index.to_le_bytes());
    code[DISPATCH_AT + 10..DISPATCH_AT + 12].copy_from_slice(&MOV_R11_IMM64);
    code[DISPATCH_AT + 12..DISPATCH_AT + 20].copy_from_slice(&trampoline.to_le_bytes());
    code[DISPATCH_AT + 20..DISPATCH_AT + 23].copy_from_slice(&JMP_R11);
    code
}

/// The syscall gadget every stub's landing zone jumps to, built once.
///
/// The same `OnceLock`-cached gadget `orbistoun-abi` publishes through a named global, so a guest
/// reaching it by name or through a stub offset gets the same dispatcher and trace (D378).
fn landing_gadget() -> Option<u64> {
    let dispatch: unsafe extern "sysv64" fn(*const u64) -> u64 =
        syscall::orbistoun_syscall_dispatch;
    orbistoun_abi::enter::syscall_gadget(dispatch as *const () as usize as u64, syscall::SAVED)
}

/// A block of stubs, one per dynamic symbol, mapped and ready to be jumped to.
///
/// Owns its mapping, so the stubs live exactly as long as the table.
#[derive(Debug)]
pub struct ThunkTable {
    space: AddressSpace,
    base: u64,
    /// Every stub, imports and run-time names together.
    count: usize,
    /// Just the guest's own imports, which is what a stub count means.
    imports: usize,
}

impl ThunkTable {
    /// Builds `count` stubs at `base`, all routed to the shared trampoline.
    ///
    /// `base` must satisfy the host allocation granularity; [`SUGGESTED_BASE`] does. Every stub
    /// belongs to one of the guest's own dynamic symbols; [`Self::build_with_named`] adds stubs
    /// nothing imported.
    pub fn build(base: u64, count: usize, page: u64) -> Result<Self, MemError> {
        Self::build_with_named(base, count, 0, page)
    }

    /// Builds stubs for the guest's imports and for names it may ask for later.
    ///
    /// The first `imports` stubs are the guest's dynamic symbols, indexed as its relocations index
    /// them; the `named` stubs after them give a run-time name lookup somewhere to resolve to (D365).
    /// One table, because dispatch is indexed by a single number. [`Self::len`] still counts only
    /// imports.
    ///
    /// # Errors
    ///
    /// When the host refuses the reservation.
    pub fn build_with_named(
        base: u64,
        imports: usize,
        named: usize,
        page: u64,
    ) -> Result<Self, MemError> {
        let count = imports.saturating_add(named);
        let bytes = (count as u64).saturating_mul(THUNK_SIZE).max(page);
        let len = bytes.div_ceil(page).saturating_mul(page);

        let mut space = AddressSpace::new();
        space.reserve(base, len, Protection::READ_WRITE)?;

        let trampoline = dispatch::trampoline_address();
        let gadget = landing_gadget();
        for index in 0..count {
            let code = emit(index as u64, trampoline, gadget);
            let at = base.saturating_add((index as u64).saturating_mul(THUNK_SIZE));
            let dest = usize::try_from(at)
                .map_err(|_| MemError::HostRefused("thunk address does not fit".to_owned()))?;
            // SAFETY: the reservation above covers `base .. base + len`, and `at` is the start of slot
            // `index`, whose whole extent lies within it because `len` was sized from `count`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    code.as_ptr(),
                    std::ptr::with_exposed_provenance_mut::<u8>(dest),
                    code.len(),
                );
            }
        }

        // Executable only once every stub is written, and never writable while a guest can jump in.
        space.protect(base, len, Protection::READ_EXECUTE)?;
        dispatch::prepare_counters(count);
        dispatch::prepare_dumps(count);

        Ok(Self {
            space,
            base,
            count,
            imports,
        })
    }

    /// Address of the stub for `index`, or `None` if it is past the end, never another import's
    /// stub.
    pub fn address_of(&self, index: usize) -> Option<u64> {
        (index < self.count).then(|| {
            self.base
                .saturating_add((index as u64).saturating_mul(THUNK_SIZE))
        })
    }

    /// The index whose stub starts at `address`, or `None` for any other address.
    pub fn index_of(&self, address: u64) -> Option<usize> {
        let offset = address.checked_sub(self.base)?;
        let index = usize::try_from(offset / THUNK_SIZE).ok()?;
        (offset % THUNK_SIZE == 0 && index < self.count).then_some(index)
    }

    /// Where the table starts.
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// How many of the guest's own imports it holds.
    ///
    /// Not the number of stubs: a table may also carry stubs for run-time names.
    pub const fn len(&self) -> usize {
        self.imports
    }

    /// How many stubs it holds altogether, imports and named alike.
    pub const fn total(&self) -> usize {
        self.count
    }

    /// Whether it holds no imports.
    pub const fn is_empty(&self) -> bool {
        self.imports == 0
    }

    /// The address space backing the table.
    pub const fn space(&self) -> &AddressSpace {
        &self.space
    }
}

/// Storage for imports that name data rather than code.
///
/// A guest loads a data import's slot and dereferences it, so a stub address would be read as
/// a pointer with no fault (D307). Each gets one zeroed page: the contents are unknown, and a
/// null reads and faults visibly; one page each because guests write some of these (`_Stdout`)
/// and shared storage would alias. They include C++ vtables, iostream objects and
/// `__stack_chk_guard`.
#[derive(Debug)]
pub struct DataBlocks {
    /// Keeps the reservation alive for as long as the guest can reach it.
    _space: AddressSpace,
    /// Symbol index to the address handed to the guest.
    slots: std::collections::BTreeMap<usize, u64>,
    /// The same addresses by name, for implementations that must write one.
    named: std::collections::BTreeMap<String, u64>,
    /// What lives at each address, for a report naming what a register points at.
    ///
    /// Keyed by address because several modules import the same name (`__stack_chk_guard`) and each
    /// gets its own page; the by-name map answers an implementation's question instead.
    labels: std::collections::BTreeMap<u64, String>,
}

/// Where the current run's data imports live, by name.
///
/// A process-wide slot: a guest call arrives on a `sysv64` frame with no room to thread a
/// context through. Installed once, before the guest is entered.
static DATA_SYMBOLS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
    std::sync::OnceLock::new();

/// Publishes the data-import addresses for this run. A second call is ignored.
pub fn install_data_symbols(named: std::collections::BTreeMap<String, u64>) {
    let _ = DATA_SYMBOLS.set(named);
}

/// Where the current run's data imports live, by address, so a name several modules import
/// labels every page it was given.
static DATA_LABELS: std::sync::OnceLock<std::collections::BTreeMap<u64, String>> =
    std::sync::OnceLock::new();

/// Publishes what lives at each data-import address for this run.
pub fn install_data_labels(labels: std::collections::BTreeMap<u64, String>) {
    let _ = DATA_LABELS.set(labels);
}

/// The address of a named data import, or [`None`] if the guest does not import it; an
/// implementation never invents storage for one.
#[must_use]
pub fn data_symbol(name: &str) -> Option<u64> {
    DATA_SYMBOLS.get()?.get(name).copied()
}

/// The environment strings this run gave the guest.
///
/// Published in a process-wide slot for `getenv`, like the data symbols. Empty by default: a run
/// gives a guest only what `config.toml` names, and anything else reads as unset.
static ENVIRONMENT: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Publishes the environment for this run, in `NAME=value` form.
pub fn install_environment(strings: Vec<String>) {
    let _ = ENVIRONMENT.set(strings);
}

/// The environment strings this run gave the guest.
#[must_use]
pub fn guest_environment() -> Vec<String> {
    ENVIRONMENT.get().cloned().unwrap_or_default()
}

/// Stubs a guest can ask for by name at run time, rather than by importing them.
///
/// Open-toolchain payloads resolve most of their C library themselves at startup, one name at a
/// time (D365). The same stubs are published by name, so a resolver hands out the address the
/// linker would have written and a function behaves the same by either route.
static NAME_THUNKS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
    std::sync::OnceLock::new();

/// Publishes the stubs that a run-time lookup may answer with. A second call is ignored.
pub fn install_name_thunks(named: std::collections::BTreeMap<String, u64>) {
    let _ = NAME_THUNKS.set(named);
}

/// The stub for a name, or [`None`] when nothing here implements it; an invented address would
/// give the guest something that is not the function it asked for.
#[must_use]
pub fn name_thunk(name: &str) -> Option<u64> {
    NAME_THUNKS.get()?.get(name).copied()
}

/// Every name this run declares in the libkernel family, published for `sceKernelDlsym`.
static LIBKERNEL_NAMES: std::sync::OnceLock<std::collections::BTreeSet<String>> =
    std::sync::OnceLock::new();

/// Publishes which names belong to libkernel, so a resolver can narrow by module handle.
///
/// The platform's libkernel spans this project's `libkernel`, `libkernel_fs` and
/// `libkernel_sync_on_address` declarations in different crates, so the service, which sees
/// them all, publishes the list (D536).
pub fn install_libkernel_names(names: std::collections::BTreeSet<String>) {
    let _ = LIBKERNEL_NAMES.set(names);
}

/// Whether `name` is one libkernel declares, or [`None`] when nothing has published a list.
///
/// `None` and `Some(true)` both mean "do not refuse": a list never installed is silence, not a
/// denial.
#[must_use]
pub fn name_is_libkernel(name: &str) -> Option<bool> {
    LIBKERNEL_NAMES.get().map(|names| names.contains(name))
}

impl DataBlocks {
    /// Reserves one zeroed page for each import in `imports`, keyed by index and by name.
    ///
    /// # Errors
    ///
    /// When the host refuses the reservation.
    pub fn build(base: u64, imports: &[(usize, String)], page: u64) -> Result<Self, MemError> {
        let mut space = AddressSpace::new();
        let mut slots = std::collections::BTreeMap::new();
        let mut named = std::collections::BTreeMap::new();
        let mut labels = std::collections::BTreeMap::new();
        if imports.is_empty() {
            return Ok(Self {
                _space: space,
                slots,
                named,
                labels,
            });
        }

        let len = (imports.len() as u64).saturating_mul(page);
        space.reserve(base, len, Protection::READ_WRITE)?;
        // Published as readable, so a fault report can name a register pointing into these pages.
        note_readable_range(base, len);
        for (nth, (index, name)) in imports.iter().enumerate() {
            let at = base.saturating_add((nth as u64).saturating_mul(page));
            slots.insert(*index, at);
            named.insert(name.clone(), at);
            labels.insert(at, name.clone());
        }
        Ok(Self {
            _space: space,
            slots,
            named,
            labels,
        })
    }

    /// The storage for a named data import.
    ///
    /// Lets an implementation own guest state: `getopt` leaves the option argument in the guest's
    /// own `optarg`, a slot reserved here.
    #[must_use]
    pub fn address_of_name(&self, name: &str) -> Option<u64> {
        self.named.get(name).copied()
    }

    /// Every named import and where its storage is, for publishing to implementations.
    #[must_use]
    pub fn named(&self) -> std::collections::BTreeMap<String, u64> {
        self.named.clone()
    }

    /// What lives at each address, for publishing to the fault reporter.
    #[must_use]
    pub fn labels(&self) -> std::collections::BTreeMap<u64, String> {
        self.labels.clone()
    }

    /// The address for a symbol index, or [`None`] if it does not name data.
    ///
    /// A resolver asks here first and falls through to the thunk table on `None`.
    #[must_use]
    pub fn address_of(&self, index: usize) -> Option<u64> {
        self.slots.get(&index).copied()
    }

    /// How many imports were given storage.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether nothing needed storage, which is the ordinary case for a homebrew guest.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod data_tests {
    use super::{DataBlocks, THUNK_SIZE, ThunkTable};

    /// Imports as `build` takes them, named after their index.
    fn named(indices: &[usize]) -> Vec<(usize, String)> {
        indices.iter().map(|i| (*i, format!("sym{i}"))).collect()
    }

    fn base() -> u64 {
        use orbistoun_mem::test_bases::{Range, crates};
        static RANGE: Range = Range::nth(crates::THUNK);
        RANGE.take()
    }

    /// A stub count still means the guest's imports, counted apart from named stubs (D366).
    #[test]
    fn a_table_counts_its_imports_and_its_named_stubs_separately() {
        let table = ThunkTable::build_with_named(base(), 4, 6, 0x1000).expect("reserves");
        assert_eq!(
            table.len(),
            4,
            "imports, which is what a count has always meant"
        );
        assert_eq!(
            table.total(),
            10,
            "and every stub, for the code that lays them out"
        );
        assert!(table.address_of(9).is_some(), "a named slot has a stub");
        assert!(
            table.address_of(10).is_none(),
            "and one past the end has none"
        );
    }

    /// The named half starts exactly where the imports end, with no gap and no overlap.
    #[test]
    fn the_named_stubs_start_immediately_after_the_imports() {
        let table = ThunkTable::build_with_named(base(), 3, 2, 0x1000).expect("reserves");
        let last_import = table.address_of(2).expect("the last import");
        let first_named = table.address_of(3).expect("the first named stub");
        assert_eq!(first_named, last_import + THUNK_SIZE);
    }

    /// A stub's start names its index; any other address names none.
    #[test]
    fn a_stub_address_names_its_index() {
        let table = ThunkTable::build_with_named(base(), 3, 2, 0x1000).expect("reserves");
        let fourth = table.address_of(4).expect("the last stub");
        assert_eq!(table.index_of(fourth), Some(4));
        assert_eq!(table.index_of(fourth + 1), None);
        assert_eq!(table.index_of(fourth + THUNK_SIZE), None);
        assert_eq!(table.index_of(table.base() - THUNK_SIZE), None);
    }

    /// A lookup answers nothing for a name nobody published.
    ///
    /// The registry is process-wide and set once, so this asserts the shape of a miss rather than
    /// installing one.
    #[test]
    fn a_name_nobody_published_resolves_to_nothing() {
        assert_eq!(super::name_thunk("sceKernelNoSuchFunctionAtAll"), None);
    }

    /// Each data import gets its own distinct page (D307).
    #[test]
    fn each_data_import_gets_its_own_distinct_storage() {
        let blocks = DataBlocks::build(base(), &named(&[3, 9, 40]), 0x1000).expect("reserves");

        let addresses: Vec<u64> = [3, 9, 40]
            .iter()
            .map(|i| blocks.address_of(*i).expect("has storage"))
            .collect();

        assert_eq!(addresses.len(), 3);
        assert_ne!(addresses[0], addresses[1]);
        assert_ne!(addresses[1], addresses[2]);
        assert_ne!(addresses[0], addresses[2]);
        assert_eq!(blocks.len(), 3);
    }

    /// An index that names code gets no storage, so a resolver falls through to the thunk table.
    #[test]
    fn an_import_that_names_code_has_no_storage() {
        let blocks = DataBlocks::build(base(), &named(&[3]), 0x1000).expect("reserves");

        assert!(blocks.address_of(3).is_some());
        assert_eq!(blocks.address_of(4), None, "4 was never named as data");
        assert_eq!(blocks.address_of(0), None);
    }

    /// A guest needing no storage reserves none, rather than a page it never reads.
    #[test]
    fn a_guest_with_no_data_imports_reserves_nothing() {
        let blocks = DataBlocks::build(base(), &[], 0x1000).expect("reserves");
        assert!(blocks.is_empty());
        assert_eq!(blocks.address_of(0), None);
    }

    /// Storage can be found by name as well as by index.
    #[test]
    fn storage_can_be_found_by_name_as_well_as_by_index() {
        let blocks =
            DataBlocks::build(base(), &[(7, "optarg".to_owned())], 0x1000).expect("reserves");

        assert_eq!(blocks.address_of_name("optarg"), blocks.address_of(7));
        assert_eq!(
            blocks.address_of_name("optind"),
            None,
            "a name the guest never imported has no storage, and none is invented"
        );
    }

    /// A name two modules both import labels both of its pages.
    ///
    /// Asserted on the later page, the one a by-name map loses.
    #[test]
    fn a_repeated_name_labels_every_page_it_was_given() {
        let blocks = DataBlocks::build(
            base(),
            &[
                (3, "__stack_chk_guard".to_owned()),
                (11, "__stack_chk_guard".to_owned()),
            ],
            0x1000,
        )
        .expect("reserves");
        let labels = blocks.labels();
        let second = blocks.address_of(11).expect("has storage");

        assert_ne!(
            blocks.address_of(3),
            blocks.address_of(11),
            "two imports of one name still get separate storage"
        );
        assert_eq!(
            labels.get(&second).map(String::as_str),
            Some("__stack_chk_guard"),
            "the second page is labelled too, which the by-name map cannot do"
        );
        assert_eq!(labels.len(), 2, "one label per page, not one per name");
        assert_eq!(
            blocks.named().len(),
            1,
            "and the by-name map really is lossy here - which is why it is not the one asked"
        );
    }

    /// The storage is readable, writable, and zero.
    #[test]
    fn the_storage_reads_as_zero_and_accepts_a_write() {
        let blocks = DataBlocks::build(base(), &named(&[1]), 0x1000).expect("reserves");
        let at = blocks.address_of(1).expect("has storage");

        let cell = std::ptr::with_exposed_provenance_mut::<u64>(usize::try_from(at).expect("fits"));
        // SAFETY: `at` is the start of a page this call reserved read-write.
        let first = unsafe { std::ptr::read(cell) };
        assert_eq!(first, 0, "a guest reading a pointer out of this gets null");

        // SAFETY: same page, still reserved and writable for the life of `blocks`.
        unsafe { std::ptr::write(cell, 0xABCD) };
        // SAFETY: as above. Bound to a name rather than written inline, because `cargo fmt` collapses
        // the macro call and leaves the comment attached to nothing.
        let written = unsafe { std::ptr::read(cell) };
        assert_eq!(written, 0xABCD, "a guest may write here");
    }
}

/// An address for the data blocks, clear of both images and the thunk table.
pub const SUGGESTED_DATA_BASE: u64 = 0x0000_7200_0000_0000;

/// An address for a thunk table that is clear of where images are placed, so a stray offset
/// faults in unmapped space.
pub const SUGGESTED_BASE: u64 = 0x0000_7000_0000_0000;

#[cfg(test)]
mod tests {
    use super::{
        DISPATCH_AT, JMP_R11, JMP_REL8, LANDING_END, LANDING_START, MOV_R10_IMM64, MOV_R11_IMM64,
        NOP, PADDING, THUNK_CODE_LEN, THUNK_SIZE, emit,
    };

    /// A stub encodes the index and the trampoline literally.
    #[test]
    fn a_stub_encodes_the_index_and_the_trampoline_literally() {
        // Asserted byte for byte: an encoding wrong by one bit is a plausible different instruction.
        let code = emit(0x1234, 0xDEAD_BEEF_0000_1000, None);
        let at = DISPATCH_AT;
        assert_eq!(&code[at..at + 2], &MOV_R10_IMM64);
        assert_eq!(&code[at + 2..at + 10], &0x1234_u64.to_le_bytes());
        assert_eq!(&code[at + 10..at + 12], &MOV_R11_IMM64);
        assert_eq!(
            &code[at + 12..at + 20],
            &0xDEAD_BEEF_0000_1000_u64.to_le_bytes()
        );
        assert_eq!(&code[at + 20..at + 23], &JMP_R11);
    }

    /// The first instruction jumps over the landing zone to dispatch.
    ///
    /// The short jump counts from its own end; off by two, it would land inside `mov r10, imm64`.
    #[test]
    fn the_first_instruction_jumps_over_the_landing_zone_to_dispatch() {
        let code = emit(7, 0x1000, Some(0x2000));
        assert_eq!(code[0], JMP_REL8);
        let lands_at = LANDING_START + code[1] as usize;
        assert_eq!(lands_at, DISPATCH_AT, "the hop must clear the landing zone");
        assert_eq!(&code[lands_at..lands_at + 2], &MOV_R10_IMM64);
    }

    /// Any offset into the landing zone slides to the gadget (D400).
    #[test]
    fn any_offset_into_the_landing_zone_slides_to_the_gadget() {
        let code = emit(7, 0x1000, Some(0xCAFE_0000_1000));
        for (at, byte) in code
            .iter()
            .enumerate()
            .take(LANDING_END)
            .skip(LANDING_START)
        {
            assert_eq!(
                *byte, NOP,
                "offset {at} is not an entry point, so a payload landing there is lost"
            );
        }
        assert_eq!(&code[LANDING_END..LANDING_END + 2], &MOV_R11_IMM64);
        assert_eq!(
            &code[LANDING_END + 2..LANDING_END + 10],
            &0xCAFE_0000_1000_u64.to_le_bytes()
        );
        assert_eq!(&code[LANDING_END + 10..LANDING_END + 13], &JMP_R11);
    }

    /// Ten bytes in, the offset `elfldr` adds, is inside the landing zone.
    #[test]
    fn ten_bytes_in_is_inside_the_landing_zone() {
        // Not special-cased: the sled covers it.
        assert!(
            (LANDING_START..LANDING_END).contains(&10),
            "the offset a real payload uses falls outside the sled"
        );
    }

    /// Without a gadget, the landing zone halts rather than sliding into dispatch with a stale
    /// index.
    #[test]
    fn without_a_gadget_the_landing_zone_stops_rather_than_running_on() {
        let code = emit(7, 0x1000, None);
        for (at, byte) in code
            .iter()
            .enumerate()
            .take(LANDING_END)
            .skip(LANDING_START)
        {
            assert_eq!(*byte, PADDING, "offset {at} runs on instead of halting");
        }
    }

    /// The carrier registers are the two scratch registers that carry no argument.
    #[test]
    fn the_carrier_registers_are_the_two_that_are_not_arguments() {
        // `r10` and `r11` are the only registers a System V callee may destroy that carry no argument.
        assert_eq!(
            MOV_R10_IMM64[0] & 0x01,
            0x01,
            "REX.B selects the extended register"
        );
        assert_eq!(
            MOV_R10_IMM64[1],
            0xB8 + 2,
            "r10 is register 2 in the extended bank"
        );
        assert_eq!(MOV_R11_IMM64[1], 0xB8 + 3, "r11 is register 3");
    }

    /// Every slot is the same size, so an index multiplies into an offset.
    #[test]
    fn every_slot_is_the_same_size_so_an_index_multiplies_into_an_offset() {
        assert_eq!(THUNK_SIZE, 64);
        assert!(THUNK_SIZE >= (DISPATCH_AT + THUNK_CODE_LEN) as u64);
        assert_eq!(
            THUNK_SIZE % 16,
            0,
            "stubs should start on a branch-target boundary"
        );
    }

    /// The unused tail halts rather than running on.
    #[test]
    fn the_unused_tail_halts_rather_than_running_on() {
        // Execution never reaches it, and if it does, stopping beats running leftover bytes.
        let code = emit(0, 0, None);
        assert!(
            code[DISPATCH_AT + THUNK_CODE_LEN..]
                .iter()
                .all(|b| *b == PADDING)
        );
    }

    /// A zero index is encoded rather than omitted.
    #[test]
    fn a_zero_index_is_encoded_rather_than_omitted() {
        // Import zero is a real import; skipping the move would attribute the call to whatever `r10`
        // held.
        let code = emit(0, 0x1000, None);
        assert_eq!(&code[DISPATCH_AT..DISPATCH_AT + 2], &MOV_R10_IMM64);
        assert_eq!(&code[DISPATCH_AT + 2..DISPATCH_AT + 10], &[0; 8]);
    }
}

/// Stubs reachable by bare name for imports nothing implements, when a run asks for them.
///
/// Empty unless a run installed it, so a run that did not ask is unchanged and the difference is
/// the measurement.
static DECLARED_THUNKS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
    std::sync::OnceLock::new();

/// Publishes by-name stubs for declared imports, so `sceKernelDlsym` can answer for them.
///
/// By import, a declared name lands on a stub answering the placeholder; by `sceKernelDlsym`, it
/// is refused, since the by-name table holds only implementations. The hardware resolves both.
/// Handing out a stub has a cost (a placeholder instead of a guest's null fallback), so this is
/// installed only under a diagnostic and its run is measured.
pub fn install_declared_thunks(named: std::collections::BTreeMap<String, u64>) {
    let _ = DECLARED_THUNKS.set(named);
}

/// The stub for a declared-but-unimplemented name, when a run published them.
#[must_use]
pub fn declared_thunk(name: &str) -> Option<u64> {
    DECLARED_THUNKS.get()?.get(name).copied()
}

/// The data import whose page holds `address`, and how far into it.
///
/// A zeroed vtable page is a table of null function pointers, so a virtual call through one
/// faults at a small offset; naming the page connects the fault to its cause. Exact page only,
/// never nearest-preceding, since the pages are contiguous; an address in no block is [`None`].
#[must_use]
pub fn data_symbol_at(address: u64) -> Option<(&'static str, u64)> {
    let page = address & !(orbistoun_core::GUEST_PAGE_SIZE - 1);
    // The address-keyed table, which labels every page of a name several modules import.
    DATA_LABELS
        .get()?
        .get(&page)
        .map(|name| (name.as_str(), address - page))
}
