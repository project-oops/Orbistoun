//! Per-import thunks: the machine code a guest lands on when it calls something
//! orbistoun has not implemented.
//!
//! This is the other half of D005. Relocation writes an address into a procedure
//! linkage table slot; **this is what lives at that address**. There is still no
//! hooking pass - the guest simply calls what the linker put there, and the linker is
//! us.
//!
//! # Why one stub per import rather than one shared stub
//!
//! A single shared target answers the question "did the guest call something we have
//! not written?" and nothing else. That is worth very little. One stub per import
//! answers **which** one, in order, with counts - which is the entire input to the
//! iterative loop this project is built around: run it, read what it wanted, implement
//! the frequent ones, run it again.
//!
//! The cost is thirty-two bytes per import. A commercial executable importing 1,410
//! functions spends 45 KiB, once.
//!
//! # The shape of a stub
//!
//! ```text
//!   mov r10, <index>       ; 49 BA imm64
//!   mov r11, <trampoline>  ; 49 BB imm64
//!   jmp r11                ; 41 FF E3
//! ```
//!
//! `r10` and `r11` are the two registers System V lets a function destroy that are
//! **not** argument registers. Using anything else would corrupt an argument before the
//! trampoline could save it - and the corruption would be invisible until some
//! implemented function eventually read the wrong value.
//!
//! An absolute `jmp` through a register rather than a relative one, because the table
//! and the trampoline are separate allocations and nothing guarantees they land within
//! two gigabytes of each other.
//!
//! # Write, then execute - never both
//!
//! The table is populated writable and flipped to read-execute before any guest can
//! reach it. Leaving it writable would put a page that guest code jumps to permanently
//! at the mercy of any stray write.

pub mod dispatch;
pub mod syscall;

pub use dispatch::{
    ArgumentDump, DUMP_BYTES, ForcedWrite, GuestFn, Plant, Pointing, RecordedCall, abi_conformance,
    argument_dumps, call_counts, entry_alignment_conforms, forced_return_count,
    forced_write_counts, implemented_count, implemented_count_within, install_call_budget,
    install_float_handlers, install_forced_dumps, install_forced_returns, install_forced_writes,
    install_handlers, install_policy_returns, install_policy_writes, install_readable_ranges,
    install_stub_returns, install_writable_ranges, is_implemented, is_mapped, last_call,
    note_readable_range, ranges_known, recorded_calls, stack_arguments, total_calls,
};

use orbistoun_mem::{AddressSpace, MemError, Protection};

/// Bytes each stub occupies.
///
/// Sixty-four, and the extra thirty-two over what the dispatch instructions need buys the
/// **landing zone** below. A power of two so an index multiplies cleanly into an offset, and
/// each stub still starts where a branch target wants to be.
pub const THUNK_SIZE: u64 = 64;

/// Where the landing zone begins, and where it ends.
///
/// # What lands here, and why a range rather than a point
///
/// An open-toolchain payload does not ask for a syscall gadget. It resolves an ordinary
/// function by name, **adds a small offset**, and calls that for every system call it makes -
/// the usual trick of reaching the `syscall` instruction inside a wrapper rather than the
/// wrapper's prologue. `elfldr` adds ten (D400).
///
/// Ten is that payload's number, not the platform's: the offset is wherever a given C library
/// build happens to put the instruction, so serving exactly ten would work for one payload and
/// silently mislead the next. So this is a **sled**. Every byte from
/// [`LANDING_START`] to [`LANDING_END`] is a one-byte `nop`, and execution entering at any of
/// them slides forward into a jump to the syscall gadget. Eight, ten, twelve - all arrive.
///
/// # What it replaced, which was worse than a crash
///
/// The old layout put `mov r11, trampoline` exactly ten bytes in, so a payload offsetting into
/// a stub jumped into the dispatcher **correctly**, having skipped only the instruction that
/// loads the index. The dispatcher switched on whatever was left in `r10` and called an
/// arbitrary function. Not corruption and not random: a well-formed call to the wrong thing,
/// which is the failure mode this project spends most of its time refusing (D400).
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
/// Pure, so the encoding is testable without mapping or executing anything - and the
/// bytes are asserted directly, because an instruction encoding that is wrong by one
/// bit produces a plausible different instruction rather than an error.
pub fn emit(index: u64, trampoline: u64, syscall_gadget: Option<u64>) -> [u8; THUNK_SIZE as usize] {
    let mut code = [PADDING; THUNK_SIZE as usize];

    // Over the landing zone, to the dispatch path. A `jmp rel8` counts from the end of itself,
    // which is why this is the distance from byte two rather than from zero.
    code[0] = JMP_REL8;
    code[1] = u8::try_from(DISPATCH_AT - LANDING_START).unwrap_or(0);

    // The sled, then the jump it slides into. `jmp` and not `call`: the guest reached this by
    // calling, so its return address is already on the stack and the gadget's own `ret` takes
    // it back where it came from.
    //
    // **Left as halts when there is no gadget to reach.** A sled sliding into padding would
    // still be an improvement on dispatching a stale index, but only by accident; leaving the
    // whole region as `int3` means a payload using this convention stops where it went wrong
    // instead of somewhere later.
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
/// The same one `orbistoun-abi` hands the runtime through a named global (D378) - it caches on
/// a `OnceLock`, so a guest reaching it by name and a guest reaching it by offsetting into a
/// stub arrive at the same code, with the same dispatcher and the same trace.
fn landing_gadget() -> Option<u64> {
    let dispatch: unsafe extern "sysv64" fn(*const u64) -> u64 =
        syscall::orbistoun_syscall_dispatch;
    orbistoun_abi::enter::syscall_gadget(dispatch as *const () as usize as u64, syscall::SAVED)
}

/// A block of stubs, one per dynamic symbol, mapped and ready to be jumped to.
///
/// Owns its mapping, so the stubs stay valid for exactly as long as the table does. A
/// guest holding a resolved pointer into a freed table is the kind of bug that presents
/// as random corruption.
#[derive(Debug)]
pub struct ThunkTable {
    space: AddressSpace,
    base: u64,
    /// Every stub, imports and run-time names together.
    count: usize,
    /// Just the guest's own imports, which is what a stub count has always meant.
    imports: usize,
}

impl ThunkTable {
    /// Builds `count` stubs at `base`, all routed to the shared trampoline.
    ///
    /// `base` must satisfy the host allocation granularity - [`SUGGESTED_BASE`] is an
    /// address that does.
    ///
    /// Every stub here belongs to one of the guest's own dynamic symbols. See
    /// [`Self::build_with_named`] for the form that adds stubs nothing imported.
    pub fn build(base: u64, count: usize, page: u64) -> Result<Self, MemError> {
        Self::build_with_named(base, count, 0, page)
    }

    /// Builds stubs for the guest's imports **and** for names it may ask for later.
    ///
    /// # Two populations in one table, deliberately
    ///
    /// The first `imports` stubs are the guest's own dynamic symbols, indexed exactly as
    /// its relocations index them. The `named` stubs after them belong to no import at
    /// all: they exist so a name looked up at run time has somewhere to resolve to, which
    /// is how the open-toolchain payloads get their C library (D365).
    ///
    /// One table rather than two, because the dispatch path is indexed by a single number
    /// and a second table would need a second trampoline, a second counter array and a
    /// second way to be wrong. [`Self::len`] keeps meaning *imports*, so every report that
    /// counts stubs still counts the guest's own.
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
            // SAFETY: the reservation above covers `base .. base + len`, and `at` is
            // the start of slot `index`, whose whole extent lies within it because
            // `len` was sized from `count`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    code.as_ptr(),
                    std::ptr::with_exposed_provenance_mut::<u8>(dest),
                    code.len(),
                );
            }
        }

        // Executable only once every stub is written. A table that is writable while a
        // guest can jump into it is a page under the control of any stray write.
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

    /// Address of the stub for `index`, or `None` if it is past the end.
    ///
    /// `None` rather than a wrapped address: resolving an out-of-range symbol to some
    /// other import's stub would produce a call trace that is confidently wrong.
    pub fn address_of(&self, index: usize) -> Option<u64> {
        (index < self.count).then(|| {
            self.base
                .saturating_add((index as u64).saturating_mul(THUNK_SIZE))
        })
    }

    /// Where the table starts.
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// How many of the guest's own imports it holds.
    ///
    /// **Not the number of stubs.** A table may carry more, for names resolved at run time
    /// rather than linked - and a report saying "1,410 import stubs" must go on meaning the
    /// guest's 1,410, not that number plus everything this emulator could answer.
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

/// Storage for imports that name **data** rather than code.
///
/// # Why a thunk is the wrong answer here
///
/// Relocation writes an address into a slot, and for a function that address is a stub -
/// the guest calls it and lands somewhere that reports itself. **For an object it is
/// wrong in a way that looks right.** A guest importing `__stderrp` loads the slot and
/// then dereferences what it found, so a stub address becomes x86 instruction bytes read
/// as a pointer, and the guest carries on. Nothing faults and nothing reports (D307).
///
/// Every commercial title in the local corpus has between five and twenty of these, and
/// they are not obscure: C++ vtables for `bad_alloc` and `runtime_error`, the iostream
/// locale objects, `_Stdout` and `_Stderr`, and `__stack_chk_guard` - which is read on
/// entry to every function built with the stack protector.
///
/// # Why zeroed, and one page each
///
/// Zeroed for the reason `process_argument_block` is: the real contents are not known from
/// any lawful source, so every field reads as zero rather than as something invented. A
/// guest reading a pointer out of one gets null and can check it; a virtual call through a
/// null vtable faults immediately and says so, which is worth far more than executing
/// whatever a stub happens to begin with.
///
/// **One page each rather than one shared page**, because a guest may write to these -
/// `_Stdout` is an object, not a constant - and two imports sharing storage would alias in
/// a way nothing downstream could see.
#[derive(Debug)]
pub struct DataBlocks {
    /// Keeps the reservation alive for as long as the guest can reach it.
    _space: AddressSpace,
    /// Symbol index to the address handed to the guest.
    slots: std::collections::BTreeMap<usize, u64>,
    /// The same addresses by name, for implementations that must write one.
    named: std::collections::BTreeMap<String, u64>,
}

/// Where the current run's data imports live, by name.
///
/// A process-wide slot rather than a parameter, for the same reason the policy tables are:
/// a guest call arrives on a `sysv64` frame with six registers and no room to thread a
/// context through. Installed once, before the guest is entered.
static DATA_SYMBOLS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
    std::sync::OnceLock::new();

/// Publishes the data-import addresses for this run.
///
/// A second call is ignored rather than refused: two guests in one process is not something
/// this supports, and the first one's addresses are the live ones.
pub fn install_data_symbols(named: std::collections::BTreeMap<String, u64>) {
    let _ = DATA_SYMBOLS.set(named);
}

/// The address of a named data import, or [`None`] if the guest does not import it.
///
/// [`None`] is a real answer: a guest that never imported `optarg` has no `optarg`, and an
/// implementation must not invent somewhere to put one.
#[must_use]
pub fn data_symbol(name: &str) -> Option<u64> {
    DATA_SYMBOLS.get()?.get(name).copied()
}

/// The environment strings this run gave the guest.
///
/// # Why they are published rather than passed
///
/// `getenv` is an implementation like any other: it arrives on a `sysv64` frame with six
/// registers and no room for a context, so what it needs has to be reachable from a
/// process-wide slot. The same reasoning as the data symbols above.
///
/// **Empty is the default and is meaningful.** Nothing here knows what the platform sets, so
/// a run gives a guest only what `config.toml` names - and a guest asking for anything else
/// is told it is unset, which is a real answer every caller already handles.
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

/// Stubs a guest can ask for **by name at run time**, rather than by importing them.
///
/// # Why an import table is not the whole story
///
/// A dynamic import is a name the linker resolved before the program started. It is not the
/// only way a program gets an address: the open-toolchain payloads resolve most of their C
/// library *themselves*, at startup, by asking the platform for one name at a time and
/// storing what comes back in their own `.bss` (D365). Nothing in an import table can
/// answer that, because the names are never in one.
///
/// So the same table of stubs is published a second way - by name - and a resolver hands
/// out the same address the linker would have written. One set of stubs, two ways to reach
/// them, so a function cannot behave differently depending on which route found it.
static NAME_THUNKS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
    std::sync::OnceLock::new();

/// Publishes the stubs that a run-time lookup may answer with.
///
/// A second call is ignored, exactly as for the data symbols, and for the same reason: two
/// guests in one process is not something this supports.
pub fn install_name_thunks(named: std::collections::BTreeMap<String, u64>) {
    let _ = NAME_THUNKS.set(named);
}

/// The stub for a name, or [`None`] when nothing here implements it.
///
/// [`None`] is the honest answer and the useful one: a resolver that invents an address for
/// a name nobody wrote hands the guest something to call, and what it calls is not the
/// function it asked for.
#[must_use]
pub fn name_thunk(name: &str) -> Option<u64> {
    NAME_THUNKS.get()?.get(name).copied()
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
        if imports.is_empty() {
            return Ok(Self {
                _space: space,
                slots,
                named,
            });
        }

        let len = (imports.len() as u64).saturating_mul(page);
        space.reserve(base, len, Protection::READ_WRITE)?;
        for (nth, (index, name)) in imports.iter().enumerate() {
            let at = base.saturating_add((nth as u64).saturating_mul(page));
            slots.insert(*index, at);
            named.insert(name.clone(), at);
        }
        Ok(Self {
            _space: space,
            slots,
            named,
        })
    }

    /// The storage for a named data import.
    ///
    /// **What lets an implementation own guest state.** `getopt` has to leave the current
    /// option argument where the guest will read it, and the guest reads its own `optarg` -
    /// a slot this layer reserved. Without a way back from the name an implementation can
    /// only answer a return value, and half the C library's contract lives in its globals.
    #[must_use]
    pub fn address_of_name(&self, name: &str) -> Option<u64> {
        self.named.get(name).copied()
    }

    /// Every named import and where its storage is, for publishing to implementations.
    #[must_use]
    pub fn named(&self) -> std::collections::BTreeMap<String, u64> {
        self.named.clone()
    }

    /// The address for a symbol index, or [`None`] if it does not name data.
    ///
    /// [`None`] is what makes this composable: a resolver asks here first and falls
    /// through to the thunk table, so a function is unaffected by this existing at all.
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

    /// A base no other test will use, in this binary or any other.
    ///
    /// **Not the shipped one, and not one picked by hand.** These run in parallel in a
    /// single process and alongside other crates' binaries, so an address used twice fails
    /// either way. The first version of these tests took the shipped base four times and
    /// whichever arrived second got `Conflict`; the second picked offsets by hand, which is
    /// the documented way to reintroduce it (D323).
    /// Imports as `build` takes them, named after their index.
    ///
    /// The names matter to `address_of_name` and not to the reservation, so a test about
    /// placement makes them up rather than pretending to care.
    fn named(indices: &[usize]) -> Vec<(usize, String)> {
        indices.iter().map(|i| (*i, format!("sym{i}"))).collect()
    }

    fn base() -> u64 {
        use orbistoun_mem::test_bases::{Range, crates};
        static RANGE: Range = Range::nth(crates::THUNK);
        RANGE.take()
    }

    /// **A stub count still means the guest's imports** (D366).
    ///
    /// The table carries more than that now - one extra per name a guest may resolve at
    /// run time - and a report saying "34 import stubs" must go on meaning the guest's 34.
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

    /// A lookup answers nothing for a name nobody published, rather than an address.
    ///
    /// The registry is process-wide and set once, so this asserts the shape of a miss
    /// rather than installing one - an install here would be the run's only install and
    /// would make every other test in the process see it.
    #[test]
    fn a_name_nobody_published_resolves_to_nothing() {
        assert_eq!(super::name_thunk("sceKernelNoSuchFunctionAtAll"), None);
    }

    /// **One page each, and no two imports share one.**
    ///
    /// A guest writes to some of these - `_Stdout` is an object, not a constant - so two
    /// imports resolving to the same storage would alias, and nothing downstream could
    /// see it happen (D307).
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

    /// **An index that names code gets nothing here**, which is what lets a resolver
    /// compose: it misses, falls through to the thunk table, and a function is unaffected
    /// by any of this existing.
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

    /// **A named import can be found by name**, which is what lets `getopt` write `optarg`.
    ///
    /// Without it an implementation can only answer a return value, and half the C
    /// library's contract lives in its globals (D344).
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

    /// The storage is readable, writable, and **zero** - which is the whole claim.
    ///
    /// Zero is what a guest can check. Instruction bytes are what it cannot, and that is
    /// the difference this exists to make.
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
        // SAFETY: as above. Bound to a name rather than written inline, because `cargo
        // fmt` collapses the macro call and leaves the comment attached to nothing.
        let written = unsafe { std::ptr::read(cell) };
        assert_eq!(written, 0xABCD, "a guest may write here");
    }
}

/// An address for the data blocks, clear of both images and the thunk table.
pub const SUGGESTED_DATA_BASE: u64 = 0x0000_7200_0000_0000;

/// An address for a thunk table that is clear of where images are placed.
///
/// Far enough from the module base that a stray offset lands in unmapped space and
/// faults, rather than quietly hitting the other allocation.
pub const SUGGESTED_BASE: u64 = 0x0000_7000_0000_0000;

#[cfg(test)]
mod tests {
    use super::{
        DISPATCH_AT, JMP_R11, JMP_REL8, LANDING_END, LANDING_START, MOV_R10_IMM64, MOV_R11_IMM64,
        NOP, PADDING, THUNK_CODE_LEN, THUNK_SIZE, emit,
    };

    #[test]
    fn a_stub_encodes_the_index_and_the_trampoline_literally() {
        // Asserted byte for byte: an encoding wrong by one bit is a plausible different
        // instruction, not an error, and would be found only by executing it.
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

    /// **Entering at the front reaches dispatch and nothing else.**
    ///
    /// The short jump counts from its own end, so an off-by-two here lands two bytes into
    /// `mov r10, imm64` - which is a valid instruction reading part of the index as an opcode,
    /// exactly the class of wrongness the landing zone was built to stop.
    #[test]
    fn the_first_instruction_jumps_over_the_landing_zone_to_dispatch() {
        let code = emit(7, 0x1000, Some(0x2000));
        assert_eq!(code[0], JMP_REL8);
        let lands_at = LANDING_START + code[1] as usize;
        assert_eq!(lands_at, DISPATCH_AT, "the hop must clear the landing zone");
        assert_eq!(&code[lands_at..lands_at + 2], &MOV_R10_IMM64);
    }

    /// **Every byte of the landing zone is an entry point, which is the whole idea.**
    ///
    /// A payload resolves a name and adds an offset this project does not get to choose - ten
    /// for one payload, something else for the next. A sled means each of those offsets slides
    /// into the same jump instead of only the one somebody happened to test (D400).
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

    /// The offset that started this: ten bytes in has to reach the gadget.
    #[test]
    fn ten_bytes_in_is_inside_the_landing_zone() {
        // `elfldr` adds exactly this. It is not special-cased anywhere and must not be - the
        // assertion is that the sled covers it, not that it is the only one covered.
        assert!(
            (LANDING_START..LANDING_END).contains(&10),
            "the offset a real payload uses falls outside the sled"
        );
    }

    /// **With no gadget to reach, the zone halts rather than sliding into padding.**
    ///
    /// The negative case, written because a guard nobody has watched reject something is a
    /// guard nobody knows anything about. A sled with nothing at the end of it would run on
    /// into the dispatch path and call an arbitrary function - which is precisely the bug
    /// being fixed, reintroduced by the fallback.
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

    #[test]
    fn the_carrier_registers_are_the_two_that_are_not_arguments() {
        // r10 and r11 are the only registers System V lets a callee destroy that do not
        // carry an argument. Any other choice corrupts an argument before the
        // trampoline can save it, invisibly.
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

    #[test]
    fn the_unused_tail_halts_rather_than_running_on() {
        // Execution should never reach it, but if it does, stopping is far better than
        // interpreting whatever the allocator left behind as instructions.
        let code = emit(0, 0, None);
        assert!(
            code[DISPATCH_AT + THUNK_CODE_LEN..]
                .iter()
                .all(|b| *b == PADDING)
        );
    }

    #[test]
    fn a_zero_index_is_encoded_rather_than_omitted() {
        // Import zero is a real import. Skipping the move would leave whatever the
        // previous call put in r10 and attribute the call to the wrong function.
        let code = emit(0, 0x1000, None);
        assert_eq!(&code[DISPATCH_AT..DISPATCH_AT + 2], &MOV_R10_IMM64);
        assert_eq!(&code[DISPATCH_AT + 2..DISPATCH_AT + 10], &[0; 8]);
    }
}
