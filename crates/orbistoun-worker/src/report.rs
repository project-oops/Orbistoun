//! Saying *where* a guest faulted, from inside the fault.
//!
//! An access violation with no address is one bit of information. The same fault with
//! "read of `0x0` while executing at image+0x1a4c20" is a work list - it says whether
//! the guest dereferenced a null thread pointer, ran off the end of a stub, or jumped
//! somewhere that was never mapped.
//!
//! # Why this writes to the error stream and not the protocol stream
//!
//! The protocol is newline-delimited JSON, and a process dying mid-write would leave a
//! half-finished line that breaks the reader for good. A fault report is diagnostic
//! text, so it goes to the error stream where a truncated line costs nothing. The
//! parent still produces a structured verdict of its own (D064); this adds the detail
//! that only the faulting process can know.
//!
//! # The formatting is allocation-free
//!
//! A handler runs on a thread that has just faulted, on the guest stack. Building the
//! message with the ordinary formatting machinery would allocate, and allocating there
//! risks deadlocking against the code that crashed. So the message is assembled in a
//! fixed buffer and issued as a single write - the same rule principle 9 already
//! imposes on trace recording.

use core::sync::atomic::{AtomicU64, Ordering};

// The shapes this fills in live one layer down, in `orbistoun-report`, so the service
// layer and both shims can see them. Only the producing side is here (D160).
use orbistoun_report::trace::{
    AbiReport, ArgumentDump, CallTrace, CalledImport, Conditions, FaultSite, FormatReport, Quiet,
    ReadReport, SubmissionSummary, TAIL_CALLS, TracedCall,
};
// `Frame` (stack-walk records) and `Registers` are consumed only by the Windows fault
// reporter (`walk_frames`, `emit`, `describe_pointees`), all `#[cfg(windows)]`.
#[cfg(windows)]
use orbistoun_report::trace::{Frame, Registers};

/// How many regions can be named in a fault report.
const MAX_REGIONS: usize = 5;

/// Bases of the named regions, or zero for an unused slot.
static REGION_BASE: [AtomicU64; MAX_REGIONS] = [const { AtomicU64::new(0) }; MAX_REGIONS];
/// Lengths, parallel to [`REGION_BASE`].
static REGION_LEN: [AtomicU64; MAX_REGIONS] = [const { AtomicU64::new(0) }; MAX_REGIONS];

/// Names, indexed the same way. Fixed rather than stored, so the handler never chases a
/// pointer that the faulting code may have invalidated.
const REGION_NAMES: [&str; MAX_REGIONS] = [
    "image",
    "stubs",
    "stack",
    "guest mappings",
    "the title's own modules",
];

/// Which slot each kind of region occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The placed guest image.
    Image,
    /// The per-import stub table.
    Stubs,
    /// The guest stack.
    Stack,
    /// Every module the title ships, as one span covering all of them.
    ///
    /// **One span rather than one per module**, because there are only so many slots and a
    /// title ships a handful. The span includes the unmapped guard between modules, so an
    /// address in a guard is named as a module - which overstates by a granule and is still
    /// far better than the alternative, which was naming it as orbistoun's own code (D489).
    TitleModules,
    /// The arena guest-requested mappings are placed in, as far as it has been used.
    ///
    /// **Registered when the guest stops rather than before it starts**, unlike every other
    /// region here: this one does not exist at entry and grows as the guest maps. That is
    /// enough for an argument dump, which is collected after the guest has stopped, and it is
    /// *not* enough for the fault handler - a run killed from outside names a faulting address
    /// in the arena as a bare number. Said here rather than discovered, because a region that
    /// is sometimes registered is exactly the kind of thing a reader would assume was always
    /// (D579).
    Mappings,
}

impl Region {
    /// Slot index for this region.
    const fn slot(self) -> usize {
        match self {
            Self::Image => 0,
            Self::Stubs => 1,
            Self::Stack => 2,
            Self::Mappings => 3,
            Self::TitleModules => 4,
        }
    }
}

/// Registers a region so a fault inside it can be named rather than left as a number.
///
/// Call before entering the guest. Registering afterwards would be too late for the
/// only fault that matters.
pub fn describe_region(region: Region, base: u64, len: u64) {
    REGION_BASE[region.slot()].store(base, Ordering::Relaxed);
    REGION_LEN[region.slot()].store(len, Ordering::Relaxed);
}

/// Names the region containing `address`, and the offset into it.
///
/// Pure and therefore testable, which matters more than usual here: the handler that
/// uses it cannot be stepped through, and a bug in it appears as a garbled message at
/// the exact moment the message is most needed.
pub fn locate(address: u64) -> Option<(&'static str, u64)> {
    (0..MAX_REGIONS).find_map(|i| {
        let base = REGION_BASE[i].load(Ordering::Relaxed);
        let len = REGION_LEN[i].load(Ordering::Relaxed);
        (len > 0 && address >= base && address < base.saturating_add(len))
            .then(|| (REGION_NAMES[i], address - base))
    })
}

/// Imports the loader bound into a module the title ships, by stub index.
static BOUND_TO_MODULES: std::sync::OnceLock<std::collections::BTreeSet<usize>> =
    std::sync::OnceLock::new();

/// Records which imports were bound into a title's own module.
///
/// **So the report can catch its own contradiction.** An import bound to a placed module is
/// called *in that module*; orbistoun never sees it, and no call is counted against its stub. So
/// a bound import with calls on its stub is impossible - and it is exactly what PPSA25872 shows,
/// nineteen million times, while the binding account says the import was bound (D640).
///
/// One of those two statements is wrong and until now nothing compared them.
pub fn name_bound_imports(indices: std::collections::BTreeSet<usize>) {
    let _ = BOUND_TO_MODULES.set(indices);
}

/// Bound imports that were nevertheless called through a stub, with their call counts.
///
/// Empty is the correct answer and the usual one. A non-empty list is a contradiction between
/// two things this run believes, and it names them rather than leaving a reader to notice.
pub(crate) fn bound_but_called() -> Vec<(String, u64)> {
    let Some(bound) = BOUND_TO_MODULES.get() else {
        return Vec::new();
    };
    let counts = orbistoun_thunk::call_counts();
    bound
        .iter()
        .filter_map(|index| {
            let calls = counts.get(*index).copied().unwrap_or(0);
            (calls > 0).then(|| (label_of(*index).unwrap_or("unknown").to_owned(), calls))
        })
        .collect()
}

/// Imports a run named with `ORBISTOUN_DUMP`, by label.
static FORCED_DUMPS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Records which imports a run asked about, so a captured-arguments finding answers only those.
pub fn name_forced_dumps(labels: Vec<String>) {
    let _ = FORCED_DUMPS.set(labels);
}

/// The libraries a title's own placed modules provide, by the name their imports carry.
static TITLE_MODULES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Records which library names belong to modules the title itself ships.
///
/// **So the report can stop giving advice that cannot work.** A hash from one of these is the
/// game's own symbol and no vendor vocabulary will ever hold it; telling a reader to extend one
/// is an afternoon spent on something with no answer (D631).
pub fn name_title_modules(libraries: Vec<String>) {
    let _ = TITLE_MODULES.set(libraries);
}

/// Functions the guest module names in its own symbol table, sorted by address.
///
/// `(offset into the image, extent, name)`. Empty for every stripped module, which is every
/// commercial title - and not empty for the open-toolchain guests, which are the ones this
/// project runs most and reads least well at a fault (D628).
static GUEST_CODE: std::sync::OnceLock<Vec<(u64, u64, String)>> = std::sync::OnceLock::new();

/// Records what a guest module calls its own functions, for naming an address in it.
///
/// Called once, before entry. Sorting here rather than at lookup is what keeps the fault path
/// allocation-free: the handler does a binary search over a slice and formats a `&str` it does
/// not own (principle 9).
pub fn name_guest_functions(mut functions: Vec<(u64, u64, String)>) {
    functions.sort_unstable_by_key(|(at, _, _)| *at);
    let _ = GUEST_CODE.set(functions);
}

/// The guest function an image offset falls in, and how far into it.
///
/// **Only when the symbol says it covers the offset.** Where the producer recorded an extent,
/// an offset past the end of the nearest preceding function is *not* in that function - it is
/// in a gap, or in something the table does not name, and saying otherwise would put a
/// confident wrong name on a fault. Where the extent is zero the nearest preceding name is
/// offered anyway, with the distance beside it, which is the same hint-not-fact bargain
/// `own_code_site` makes one region over.
fn guest_code_site(offset: u64) -> Option<(&'static str, u64)> {
    let functions = GUEST_CODE.get()?;
    let index = functions
        .partition_point(|(at, _, _)| *at <= offset)
        .checked_sub(1)?;
    let (at, size, name) = functions.get(index)?;
    let into = offset - at;
    (*size == 0 || into < *size).then_some((name.as_str(), into))
}

/// The whole span this run published, from the lowest region base to the highest end.
///
/// [`None`] when nothing has been described yet.
///
/// # What it is for
///
/// An address-shaped value that no region covers has two readings that look identical and are
/// not: a pointer into something this run never declared, and **a register the call never set**.
/// A call whose real arity is three leaves three registers holding whatever the caller last put
/// there, and the dump prints six because six is what it captures.
///
/// The distinction is checkable at the envelope. `sceAgcDriverAddEqEvent` was dumped with
/// `arg3 = 0x7ff7620bcab0`, which is above every region a guest was given - so it is not a guest
/// pointer this run failed to declare, it is a host address, and two runs of one build then
/// confirmed it by disagreeing about it (D615).
///
/// **Outside the envelope is not proof of a stale register.** A guest can compute a wild pointer,
/// and `orbistoun-abi` still hands out a host address in two places. It is a fact about where the
/// value sits, which is what the report says, and the reading is left to the reader.
#[must_use]
pub fn published_envelope() -> Option<(u64, u64)> {
    let mut lowest = u64::MAX;
    let mut highest = 0;
    for i in 0..MAX_REGIONS {
        let base = REGION_BASE[i].load(Ordering::Relaxed);
        let len = REGION_LEN[i].load(Ordering::Relaxed);
        if len == 0 {
            continue;
        }
        lowest = lowest.min(base);
        highest = highest.max(base.saturating_add(len));
    }
    (highest > 0).then_some((lowest, highest))
}

/// How an address-shaped value that no region covers should be described.
///
/// Split from the rendering so the sentence is testable without a run, which is the same reason
/// [`locate`] is pure: the message matters most exactly where it cannot be stepped through.
#[must_use]
pub fn describe_unreadable(address: u64) -> String {
    const UNREADABLE: &str = "in no span this run published as readable, and address-shaped";
    match published_envelope() {
        Some((low, high)) if address < low || address >= high => {
            format!(
                "{UNREADABLE} - and outside every region this run gave the guest ({low:#x}..{high:#x})"
            )
        }
        _ => UNREADABLE.to_owned(),
    }
}
/// Which breakpoint kind an address deserves, given where the stub table is.
///
/// **Pure, and split from the lookup for the reason [`locate`] is: the handler that uses it
/// cannot be stepped through.** A wrong answer here is a message asserting a cause at the
/// exact moment the message matters most, which is how every breakpoint came to be reported
/// as stub padding without anything having looked at the address (D576).
///
/// Three answers, because there are three things that can be known: the address is inside the
/// stub table, it is outside a table whose span is known, or no span was registered and the
/// question cannot be asked. Collapsing the last two would be the same overstatement in
/// miniature.
#[must_use]
pub fn breakpoint_kind_in(address: u64, stub_base: u64, stub_len: u64) -> &'static str {
    use orbistoun_report::trace::FaultSite;
    if stub_len == 0 {
        return FaultSite::BREAKPOINT_UNPLACED;
    }
    if address >= stub_base && address < stub_base.saturating_add(stub_len) {
        FaultSite::BREAKPOINT_IN_STUBS
    } else {
        FaultSite::BREAKPOINT_OUTSIDE_STUBS
    }
}

/// The breakpoint kind for `address`, against the stub span this process registered.
///
/// The thin effectful half: it reads the two atomics [`describe_region`] filled and hands
/// them to the decision above.
#[must_use]
pub fn breakpoint_kind(address: u64) -> &'static str {
    breakpoint_kind_in(
        address,
        REGION_BASE[Region::Stubs.slot()].load(Ordering::Relaxed),
        REGION_LEN[Region::Stubs.slot()].load(Ordering::Relaxed),
    )
}

/// A fixed-size line builder.
///
/// No allocation and no locks, because a handler may run while the allocator lock is
/// held by the code that just crashed.
#[derive(Debug)]
pub struct Line {
    buffer: [u8; Self::CAPACITY],
    used: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Line {
    /// Longest report this can hold. Anything beyond is dropped rather than wrapped.
    ///
    /// Raised from 512 when the report began carrying the faulting instruction's bytes and the
    /// window before it: two hex runs cost ~200 characters, and the register and frame lines that
    /// follow them are the ones that must never be the part that gets dropped. A stack buffer in a
    /// fault handler, so the room is close to free.
    pub const CAPACITY: usize = 1024;

    /// An empty line.
    pub const fn new() -> Self {
        Self {
            buffer: [0; Self::CAPACITY],
            used: 0,
        }
    }

    /// Appends text, truncating rather than overflowing.
    pub fn text(&mut self, text: &str) -> &mut Self {
        let room = Self::CAPACITY - self.used;
        let take = text.len().min(room);
        self.buffer[self.used..self.used + take].copy_from_slice(&text.as_bytes()[..take]);
        self.used += take;
        self
    }

    /// Appends a hexadecimal number, prefixed.
    ///
    /// Hand-rolled because the formatting machinery allocates, and this runs where
    /// allocating may deadlock.
    pub fn hex(&mut self, value: u64) -> &mut Self {
        self.text("0x");
        let mut digits = [0_u8; 16];
        let mut count = 0;
        let mut rest = value;
        loop {
            let nibble = (rest & 0xF) as usize;
            digits[count] = b"0123456789abcdef"[nibble];
            count += 1;
            rest >>= 4;
            if rest == 0 {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            if self.used < Self::CAPACITY {
                self.buffer[self.used] = digits[count];
                self.used += 1;
            }
        }
        self
    }

    /// Appends one byte as two hexadecimal digits, unprefixed - for a run of raw bytes, where a
    /// `0x` before each would be noise. Hand-rolled for the same reason [`Self::hex`] is.
    pub fn byte(&mut self, value: u8) -> &mut Self {
        if self.used + 2 <= Self::CAPACITY {
            self.buffer[self.used] = b"0123456789abcdef"[(value >> 4) as usize];
            self.buffer[self.used + 1] = b"0123456789abcdef"[(value & 0xF) as usize];
            self.used += 2;
        }
        self
    }

    /// Appends an address and, when it falls in a known region, where that is.
    pub fn address(&mut self, value: u64) -> &mut Self {
        self.hex(value);
        if let Some((name, offset)) = locate(value) {
            self.text(" (").text(name).text("+").hex(offset);
            // **And the guest's own name for it, where the module carries one.** An offset
            // into the image is a byte; a function name is a place to start reading. Only
            // the image, because the other regions are ours and are named already (D628).
            if name == REGION_NAMES[Region::Image as usize]
                && let Some((symbol, into)) = guest_code_site(offset)
            {
                self.text(" ").text(symbol).text("+").hex(into);
            }
            self.text(")");
        } else if let Some((name, into)) = orbistoun_thunk::data_symbol_at(value) {
            // **The sixth region, without a sixth slot.** A data import is a zeroed page this
            // project handed the guest, and the five regions the handler knows do not include
            // them - so a register pointing at one printed as a bare number, and twenty blank
            // C++ vtables were invisible in the fault that came out of them (D639).
            self.text(" (data ")
                .text(name)
                .text("+")
                .hex(into)
                .text(")");
        }
        self
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer[..self.used]
    }
}

/// The module being run, for the trace a fault handler writes.
static MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Names the module a fault report belongs to.
pub fn describe_module(module: String) {
    let _ = MODULE.set(module);
}

/// Whether a fault has already been reported.
///
/// A faulting handler can be re-entered - the report itself may fault, or the exception
/// may be raised again as it unwinds - and a loop of half-written messages would bury
/// the one that mattered.
#[cfg(windows)]
static REPORTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Whether `len` bytes at `address` can be read without faulting.
///
/// # Why the fault reporter has to ask
///
/// Both byte windows below used to assume the page holding the faulting instruction pointer was
/// mapped, "because the guest was executing in it". That is true of a **data** fault and false of
/// an **execute** one: when a guest jumps to an address that is not code, `rip` *is* the unmapped
/// address, and reading the instruction at it faults a second time. A fault inside a fault handler
/// is not reported - it ends the process - so the first fault was never recorded and the run
/// produced no trace at all (D471).
///
/// `VirtualQuery` allocates nothing and reads nothing at `address`, so it is safe on this path.
#[cfg(windows)]
fn readable(address: u64, len: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery,
    };

    if address == 0 || len == 0 {
        return false;
    }
    // SAFETY: every field of this structure is a plain integer or pointer, so all-zero is a
    // valid initialised value; `VirtualQuery` overwrites it before it is read.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let size = size_of::<MEMORY_BASIC_INFORMATION>();
    // SAFETY: `info` is a live, correctly sized buffer this call owns. `VirtualQuery` only
    // *describes* the mapping at `address` - it does not dereference it - so asking about an
    // unmapped address is exactly what it is for.
    let written = unsafe { VirtualQuery(address as *const core::ffi::c_void, &raw mut info, size) };
    if written == 0 || info.State != MEM_COMMIT {
        return false;
    }
    // A guard page is committed and still faults on touch, which is the whole point of it.
    if info.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    // The whole window has to be inside the one region: the next one may be unmapped.
    let end = (info.BaseAddress as u64).saturating_add(info.RegionSize as u64);
    address.saturating_add(len as u64) <= end
}

/// Away from Windows the fault reporter is not implemented at all, so nothing here is reached.
/// Answering "not readable" keeps the byte windows empty rather than risking a second fault.
#[cfg(not(windows))]
#[cfg(windows)]
const fn readable(_address: u64, _len: usize) -> bool {
    false
}

/// The host module holding `address`, as a file name and the offset into it.
///
/// # Why a fault in host code needs a name
///
/// **A bare host address is not a reproducible measurement.** PPSA02664 and PPSA03416 both end
/// in host code, and the number moves: `0x7fff13abdc8d` one day, `0x7ff9c071dc8d` the next, for
/// the same fault reached the same way. Windows bases system modules per boot, so it is stable
/// within a boot session and changes across reboots. Two things follow, and both are bad:
///
/// - the recorded outcome in `compat/` **cannot be reproduced after a reboot**, so what reads as
///   a measurement of the guest is partly a measurement of where the host loader put something;
/// - `compare` sees the ending change whenever the machine rebooted between two runs, with
///   nothing having changed - a false signal in the only measure of progress this project has.
///
/// A module's own base is stable relative to itself, so `name+offset` reproduces - and it says
/// *which* host code faulted, where the address said only that some did. (worklog 594)
///
/// # The file name, never the path
///
/// `GetModuleFileNameW` answers a full path. Only the last component is kept, because this
/// string is written into `compat/` and an absolute path from this machine must never reach a
/// tracked file - the identity guard blocks exactly that, and it is right to.
///
/// Runs after the fault rather than inside the handler, on the same terms as the call trace
/// below it: this allocates, and by then the process is only assembling its report.
#[cfg(windows)]
fn host_module_of(address: u64) -> Option<(String, u64)> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};

    if address == 0 {
        return None;
    }
    // SAFETY: every field is a plain integer or pointer, so all-zero is a valid initialised
    // value; `VirtualQuery` overwrites it before it is read.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a live, correctly sized buffer this call owns. `VirtualQuery` only
    // describes the mapping at `address` - it never dereferences it - so asking about an
    // address that just faulted is exactly what it is for.
    let written = unsafe {
        VirtualQuery(
            address as *const core::ffi::c_void,
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 {
        return None;
    }
    // The allocation base of a mapped image is its module handle, which is what makes this
    // two calls rather than a module walk.
    // **`base == 0` is not a formality.** `GetModuleFileNameW(NULL)` is documented to answer the
    // *current executable*, so without this an address in no mapping comes back named as this
    // binary at a nonsense offset - a fabricated location inside a record that reads as a
    // measurement, which is principle 3's failure exactly. The negative test pins it.
    let base = info.AllocationBase as u64;
    if base == 0 || address < base {
        return None;
    }

    // Generous rather than `MAX_PATH`: a truncated answer keeps the *prefix*, and the prefix is
    // the directory this deliberately throws away - so truncation would cost the one part worth
    // having.
    let mut buffer = [0_u16; 1024];
    // SAFETY: `base` is the allocation base reported above and `buffer` is a live array of the
    // length passed. An address that is not a mapped image answers zero rather than misbehaving.
    let len = unsafe {
        GetModuleFileNameW(
            base as *mut core::ffi::c_void,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).unwrap_or(u32::MAX),
        )
    };
    if len == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    let name = path.rsplit(['\\', '/']).next()?.to_owned();
    (!name.is_empty()).then(|| (name, address - base))
}

/// Away from Windows nothing reaches the fault reporter, so no host module is ever named.
#[cfg(not(windows))]
#[cfg(windows)]
const fn host_module_of(_address: u64) -> Option<(String, u64)> {
    None
}

/// The bytes of the faulting instruction, copied straight from the instruction pointer.
///
/// Returns a fixed buffer and how many of it are valid - no allocation, because this runs inside the
/// fault. The read is **clamped to the page `ip` sits in**: that page is mapped and readable (the
/// guest was executing in it, and execute implies read here - D065), so the read is sound; stopping
/// at the page boundary means it never reaches into a neighbour that may be unmapped, which is the
/// one way reading an instruction could fault a second time. A null or wrapped pointer yields zero
/// bytes rather than a guess.
#[cfg(windows)]
fn instruction_bytes(ip: u64) -> ([u8; 16], usize) {
    const PAGE: u64 = 0x1000;
    let mut out = [0_u8; 16];
    if ip == 0 {
        return (out, 0);
    }
    let to_page_end = (PAGE - (ip & (PAGE - 1))) as usize;
    let len = to_page_end.min(out.len());
    // **Checked, not assumed.** An execute fault puts `rip` *at* the unmapped address, so the
    // page the guest "was executing in" may not exist (D471).
    if !readable(ip, len) {
        return (out, 0);
    }
    // SAFETY: `readable` has just confirmed `len` bytes at `ip` are committed and readable, and
    // `len` is clamped to the remainder of one page, so the slice cannot cross into an unmapped
    // neighbour. The bytes are read once into `out` and the borrow does not outlive this call.
    let src = unsafe { std::slice::from_raw_parts(ip as *const u8, len) };
    out[..len].copy_from_slice(src);
    (out, len)
}

/// The bytes immediately *before* the faulting instruction - the code that set up the registers it
/// faulted on, which a null write (`mov [rax], rax` with `rax` zero) needs to be read backwards from
/// to find where the null came from.
///
/// Clamped to the **start** of the page `ip` sits in, the mirror of [`instruction_bytes`]'s clamp to
/// the end: the whole window stays in the one mapped page, so reading it cannot fault, and a fault
/// near a page boundary simply yields fewer bytes rather than reaching into a neighbour.
#[cfg(windows)]
fn bytes_before(ip: u64) -> ([u8; 48], usize) {
    const PAGE: u64 = 0x1000;
    let mut out = [0_u8; 48];
    if ip == 0 {
        return (out, 0);
    }
    let into_page = (ip & (PAGE - 1)) as usize;
    let len = into_page.min(out.len());
    let start = ip - len as u64;
    // The same check, and the read that actually killed a run: `bytes_before(0x7fff0001)` starts
    // at `0x7fff0000`, one byte below a placeholder the guest had jumped to (D471).
    if !readable(start, len) {
        return (out, 0);
    }
    // SAFETY: `readable` has just confirmed `len` bytes at `start` are committed and readable;
    // `start .. ip` lies within the single page holding `ip`, so the slice never precedes that
    // page's start. Read once into `out`, borrow not held past the call.
    let src = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
    out[..len].copy_from_slice(src);
    (out, len)
}

/// Written explicitly so a report line is one line however the stream is buffered.
#[cfg(windows)]
const NEWLINE: &str = "
";

/// Says that the guest used one of orbistoun's own refusals as an address.
///
/// # Why this is not the emulator-bug message
///
/// The two look identical to the check above: a stub answers `0x7FFF_0001`, the guest reads it
/// as a pointer and jumps through it, and the instruction pointer is then outside every region
/// orbistoun placed - which is exactly the test for "our code faulted".
///
/// **It is the opposite diagnosis.** Nothing in this codebase misbehaved; a function nobody has
/// written answered the way it is designed to, and the guest believed it. The reader wants the
/// import in the header - that is the function to implement - not a host stack trace of the
/// emulator working correctly (D128, D154, D186, D299).
#[cfg(windows)]
fn note_placeholder_fault(line: &mut Line, what: &'static str, exact: bool) {
    line.text("  >> NOT AN EMULATOR BUG: this address is orbistoun's own `");
    line.text(what);
    line.text(if exact { "` answer" } else { "` block" });
    line.text(NEWLINE);
    line.text(
        "     - a stub returned it and the guest used it as a pointer. The emulator did what",
    );
    line.text(NEWLINE);
    line.text("     it was built to do, so the host stack below is the trampoline, not the cause.");
    line.text(NEWLINE);
    line.text("     **The import in the header is the last call, not the source** - it is usually");
    line.text(NEWLINE);
    line.text("     implemented. Look down the recorded calls for the most recent one returning");
    line.text(NEWLINE);
    line.text("     this value: that is the function to write.");
    line.text(NEWLINE);
}

/// A fault address this far from zero or below is a null pointer plus a field offset, not an
/// address the guest computed. Matches `orbistoun-report`'s `NEAR_NULL` (a page).
#[cfg(windows)]
const NEAR_NULL_FAULT: u64 = 0x1000;

/// Whether a fault **in orbistoun's own code** is a null-ish pointer - the guest's, handed to a libc
/// shim - rather than an emulator logic bug. Pure, so the threshold is tested without raising a real
/// fault. Null plus a field offset below a page is a dereferenced null structure, not an address any
/// running code computed.
#[cfg(windows)]
fn is_null_pointer_fault(faulting_address: u64) -> bool {
    faulting_address < NEAR_NULL_FAULT
}

/// Says, unmistakably, that the fault is in orbistoun's own code rather than the guest's.
///
/// The header names the guest's *last import call* for context, which reads as "the guest
/// faulted in this function" when it means "the emulator faulted, and this is the last thing
/// the guest asked for". That misreading cost a whole investigation once, so it is spelled out:
/// the function that actually faulted is in the host stack below, never the import above.
#[cfg(windows)]
fn note_emulator_fault(line: &mut Line) {
    line.text("  >> EMULATOR BUG: the fault is in orbistoun's OWN code, not the guest's - the");
    line.text(NEWLINE);
    line.text(
        "     instruction pointer is outside the guest image. The import in the header is the",
    );
    line.text(NEWLINE);
    line.text(
        "     guest's last call (context only); the function that actually faulted is in the",
    );
    line.text(NEWLINE);
    line.text("     host stack below, not the 'nearest implementation' line.");
    line.text(NEWLINE);
}

/// The refinement of [`note_emulator_fault`] for a **null-ish** faulting address in orbistoun's code.
///
/// The instruction pointer being outside the guest image is the same test either way, but a fault
/// address of null-plus-a-small-offset is a different diagnosis: it is the signature of the guest
/// handing an **unpopulated pointer** to a libc function - `memcpy`, `strlen`, `memset` - which the
/// host stack below shows as `orbistoun_libc`. When it does, the guest's own libc would fault the
/// same way on real hardware, so this is not the emulator's logic misbehaving - the cause is
/// upstream, the call that should have filled the pointer, exactly the reframe `int 0x41` needed
/// (worklog 611). Calling this "EMULATOR BUG" sends the reader to debug orbistoun's `memcpy`, which
/// is correct, instead of the guest state that is not. Stated conditionally, because a null deref in
/// orbistoun's *own* logic lands here too, and the host stack is what tells the two apart.
#[cfg(windows)]
fn note_null_pointer_to_libc(line: &mut Line) {
    line.text("  >> NULL-ISH ADDRESS IN OWN CODE: the instruction pointer is in orbistoun's code");
    line.text(NEWLINE);
    line.text(
        "     and the faulting address is null plus a small offset. When the host stack below",
    );
    line.text(NEWLINE);
    line.text("     is orbistoun_libc, this is the guest handing an unpopulated pointer to");
    line.text(NEWLINE);
    line.text(
        "     memcpy/strlen - its own libc would fault the same on hardware, so the cause is",
    );
    line.text(NEWLINE);
    line.text(
        "     upstream (the call that should have filled the pointer), not this emulator. When",
    );
    line.text(NEWLINE);
    line.text("     the stack is not a libc shim, it is an emulator null-deref after all.");
    line.text(NEWLINE);
}

/// Services a faulting instruction that is a software interrupt, if a handler is registered.
///
/// The decision half of the interrupt glue, pure so it can be tested without raising a real trap:
/// the `CONTEXT` read and write-back stay in the vectored handler, and everything that could be
/// *wrong* - is this an `int`, which vector, where does it resume, did a handler run - is here.
///
/// Returns the frame to resume the guest from when the instruction is an `int n` with a registered
/// handler; `None` otherwise, which is every case today, because the table is empty until obSCEne
/// measures a vector (worklog 608). The resume point is set past the two-byte `int` before the
/// handler runs, so a handler that touches nothing returns to the instruction after the trap.
#[cfg(windows)]
fn serviced_interrupt(
    opcode: &[u8],
    mut frame: orbistoun_kernel::interrupt::InterruptFrame,
) -> Option<orbistoun_kernel::interrupt::InterruptFrame> {
    let orbistoun_report::trace::TrapKind::KernelEntry {
        vector: Some(vector),
    } = orbistoun_report::trace::classify_trap(opcode)?
    else {
        return None;
    };
    // `cd NN` is two bytes; resume after it unless the handler jumps.
    frame.rip = frame.rip.wrapping_add(2);
    orbistoun_kernel::interrupt::service(vector, &mut frame).then_some(frame)
}

/// Names a privileged or trap faulting instruction, and explains an all-ones fault address.
///
/// A guest executing `syscall`, `sysenter`, `int`, `hlt` or `ud2` has gone *under* the library
/// boundary this project intercepts (D378): there is no import to name and no library stub to
/// write - it wants kernel-level support. And the host turns the general-protection fault such
/// an instruction (or a misaligned SSE access) raises into an access violation at
/// `0xffffffffffffffff`, which looks exactly like a guest dereferencing -1 and is not a pointer
/// at all - a confusion that cost a dozen wrong eliminations before it was understood (D384).
#[cfg(windows)]
fn note_instruction_shape(line: &mut Line, opcode: &[u8], faulting_address: u64) {
    // A software interrupt carries its vector in the byte after `0xcd`, and that vector is the
    // whole actionable fact: it names which kernel entry the guest took and, therefore, exactly
    // what has to be characterised. Reporting a bare "int" threw that away and read as a shrug -
    // it is what sent one investigation off after a filesystem path when the answer was "int 0x41
    // is a kernel entry we do not implement" (worklog 603).
    //
    // Written straight into the line, never through `format!`: this runs in the fault handler
    // where allocating may deadlock, which is the whole reason `Line::hex` is hand-rolled.
    //
    // **The classification is `orbistoun_report::trace::classify_trap`, the same one the ranked
    // finding uses** - one classifier so the crash print and the worklist cannot name the same
    // fault two different ways. It is pure and allocation-free, so it is safe here.
    use orbistoun_report::trace::{TrapKind, classify_trap};
    let Some(kind) = classify_trap(opcode) else {
        if faulting_address == u64::MAX {
            line.text(
                "  >> 0xffffffffffffffff is usually a general-protection fault reported by the host",
            );
            line.text(NEWLINE);
            line.text("     (a misaligned SSE access, or a privileged instruction), not a genuine read of -1 (D384).");
            line.text(NEWLINE);
        }
        return;
    };
    {
        // int 0x41 is measured fatal (obSCEne REQ-...b3c2): a guest trap whose cause is upstream,
        // not a kernel entry awaiting a handler - so it is headed and explained as a trap, even
        // though `classify_trap` decodes its bytes as a `KernelEntry` shape. Every other vector
        // keeps the kernel-entry framing until it too is measured. One classifier still, one extra
        // fact layered on the one vector a measurement has settled.
        let int_0x41 = opcode.first() == Some(&0xcd) && opcode.get(1) == Some(&0x41);
        line.text(if int_0x41 {
            "  >> GUEST TRAP (int 0x41, measured fatal): the faulting instruction is "
        } else {
            match kind {
                TrapKind::KernelEntry { .. } => {
                    "  >> KERNEL ENTRY, UNIMPLEMENTED: the faulting instruction is "
                }
                TrapKind::GuestTrap => "  >> GUEST TRAP: the faulting instruction is ",
            }
        });
        match opcode.first().copied() {
            Some(0xf4) => {
                line.text("hlt");
            }
            Some(0xcd) => {
                // The vector is the actionable half. `int 0x41` names the kernel entry exactly.
                line.text("int ")
                    .hex(u64::from(opcode.get(1).copied().unwrap_or(0)));
                line.text(" (a software interrupt)");
            }
            Some(0x0f) if opcode.get(1) == Some(&0x34) => {
                line.text("sysenter");
            }
            Some(0x0f) if opcode.get(1) == Some(&0x0b) => {
                line.text("ud2 (a deliberate trap)");
            }
            _ => {
                line.text("syscall");
            }
        }
        line.text(NEWLINE);
        if int_0x41 {
            // The measurement refuted "characterise it, then add the handler": there is nothing to
            // add. A bare int 0x41 faults on hardware too, so a guest reaching it took a path real
            // hardware would fault on - which puts the cause upstream, the same place a ud2 points.
            line.text(
                "     Measured fatal on retail: a bare int 0x41 raises a signal and does not return",
            );
            line.text(NEWLINE);
            line.text(
                "     (obSCEne REQ-...b3c2), so it is not a kernel service to add. The guest reached it",
            );
            line.text(NEWLINE);
            line.text(
                "     via an upstream wrong value, like a ud2 abort: the cause is what it was told just before.",
            );
            line.text(NEWLINE);
        } else if matches!(kind, TrapKind::KernelEntry { .. }) {
            // The load-bearing reframe: this is orbistoun's gap, stated as one, with the next
            // step. orbistoun resolves the library boundary (imports) and the syscall-gadget path
            // (D376), but implements no interrupt/trap vectors at all (D378) - so any guest that
            // enters the kernel this way stops here regardless of what it is. The old wording -
            // "orbistoun does not intercept ... no import is to blame" - was true and useless; it
            // read as "nothing to do", when the thing to do is precise.
            line.text(
                "     The guest entered the kernel through an instruction orbistoun does not implement.",
            );
            line.text(NEWLINE);
            line.text(
                "     This is orbistoun's gap, not the guest's - a retail title reaching it runs on real",
            );
            line.text(NEWLINE);
            line.text(
                "     hardware, so the wall is here. Next step: characterise what this entry reads and",
            );
            line.text(NEWLINE);
            line.text(
                "     returns (an obSCEne measurement, since it is below the NID/library layer), then add",
            );
            line.text(NEWLINE);
            line.text("     the handler. No import is involved and none is to blame.");
            line.text(NEWLINE);
        } else {
            line.text(
                "     A deliberate trap the guest raised itself - typically an assertion or an abort after",
            );
            line.text(NEWLINE);
            line.text(
                "     a check it failed. The cause is upstream: read what the guest was told just before.",
            );
            line.text(NEWLINE);
        }
    }
    if faulting_address == u64::MAX {
        line.text(
            "  >> 0xffffffffffffffff is usually a general-protection fault reported by the host",
        );
        line.text(NEWLINE);
        line.text("     (a misaligned SSE access, or a privileged instruction), not a genuine read of -1 (D384).");
        line.text(NEWLINE);
    }
}

/// Emits one fault report.
///
/// Shared by both platforms so the wording, and the region attribution, cannot drift
/// between them. `kind` carries its own preposition - "read of" wants the address
/// straight after it, "illegal instruction at" does not - so nothing is inserted here.
#[allow(
    clippy::too_many_lines,
    reason = "one linear report builder; each block appends one labelled section and splitting them would scatter the fault report across functions for no reader's benefit"
)]
#[cfg(windows)]
fn emit(kind: &str, faulting_address: u64, instruction_pointer: u64, registers: Registers) {
    use std::io::Write as _;

    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut line = Line::new();
    line.text("orbistoun: guest fault: ").text(kind).text(" ");
    line.address(faulting_address);
    line.text(" while executing at ");
    line.address(instruction_pointer);

    // Naming the import is what turns "somewhere in the emulator" into "in this function".
    // Everything up to here is allocation-free and stays that way: the label is a
    // `&'static str` from the import table, not a formatted string.
    let inside = inside_import_name(instruction_pointer);
    if let Some(name) = inside {
        line.text(" (inside ").text(name).text(")");
    }
    line.text(NEWLINE);

    // **Say loudly whose bug this is** - `inside` is set only when the instruction pointer is
    // outside the guest image, which used to be taken as "orbistoun's own code faulted".
    //
    // That test cannot tell our code from **our own placeholder answers**: a stub returns
    // `0x7FFF_0001`, the guest jumps through it, and the instruction pointer is outside every
    // placed region for a reason that has nothing to do with a bug here. Checked first, because
    // the two messages send a reader to opposite places.
    let placeholder = orbistoun_core::placeholder_named(instruction_pointer)
        .or_else(|| orbistoun_core::placeholder_named(faulting_address));
    if let Some((what, exact)) = placeholder {
        note_placeholder_fault(&mut line, what, exact);
    } else if inside.is_some() {
        // A null-ish faulting address in our code is usually the guest handing a libc shim an
        // unpopulated pointer, not orbistoun's logic misbehaving - said apart so the reader is not
        // sent to debug a correct `memcpy` (the `int 0x41` lesson, worklog 611).
        if is_null_pointer_fault(faulting_address) {
            note_null_pointer_to_libc(&mut line);
        } else {
            note_emulator_fault(&mut line);
        }
    }

    // **Where in *our* code, when it is our code.** `inside` names the last import called,
    // which is an attribution rather than a location: it says which function the guest
    // wanted, not which instruction faulted. For a fault in guest code that is the whole
    // story, and this project has never needed more - a fault in the emulator's own code was
    // always somebody's bug to find by reading.
    //
    // It is not enough once an implementation is complicated enough to fault inside itself.
    // The address is useless on its own because it is randomised per run, so what is printed
    // is its distance from a function in this file: add that to `emit`'s own offset in the
    // binary and the symbol is one `nm` away (D380).
    if inside.is_some()
        && placeholder.is_none()
        && let Some((name, offset)) = own_code_site(instruction_pointer)
    {
        line.text("  in orbistoun's own code, nearest implementation is ");
        line.text(name);
        line.text("+");
        line.hex(offset);
        line.text(NEWLINE);
    }

    // **The faulting instruction itself.** A fault in a wrapper-encoded guest is the one case the
    // location alone cannot be turned into an instruction by hand: the ELF `p_offset` fields do not
    // locate the loaded bytes, only the loader's wrapper decode does, so `image+0x…` names a
    // disassembly nobody can reach from the file. These bytes come straight from where the guest was
    // executing, so the report *is* the disassembler's input rather than a place to go and look
    // (D065 established this memory is readable: execute never drops read here).
    let (before, before_len) = bytes_before(instruction_pointer);
    if before_len > 0 {
        line.text("  before ");
        for &b in &before[..before_len] {
            line.byte(b).text(" ");
        }
        line.text(NEWLINE);
    }
    let (opcode, len) = instruction_bytes(instruction_pointer);
    if len > 0 {
        line.text("  bytes ");
        for &b in &opcode[..len] {
            line.byte(b).text(" ");
        }
        line.text(NEWLINE);
    }

    // Name a privileged/trap faulting instruction, and explain the all-ones address, when they
    // apply - the two most misleading fault shapes to read otherwise (D378, D384).
    note_instruction_shape(&mut line, &opcode[..len], faulting_address);

    // The stack pointer earns its place on the first line: "the guest ran out of stack"
    // and "a pointer was wrong" look identical without it, and the first is a whole class
    // of failure this project can cause by running host code on a guest stack.
    // The path through the guest's own code, which is the part an instruction pointer
    // alone cannot give. Written before the registers because it is what gets read first.
    for frame in walk_frames(registers.rbp) {
        line.text("  from ");
        line.address(frame.return_address);
        line.text(NEWLINE);
    }

    line.text("  rsp ");
    line.address(registers.rsp);
    line.text("  rdi ");
    line.hex(registers.rdi);
    line.text("  rax ");
    line.hex(registers.rax);
    line.text(NEWLINE);

    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();

    // **The host stack, but only when the fault is ours.**
    //
    // The lines above are allocation-free and are out of the door before this runs, because
    // capturing a backtrace allocates, takes locks, and reads the symbol file - none of which
    // a fault handler should do before it has said the thing that matters.
    //
    // Only when the fault is in orbistoun's own code. A guest that faults on its own pointer
    // has a host stack of *this emulator's dispatch machinery*, which is noise; a guest that
    // faulted this process has one that is the whole answer, and naming the nearest
    // implementation stops short of it (D381).
    if inside.is_some() {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let mut err = std::io::stderr();
        let _ = writeln!(err, "  the host stack that got there:");
        for one in backtrace.to_string().lines().take(40) {
            let _ = writeln!(err, "  {one}");
        }
        let _ = err.flush();
    }

    // What the guest asked the kernel for directly, which a fault is very often the end of.
    // Same trade as the trace below: this allocates, and it runs after everything that must
    // not.
    what_the_guest_asked_for();

    // Then the call trace, which is the part worth having. A guest that faults has
    // still said what it wanted, and losing that means the run produced nothing.
    //
    // This allocates, which the formatting above deliberately does not. The trade is
    // deliberate: a vectored handler runs in ordinary user context on a thread that
    // faulted on a *guest* pointer, so the allocator is not the thing that broke, and
    // the alternative is discarding the only output the run had. If it ever does
    // deadlock, it deadlocks a process that was about to die anyway.
    let module = MODULE.get().map_or("unknown", String::as_str);
    // A region orbistoun placed if it is one, and otherwise the host module the address falls
    // in - which is the difference between an outcome that reproduces and one that moves every
    // reboot. The bare address survives in `instruction_pointer` either way.
    let (region, offset) = match locate(instruction_pointer) {
        Some((name, offset)) => (Some(name.to_owned()), Some(offset)),
        None => match host_module_of(instruction_pointer) {
            Some((name, offset)) => (Some(name), Some(offset)),
            None => (None, None),
        },
    };
    let trace = collect_with_fault(
        module,
        "Entered",
        Some(FaultSite {
            kind: kind.to_owned(),
            address: faulting_address,
            instruction_pointer,
            region,
            offset,
            inside_import: inside_import_name(instruction_pointer).map(str::to_owned),
            // The handler runs on the faulting thread, so this is that thread's handle.
            // Zero is "nothing claimed this host thread", which is a real state and not a
            // thread called zero - so it is `None` rather than a number nobody issued.
            thread: Some(orbistoun_kernel::thread::current()).filter(|h| *h != 0),
            // The same identifier the recorded calls carry, so the tail can be filtered to
            // this thread rather than read as though every line were on it (D621).
            host_thread: Some(orbistoun_thunk::current_thread()),
            registers: Some(registers),
            pointees: describe_pointees(&registers),
            frames: walk_frames(registers.rbp),
            // The faulting instruction's bytes, so the ranked findings can classify the trap the
            // way the live print does - a kernel entry (`int 0x41`) names itself in the worklist,
            // not only in the crash output (worklog 605).
            instruction: {
                let (opcode, len) = instruction_bytes(instruction_pointer);
                opcode[..len].to_vec()
            },
        }),
    );
    persist(&trace);
}

/// What each register holding a readable address is pointing at.
///
/// # Why a fault needs this and the argument dump was not enough
///
/// The argument dumper has named and dumped pointers since D198, but only for imports. A fault
/// prints sixteen bare values, and at this title's wall the one that mattered was a short text
/// label in `r12` - reading it meant arming a watchpoint on a stack address that orbistoun's own
/// shims churn, which filled the recorder with host sites and never showed the guest's access
/// (D522).
///
/// **Readable is asked, never assumed.** `is_mapped` is the same question the argument dumper
/// asks, and a register that fails it is left out entirely rather than reported as unreadable -
/// most of the sixteen hold scalars, and sixteen "not an address" lines would bury the two that
/// are.
#[cfg(windows)]
fn describe_pointees(registers: &Registers) -> Vec<String> {
    if !orbistoun_thunk::ranges_known() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, value) in registers.named() {
        if value == 0 || !orbistoun_thunk::is_mapped(value) {
            continue;
        }
        let Ok(at) = usize::try_from(value) else {
            continue;
        };
        // SAFETY: `is_mapped` says this address is inside a region this run published as
        // readable, and sixteen bytes from it stay inside it - the ranges are page-granular
        // and no published range is shorter than that.
        let bytes: [u8; 16] =
            unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<[u8; 16]>(at)) };
        let hex = bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        // **And the data-import pages, which are not one of the five regions.** A register
        // pointing at a zeroed page this project handed the guest printed as a bare number, so
        // twenty blank C++ vtables were invisible in the fault that came out of one (D639).
        let where_ = locate(value).map_or_else(
            || {
                orbistoun_thunk::data_symbol_at(value).map_or_else(
                    || format!("{value:#x}"),
                    |(name, into)| format!("data {name}+{into:#x}"),
                )
            },
            |(region, offset)| format!("{region}+{offset:#x}"),
        );
        // The text form only when it is text: a run of printable bytes ending in a
        // terminator is a string a guest passed, and rendering arbitrary bytes as characters
        // would invent one.
        let text = printable_text(&bytes).map_or_else(String::new, |t| format!("  {t:?}"));
        out.push(format!("{name} -> {where_} = {hex}{text}"));
    }
    out
}

/// The text `bytes` holds, when it holds text and not merely bytes that could be read as some.
///
/// # The rule, and why it is this strict
///
/// A terminated run of printable characters is a string a guest passed. Anything else is bytes,
/// and rendering bytes as characters **invents a string** - which is the same failure as
/// inventing a constant, in the one place a reader is most likely to believe it (principle 3).
///
/// So all three must hold: something before the terminator, a terminator inside the window
/// (otherwise the run is only the start of something longer and the text would be a fragment),
/// and every character printable. `"None"` passes; a pointer that happens to begin `0x65 0x4e`
/// does not.
#[cfg(windows)]
fn printable_text(bytes: &[u8]) -> Option<String> {
    let text: String = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| char::from(*b))
        .collect();
    let terminated = text.len() < bytes.len();
    (!text.is_empty() && terminated && text.chars().all(|c| c.is_ascii_graphic() || c == ' '))
        .then_some(text)
}

/// Says which paths the guest asked for and did not get.
///
/// **The filesystem's most useful output.** The mount table is two entries wide and what else
/// belongs in it has been an open research question; it is not one. The guest names them, and
/// a mount added afterwards then answers a request that was actually made (D387).
///
/// Read here rather than printed there, as everything else on the guest's stack is (D381).
pub(crate) fn paths_wanted() {
    use std::io::Write as _;

    let wanted = orbistoun_fs::wanted::unanswered();
    if wanted.is_empty() {
        return;
    }
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: the guest asked for {} path{} nothing here holds:",
        wanted.len(),
        if wanted.len() == 1 { "" } else { "s" }
    );
    for path in &wanted {
        let _ = writeln!(err, "  {path}");
        orbistoun_core::klog::note(&format!("orbistoun: no such path {path}"));
    }
    let _ = writeln!(
        err,
        "  each is a directory or file the platform has and the mount table does not - a work item, spelled by the thing that wanted it"
    );
    let _ = err.flush();
}

/// Says what the guest opened, when the run was asked to record it.
///
/// **The half [`paths_wanted`] cannot show.** That one names what was missing, and a run can
/// fail the opposite way: PPSA03416 opened what it asked for and still performed one read of
/// zero bytes, with only its four failed probes visible - all four of them a layout the title
/// does not use (D578). Off unless `ORBISTOUN_TRACE_OPENS` is set, because successes are the
/// common case and recording them is not free on the guest's stack.
///
/// **Silence and nothing-asked-for are different findings**, so a run that was recording and
/// opened nothing says so rather than printing nothing at all. That is the whole reason this
/// asks the recorder whether it was on.
pub(crate) fn paths_opened() {
    use std::io::Write as _;

    if !orbistoun_fs::opened::recording() {
        return;
    }
    let opened = orbistoun_fs::opened::answered();
    let mut err = std::io::stderr();
    if opened.is_empty() {
        let _ = writeln!(
            err,
            "orbistoun: the guest opened nothing - it read no file at all, which is a finding rather than an empty list"
        );
        let _ = err.flush();
        return;
    }
    let _ = writeln!(
        err,
        "orbistoun: the guest opened {} path{}:",
        opened.len(),
        if opened.len() == 1 { "" } else { "s" }
    );
    for path in &opened {
        let _ = writeln!(err, "  {path}");
    }
    let _ = err.flush();

    // **And what each read got.** A list of opens says the guest found its files; this says
    // what came back, which for a title that reads zero bytes is the whole finding (D595).
    let reads = orbistoun_fs::opened::reads_made();
    if reads.is_empty() {
        return;
    }
    let _ = writeln!(
        err,
        "orbistoun: and read from them {} time(s):",
        reads.len()
    );
    for line in &reads {
        let _ = writeln!(err, "  {line}");
    }
    let _ = err.flush();
}

/// Every mapping the guest was given, in the order it was given them.
///
/// **The half a run has never reported.** A run says which reservations *failed* and nothing
/// about the hundreds that succeeded, so a pointer into guest memory could not be traced back
/// to the call that produced it - and two runs whose arena addresses differ could be seen to
/// differ and not where. The arena is bump-allocated, so order is the finding and the list is
/// printed in it (D581).
pub(crate) fn maps_given() {
    use std::io::Write as _;

    if !orbistoun_kernel::mapped::recording() {
        return;
    }
    let given = orbistoun_kernel::mapped::given();
    let mut err = std::io::stderr();
    if given.is_empty() {
        let _ = writeln!(
            err,
            "orbistoun: the guest was given no mapping at all, which is a finding rather than an empty list"
        );
        let _ = err.flush();
        return;
    }
    // Cumulative, because the question a diff asks is *which* placement first moved - and an
    // address on its own does not say how much was placed before it.
    let mut total = 0_u64;
    let _ = writeln!(
        err,
        "orbistoun: the guest was given {} mapping(s):",
        given.len()
    );
    for (index, m) in given.iter().enumerate() {
        total = total.saturating_add(m.len);
        let _ = writeln!(
            err,
            "  {index:4}  call {:<8}  {:#018x} +{:#x}  {}{}  (cumulative {:#x})  during {}{}",
            m.at_call,
            m.base,
            m.len,
            if m.readable { "r" } else { "-" },
            if m.hinted { " asked-for" } else { " arena    " },
            total,
            // Named here because this is the layer that holds the import table. An index with
            // no name is still worth printing: it is stable across runs and diffs.
            m.during
                .and_then(|i| label_of(i as usize))
                .unwrap_or("nothing yet"),
            // A refusal reads as an event the guest asked for and did not get, which is what
            // it is - and is the line that was missing (D602).
            m.refused
                .as_deref()
                .map_or_else(String::new, |why| format!("  REFUSED: {why}"))
        );
    }
    let _ = err.flush();
}

/// Everything the guest asked the system for, in one call.
///
/// **One function because there are two call sites**, and they have to agree. A guest that
/// faults and a guest that stops cleanly leave through different paths, and each has to say the
/// same things - adding a fourth reporter to one of them and not the other is how a record ends
/// up existing for crashes and not for clean runs. That happened to `paths_opened` on the way in
/// (D578), and was found by testing rather than by reading.
/// Says what the guest formatted, which is usually how it explains itself.
///
/// **The last ones**, because a title says the interesting thing just before it stops. Printed
/// after the guest has stopped, like every other record here (D381, D590).
pub(crate) fn what_it_said() {
    use std::io::Write as _;

    if !orbistoun_libc::said::recording() {
        return;
    }
    let said = orbistoun_libc::said::rendered();
    let mut err = std::io::stderr();
    if said.is_empty() {
        let _ = writeln!(
            err,
            "orbistoun: the guest formatted nothing at all, which is a finding rather than an empty list"
        );
        let _ = err.flush();
        return;
    }
    let _ = writeln!(
        err,
        "orbistoun: the guest formatted {} string(s):",
        said.len()
    );
    for text in &said {
        let _ = writeln!(err, "  {text}");
    }
    let _ = err.flush();
}

/// Says when the argument dump ran out of room.
///
/// **A dump that was wanted and not taken reads as a call that passed nothing worth showing.**
/// The buffer holds a few hundred, and a guest calling many unimplemented functions fills it long
/// before an interesting one - so a run that dropped any has to say by how much, or asking for one
/// import and getting a list without it looks like an answer (D623).
pub(crate) fn dumps_dropped() {
    use std::io::Write as _;

    let dropped = orbistoun_thunk::dumps_dropped();
    if dropped == 0 {
        return;
    }
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: {dropped} argument dump(s) wanted after the buffer was full - name an import with ORBISTOUN_DUMP to spend the room on it"
    );
    let _ = err.flush();
}

/// Says when the argument dump ran out of room to remember where it may read.
///
/// **A dropped range and a wrong pointer print identically**, so a run that dropped any has to
/// say so - otherwise every unreadable argument in it reads as the guest's mistake (D588).
pub(crate) fn ranges_dropped() {
    use std::io::Write as _;

    let dropped = orbistoun_thunk::dropped_ranges();
    if dropped == 0 {
        return;
    }
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: {dropped} readable range(s) could not be remembered - an argument pointing into one of them reports as unreadable, and that is this run's blind spot rather than a bad pointer"
    );
    let _ = err.flush();
}

pub(crate) fn what_the_guest_asked_for() {
    syscalls_asked_for();
    dumps_dropped();
    bound_yet_called();
    tables_disagree();
    paths_wanted();
    paths_opened();
    maps_given();
    ranges_dropped();
    opening_calls();
    what_it_said();
}

/// Says what the guest asked the kernel for directly.
///
/// **Read here rather than printed there.** The dispatch path runs on the guest's stack, so it
/// only sets a bit; formatting and locking a stream from that frame is what put a fault in the
/// middle of the first syscall a guest ever made here (D381).
///
/// # Why it lives in the reporting module
///
/// It was written next to the other run conditions and called from `prepare_diagnostics` -
/// which runs *before* the guest is entered, so the record it reads is always empty and the
/// report could never say anything. **A fifth setting consulted nowhere** (D082, D166, D187,
/// D379), and the one that made the syscall boundary invisible for exactly as long as it had
/// existed. Both paths a run can end by call it now: the ordinary return, and the fault.
pub(crate) fn syscalls_asked_for() {
    use std::io::Write as _;

    // The sequence first, because *when* is what says what the guest was doing (D388).
    let sequence = orbistoun_thunk::syscall::syscalls_in_order();
    if !sequence.made.is_empty() {
        let mut err = std::io::stderr();
        let _ = writeln!(
            err,
            "orbistoun: the guest made {} syscalls, in this order:",
            sequence.total
        );
        for (position, asked) in sequence.made.iter().enumerate() {
            let called = asked.name.unwrap_or("nothing here implements it");
            let _ = writeln!(
                err,
                "  {position:3}  {:5}  {called}  ({:#x})",
                asked.number, asked.argument
            );
            orbistoun_core::klog::note(&format!("orbistoun: syscall {} - {called}", asked.number));
        }
        let kept = sequence.made.len() as u64;
        if sequence.total > kept {
            let _ = writeln!(
                err,
                "  and {} more, past what this run records in order",
                sequence.total - kept
            );
        }
        let _ = err.flush();
    }

    for (number, name) in orbistoun_thunk::syscall::syscalls_asked_for() {
        let mut err = std::io::stderr();
        let _ = match name {
            Some(name) => writeln!(
                err,
                "orbistoun: the guest asked the kernel for call {number} directly, which is {name}"
            ),
            None => writeln!(
                err,
                "orbistoun: the guest asked the kernel for call {number} directly, and nothing here implements it"
            ),
        };
        let _ = err.flush();
    }
}

#[cfg(windows)]
mod imp {
    use super::emit;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    /// Let the exception carry on to whatever would otherwise have handled it.
    ///
    /// This reports and gets out of the way: swallowing the fault would leave a guest
    /// running with corrupt state, which is far worse than stopping.
    const CONTINUE_SEARCH: i32 = 0;

    /// The access violation code, which is the one that matters here.
    const ACCESS_VIOLATION: i32 = 0xC000_0005_u32 as i32;
    /// Executing something that is not code.
    const ILLEGAL_INSTRUCTION: i32 = 0xC000_001D_u32 as i32;
    /// Reaching stub padding.
    const BREAKPOINT: i32 = 0x8000_0003_u32 as i32;
    /// Running past the guard page.
    const STACK_OVERFLOW: i32 = 0xC000_00FD_u32 as i32;
    /// A debug exception - which, with a debug register armed, is a watched access.
    const SINGLE_STEP: i32 = 0x8000_0004_u32 as i32;

    /// Resume the interrupted thread with the context as it now stands.
    ///
    /// Correct here and wrong for every other exception this handler sees: a watched access
    /// has **already happened** and nothing is broken, whereas swallowing an access violation
    /// would leave a guest running on corrupt state.
    const CONTINUE_EXECUTION: i32 = -1;

    /// What the first parameter of an access violation means.
    const ACCESS_WAS_WRITE: usize = 1;
    /// A fetch from a page with no execute permission.
    const ACCESS_WAS_EXECUTE: usize = 8;

    #[allow(
        clippy::too_many_lines,
        reason = "one linear exception dispatcher: watchpoint, TLS backstop, interrupt service, then the fault report - each a labelled block, and splitting the register copies into helpers would scatter one context read/write across functions"
    )]
    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // Read one field per block, as the lints require. Verbose, but each read is a
        // separate dereference of a pointer the operating system owns, and stating that
        // once per block is the discipline that keeps it checkable.

        // SAFETY: a vectored handler is passed a valid, fully populated structure that
        // stays live for the duration of the call.
        let record = unsafe { (*info).ExceptionRecord };
        // SAFETY: as above; the context record is populated for every exception.
        let context = unsafe { (*info).ContextRecord };
        // SAFETY: `record` came from the structure above and is live for this call.
        let code = unsafe { (*record).ExceptionCode };
        // SAFETY: same record. The array is fixed-size and always present.
        let parameters = unsafe { (*record).ExceptionInformation };
        // SAFETY: same record; says how many of `parameters` are meaningful.
        let count = unsafe { (*record).NumberParameters };
        // SAFETY: `context` came from the structure above and is live for this call.
        let rip = unsafe { (*context).Rip };
        // SAFETY: same context record, read once as a whole rather than field by field.
        // One dereference, one copy: `CONTEXT` is `Copy`, so this is the read the
        // per-field version was doing sixteen times.
        let ctx = unsafe { *context };

        let registers = super::Registers {
            rax: ctx.Rax,
            rbx: ctx.Rbx,
            rcx: ctx.Rcx,
            rdx: ctx.Rdx,
            rsi: ctx.Rsi,
            rdi: ctx.Rdi,
            rbp: ctx.Rbp,
            rsp: ctx.Rsp,
            r8: ctx.R8,
            r9: ctx.R9,
            r10: ctx.R10,
            r11: ctx.R11,
            r12: ctx.R12,
            r13: ctx.R13,
            r14: ctx.R14,
            r15: ctx.R15,
        };

        // Taken before anything else, because a watched access is not a failure and must not
        // be reported as one. The debug-status register says which watchpoints fired; it is
        // cleared before resuming so the next trap is not read as this one repeating.
        if code == SINGLE_STEP {
            match crate::watchpoint::note(ctx.Dr6, rip, &registers) {
                crate::watchpoint::Trap::NotOurs => return CONTINUE_SEARCH,
                crate::watchpoint::Trap::Data => {
                    // SAFETY: the context record is the one the operating system passed in,
                    // live for this call, and this writes a single field it owns before resuming.
                    unsafe {
                        (*context).Dr6 = 0;
                    }
                    return CONTINUE_EXECUTION;
                }
                crate::watchpoint::Trap::Execute { disarm } => {
                    // An execute breakpoint is one-shot: clearing its enable bit lets the
                    // instruction it sits on run now instead of trapping again, without any
                    // resume-flag or single-step dance.
                    // SAFETY: the OS's context, live for this call; a field it owns.
                    unsafe {
                        (*context).Dr6 = 0;
                    }
                    // SAFETY: as above; clears the enable bits of the fired execute slots.
                    unsafe {
                        (*context).Dr7 &= !disarm;
                    }
                    return CONTINUE_EXECUTION;
                }
            }
        }

        // A guest thread-local access whose `fs` base the host reset out from under it (D433): put
        // the base back and let the instruction run again, rather than report a fault a running
        // guest on a base-preserving host would never see. Only for access violations - the base is
        // irrelevant to an illegal instruction or a breakpoint - and only when the base has actually
        // reverted to zero, so a genuine fault still reaches the report below. Sound because a zero
        // base sends every `fs:` access into the unmapped ±2 GiB around zero, so it faults here
        // rather than reading wrong data.
        if code == ACCESS_VIOLATION && crate::tls_backstop::restore_if_reverted() {
            return CONTINUE_EXECUTION;
        }

        // **A software interrupt with a registered handler is serviced, not reported.** A guest
        // reaching the kernel by `int n` raises a general-protection fault the host delivers as an
        // access violation (at `0xffff...`, D384), and orbistoun has no interrupt handling beyond
        // this point - so without this the trap falls through to the fault report and the process
        // ends, which is why `int 0x41` walled PPSA04263 (worklog 603, 608). When the vector has a
        // handler, run it against the guest's registers and resume past the instruction, the way the
        // kernel's own interrupt gate would. `orbistoun-kernel`'s table is empty until obSCEne
        // measures a vector's semantics, so today every `int` still falls through here - this is the
        // mechanism, in place and ready for the one-line registration that follows the measurement.
        if matches!(code, ACCESS_VIOLATION | ILLEGAL_INSTRUCTION) {
            let (opcode, len) = super::instruction_bytes(rip);
            let frame = orbistoun_kernel::interrupt::InterruptFrame {
                rax: ctx.Rax,
                rbx: ctx.Rbx,
                rcx: ctx.Rcx,
                rdx: ctx.Rdx,
                rsi: ctx.Rsi,
                rdi: ctx.Rdi,
                rbp: ctx.Rbp,
                rsp: ctx.Rsp,
                r8: ctx.R8,
                r9: ctx.R9,
                r10: ctx.R10,
                r11: ctx.R11,
                r12: ctx.R12,
                r13: ctx.R13,
                r14: ctx.R14,
                r15: ctx.R15,
                rip,
            };
            if let Some(resumed) = super::serviced_interrupt(&opcode[..len], frame) {
                // The register fields are updated on a **copy** of the context - which is safe,
                // ordinary struct assignment - and the whole record is written back in one store,
                // so the guest resumes with the handler's answers. Building the copy here rather
                // than seventeen writes through the raw pointer keeps the single unsafe operation
                // that the discipline wants (principle 4).
                let mut next = ctx;
                next.Rax = resumed.rax;
                next.Rbx = resumed.rbx;
                next.Rcx = resumed.rcx;
                next.Rdx = resumed.rdx;
                next.Rsi = resumed.rsi;
                next.Rdi = resumed.rdi;
                next.Rbp = resumed.rbp;
                next.Rsp = resumed.rsp;
                next.R8 = resumed.r8;
                next.R9 = resumed.r9;
                next.R10 = resumed.r10;
                next.R11 = resumed.r11;
                next.R12 = resumed.r12;
                next.R13 = resumed.r13;
                next.R14 = resumed.r14;
                next.R15 = resumed.r15;
                next.Rip = resumed.rip;
                // SAFETY: `context` is the operating system's live context record for this call;
                // writing the whole record it owns is what resuming a serviced interrupt is.
                unsafe {
                    *context = next;
                }
                return CONTINUE_EXECUTION;
            }
        }

        // An access violation reports what was attempted and where; the others carry no
        // parameters, so the faulting address is the instruction itself.
        // Taken from the two lists on `FaultSite` rather than written out again here.
        // A consumer has to be able to tell "the guest touched this address" from "this
        // is the instruction that faulted", and a second copy of these strings is a
        // second copy that drifts - after which the classification is silently wrong.
        let touched = orbistoun_report::trace::FaultSite::TOUCHED;
        let at_instruction = orbistoun_report::trace::FaultSite::AT_THE_INSTRUCTION;
        let (kind, at) = match code {
            ACCESS_VIOLATION if count >= 2 => {
                let what = match parameters[0] {
                    ACCESS_WAS_WRITE => touched[0],
                    ACCESS_WAS_EXECUTE => touched[2],
                    _ => touched[1],
                };
                (what, parameters[1] as u64)
            }
            ILLEGAL_INSTRUCTION => (at_instruction[0], rip),
            // The address decides which of the three breakpoint kinds this is. It used
            // to be `at_instruction[1]` unconditionally - the stub-padding one - which
            // asserted a cause nothing had checked (D576).
            BREAKPOINT => (super::breakpoint_kind(rip), rip),
            STACK_OVERFLOW => (at_instruction[2], rip),
            // Anything else is not ours to explain. Debuggers and language runtimes
            // raise exceptions routinely, and reporting those as guest faults would be
            // noise at best and misleading at worst.
            _ => return CONTINUE_SEARCH,
        };

        emit(kind, at, rip, registers);
        CONTINUE_SEARCH
    }

    pub(super) fn install() -> bool {
        // SAFETY: registering a handler is safe; the function pointer has the signature
        // the platform requires and remains valid for the life of the process.
        let registered = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
        !registered.is_null()
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented away from Windows yet.
    ///
    /// Reporting from inside a signal handler needs `ucontext` to reach the instruction
    /// pointer, which the crates in use here do not expose. Saying so plainly beats a
    /// handler that reports the fault address and silently omits the half that says
    /// *what was executing* - which is the more useful half (principle 3).
    pub(super) fn install() -> bool {
        false
    }
}

/// Names for stub indices, so a call trace says what the guest wanted.
///
/// Set before entering the guest. Empty is a legitimate state - a module with no
/// readable import table still runs - and the summary falls back to bare indices rather
/// than inventing names.
static IMPORT_LABELS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Where each implementation starts, sorted, so a fault inside one can be named.
///
/// **A function pointer is an address, and this project has a table of them.** The binary
/// carries no symbols on this toolchain, so an address in orbistoun's own code is opaque -
/// and once an implementation is complicated enough to fault inside itself, opaque is not
/// good enough (D380).
static OWN_CODE: std::sync::OnceLock<Vec<(u64, &'static str)>> = std::sync::OnceLock::new();

/// Publishes where each implementation starts.
///
/// Sorted here rather than at the fault, because a fault handler must not allocate or sort.
pub fn name_implementations(mut starts: Vec<(u64, &'static str)>) {
    starts.sort_unstable();
    let _ = OWN_CODE.set(starts);
}

/// The implementation an address falls in, and how far into it.
///
/// **Nearest preceding, and the distance is printed with it**, because that is what makes it
/// honest: a fault inside a library routine the compiler inlined or called - a copy, a
/// formatter - names the last implementation *before* it, which is a hint rather than a fact.
/// A small offset is a strong hint; a large one is visibly not a match.
#[cfg(windows)]
fn own_code_site(address: u64) -> Option<(&'static str, u64)> {
    let starts = OWN_CODE.get()?;
    let index = starts
        .partition_point(|(at, _)| *at <= address)
        .checked_sub(1)?;
    let (at, name) = starts.get(index)?;
    Some((name, address - at))
}

/// Records what each stub index stands for.
///
/// Indexed by dynamic symbol index, matching the stub table exactly. An entry that is
/// empty means the symbol is not an import - most of the table - and is never called.
pub fn name_imports(labels: Vec<String>) {
    let _ = IMPORT_LABELS.set(labels);
}

/// The label for a stub index, or `None` if nothing is known about it.
pub fn label_of(index: usize) -> Option<&'static str> {
    IMPORT_LABELS
        .get()
        .and_then(|l| l.get(index))
        .filter(|l| !l.is_empty())
        .map(String::as_str)
}

/// Where the call trace is written, if anywhere.
///
/// A trace that exists only on a terminal dies with the run that produced it, and the
/// run that produced it may have taken ten minutes. Persisting it is what turns a
/// session into a work list (D077).
static TRACE_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Why the guest stopped itself, once it has.
static STOPPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// How the guest's calls measured against the calling convention.
fn abi_report() -> AbiReport {
    let seen = orbistoun_thunk::abi_conformance();
    let (sequence, import, rsp) = match seen.first_misaligned {
        Some((sequence, index, rsp)) => (
            Some(sequence),
            label_of(index as usize).map(str::to_owned),
            Some(rsp),
        ),
        None => (None, None, None),
    };
    AbiReport {
        misaligned_calls: seen.misaligned_calls,
        first_misaligned_sequence: sequence,
        first_misaligned_import: import,
        first_misaligned_rsp: rsp,
    }
}

/// How many frames the walk reports before giving up.
///
/// Bounded because the chain is guest-controlled: a corrupt or hostile frame pointer can
/// form a cycle, and a fault handler that loops is a process that dies with nothing said.
pub const MAX_FRAMES: usize = 12;

/// Walks the frame-pointer chain from `rbp`.
///
/// # Why this is worth having and why it is not always right
///
/// A fault address says *where* the guest died. It does not say **who called it**, and at
/// the top of a function - which is where a null dereference usually lands - the
/// instruction pointer alone is nearly content-free. The chain of return addresses is the
/// difference between "faulted at image+0x43c4" and a path through the guest's own code
/// (D172).
///
/// It is a best effort, and the reason is worth stating: a compiler is free to omit the
/// frame pointer, and optimised code routinely does. A walk that produces nothing is that
/// case, not a failure - which is why an empty list is reported rather than an error.
///
/// **Every read is bounds-checked against the stack region** before it happens. This runs
/// inside a fault handler, on a thread that has already faulted once; a second fault here
/// would replace the report with silence.
#[cfg(windows)]
fn walk_frames(rbp: u64) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut current = rbp;

    for _ in 0..MAX_FRAMES {
        // Both the saved pointer and the return address must lie inside the stack, and a
        // frame must be aligned - anything else is a chain that has left the rails.
        if current == 0 || current % 8 != 0 || locate(current).is_none_or(|(r, _)| r != "stack") {
            break;
        }
        let Ok(at) = usize::try_from(current) else {
            break;
        };
        // SAFETY: `current` was just confirmed to lie inside the guest stack region this
        // process reserved and still holds, and to be eight-byte aligned. This is the
        // saved frame pointer a prologue pushed.
        let saved = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) };
        // SAFETY: the word above it, which is the return address the call placed there.
        // Inside the same validated frame, one word further up.
        let return_address =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at + 8)) };
        if return_address == 0 {
            break;
        }
        frames.push(Frame {
            return_address,
            frame_pointer: current,
        });
        // A chain must climb. Anything else is a cycle, and a fault handler that loops
        // dies with nothing said.
        if saved <= current {
            break;
        }
        current = saved;
    }
    frames
}

/// Names the import **this thread** was inside, when the fault was not in guest code.
///
/// An instruction pointer inside a region orbistoun placed is the guest running its own
/// code, and the import that got it there is history rather than cause. Outside every
/// placed region it is *our* code running on the guest's behalf - and then the import the
/// faulting thread is inside is the one that faulted.
///
/// # It asked the wrong thread
///
/// This read `last_call`, which answers with the newest call **any** thread made. The
/// sentence above says "the last import the guest entered", and for a single-threaded guest
/// that is the same thing. For a multithreaded one it is whatever another thread happened to
/// be doing, named in the one field that exists to point at a function to fix.
///
/// D616 found the identical mistake in the mapping record and gave the thunk a per-thread
/// answer. This is the place it mattered most and was fixed second (D621).
///
/// Allocation-free: the label is a `&'static str` out of the import table, because this
/// runs before the part of the report that is allowed to allocate.
#[cfg(windows)]
fn inside_import_name(instruction_pointer: u64) -> Option<&'static str> {
    if locate(instruction_pointer).is_some() {
        return None;
    }
    // The handler runs on the faulting thread, so the thread-local is that thread's.
    label_of(orbistoun_thunk::current_call()? as usize)
}

/// Records where to persist the trace, and which module it belongs to.
pub fn trace_to(path: std::path::PathBuf) {
    let _ = TRACE_PATH.set(path);
}

/// Every system call this run was asked for by number, with what is known about each.
///
/// The first argument comes from the ordered log rather than the seen-bitmap, because the
/// bitmap records only *that* a number was asked for. For a call nobody can name, that
/// argument is most of what there is to go on.
fn asked_syscalls() -> Vec<orbistoun_report::trace::AskedSyscall> {
    let ordered = orbistoun_thunk::syscall::syscalls_in_order();
    orbistoun_thunk::syscall::syscalls_asked_for()
        .into_iter()
        .map(|(number, name)| orbistoun_report::trace::AskedSyscall {
            number,
            name: name.map(str::to_owned),
            first_argument: ordered
                .made
                .iter()
                .find(|asked| asked.number == number)
                .map(|asked| asked.argument),
        })
        .collect()
}

/// Collects what the guest called, most-used first.
///
/// Shared by the time-limit path and the ordinary-return path, so a guest that stops on
/// its own is reported exactly as fully as one that had to be stopped.
pub fn collect_calls(module: &str, reached: &str) -> CallTrace {
    collect_with_fault(module, reached, None)
}

/// The report the AGC driver recorded for the last command buffer a guest submitted, summarised for
/// the run report - or `None` if no guest reached a submission, which is every run today (3861).
fn submission_summary() -> Option<SubmissionSummary> {
    orbistoun_gpu::agc_driver::last_submission_report().map(|report| SubmissionSummary {
        packets: report.packets,
        register_writes: report.register_writes,
        draws: report.draws,
        shaders_found: report.shaders_found,
        addresses_resolved: report.addresses_resolved,
        addresses_unresolved: report.addresses_unresolved,
    })
}

/// The head window of the flipped buffer to read, as `(address, len)`, or `None` when the
/// address is not inside any allocated region.
///
/// **Pure, so the bounds arithmetic can be made to fail in a test** (principle 8). A guest can
/// flip a buffer index registered with any address at all, so the range is checked against the
/// run's own allocated regions before a byte of it is read - the D101 boundary that keeps a
/// blind dereference off the report path, the same rule `-5bff` put on the submit path. The
/// window is capped, and never runs past the end of the region the address falls in.
fn flipped_window(address: u64, regions: &[(u64, u64, bool)], cap: u64) -> Option<(u64, usize)> {
    let room = regions
        .iter()
        .filter(|(_, _, allocated)| *allocated)
        .find(|(start, end, _)| address >= *start && address < *end)
        .map(|(_, end, _)| end - address)?;
    let len = usize::try_from(room.min(cap)).ok()?;
    (len > 0).then_some((address, len))
}

/// Whether the head of `address`, bounded to `regions` and capped at `cap`, holds a byte the
/// guest wrote (any non-zero one).
///
/// The read-and-decide behind [`flipped_frame_written`], split out so the whole of it - the
/// bounds check *and* the read - can be exercised on a real buffer in a test, with the regions
/// supplied rather than taken from the global map. False when the address is outside every
/// allocated region or the window is all zero.
fn frame_written_against(address: u64, regions: &[(u64, u64, bool)], cap: u64) -> bool {
    let Some((at, len)) = flipped_window(address, regions, cap) else {
        return false;
    };
    let Ok(at) = usize::try_from(at) else {
        return false;
    };
    // SAFETY: `flipped_window` returned this range only after finding it inside an allocated
    // region, and the caller guarantees those regions describe currently-mapped memory - in a
    // run the guest's own under the identity mapping (D014) with the map locked, in a test a
    // live host buffer. `len` was clamped to the region's end.
    unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), len) }
        .iter()
        .any(|&b| b != 0)
}

/// Whether the buffer the guest last flipped holds bytes it wrote - the framebuffer signal
/// behind `Reach::Presented` (9b1f).
///
/// The last-flipped buffer's guest address comes from the video crate (`-6a86`/`-420c`). Under
/// the identity mapping (D014) that address is a host pointer, but it is bounds-checked against
/// the run's allocated regions before it is read, and only a bounded head is read: the buffer's
/// true extent needs the attribute struct decoded, which is unmeasured, and an unwritten buffer
/// is the zero a fresh allocation holds throughout while a written one differs at its start, so
/// the head separates them. False when nothing flipped, when the address lies outside every
/// allocated region, or when the window is all zero - which is every corpus title today, none of
/// which writes a frame a reader can see without a renderer (36c0).
fn flipped_frame_written() -> bool {
    /// The head read of a flipped buffer, in bytes: one page - enough for a written frame to
    /// show at its start, small enough to read cheaply off the report path.
    const WINDOW: u64 = 4096;

    let Some((address, _attribute)) = orbistoun_video::last_flipped_buffer() else {
        return false;
    };
    let Ok(map) = orbistoun_kernel::direct::map().lock() else {
        return false;
    };
    // The regions are copied out so the read-and-decide is a function of data, and the lock is
    // held across it so a region it validated cannot be unmapped underneath the read.
    let regions: Vec<(u64, u64, bool)> = map
        .regions()
        .iter()
        .map(|r| (r.start, r.end, r.allocated))
        .collect();
    let written = frame_written_against(address, &regions, WINDOW);
    drop(map);
    written
}

/// Collects the trace, recording where the guest died.
pub fn collect_with_fault(module: &str, reached: &str, fault: Option<FaultSite>) -> CallTrace {
    // What the watched region became, printed here because this is the one point reached
    // on **both** endings - a fault and a time limit. A diagnostic that only reported when
    // the guest crashed would be silent for the title that never crashes (D223).
    let changed = crate::watch::changes();
    if !changed.is_empty() {
        use std::io::Write as _;
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "orbistoun: watched region:");
        for line in &changed {
            let _ = writeln!(err, "{line}");
        }
    }

    let mut counts: Vec<(usize, u64)> = orbistoun_thunk::call_counts()
        .into_iter()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .collect();
    counts.sort_unstable_by_key(|(_, calls)| std::cmp::Reverse(*calls));

    // Read once, beside the counts, and indexed by the same import index - so each row's inferred
    // signature comes from the same snapshot as its call count and cannot disagree with it.
    let shapes = orbistoun_thunk::arg_shapes();

    CallTrace {
        module: module.to_owned(),
        reached: reached.to_owned(),
        // Summed from the same snapshot the rows are drawn from, rather than read from
        // the global counter. The guest may still be running, so a separately-read total
        // disagrees with the rows beneath it - and a report that does not add up invites
        // the reader to distrust all of it.
        total_calls: counts.iter().map(|(_, n)| *n).sum(),
        distinct: counts.len(),
        // Asked of the port table rather than counted from the rows above, because a
        // submission a port refused is still a call to something implemented and the rows
        // cannot tell the two apart (D558).
        frames: orbistoun_video::flips_accepted(),
        // Whether the buffer that flip carried holds bytes the guest wrote - read back and
        // bounds-checked against the run's own regions, the signal that lifts `flipped` to
        // `presented` (9b1f). False for every corpus title today: none writes a frame a
        // reader can see without the renderer that 36c0 tracks.
        frame_written: flipped_frame_written(),
        // The first command buffer a guest handed to `sceAgcDriverSubmitDcb`, summarised (3861).
        submission: submission_summary(),
        // **Recorded, not just printed.** These used to reach stderr at the end of a run and
        // go no further, so a guest that talks to the kernel by number left nothing behind for
        // the work list to rank - and that is how every open-toolchain payload works (D401).
        syscalls: asked_syscalls(),
        tail: tail_of_recorded(),
        formats: {
            let seen = orbistoun_libc::format_stats();
            FormatReport {
                calls: seen.calls,
                refused: seen.refused,
                truncated: seen.truncated,
                first_fault: seen
                    .first_fault
                    .map(describe_format_fault)
                    .unwrap_or_default(),
            }
        },
        dumps: collected_dumps(),
        title_modules: TITLE_MODULES.get().cloned().unwrap_or_default(),
        forced_dumps: FORCED_DUMPS.get().cloned().unwrap_or_default(),
        quiet: QUIET.get().copied(),
        ended_by: ENDED_BY.get().cloned(),
        threads: thread_notes(),
        said: orbistoun_core::said::lines(),
        conditions: {
            // Merged at read time, because the conditions are recorded before the import
            // labels exist and resolving which import to plant into needs them (D218).
            let mut c = CONDITIONS.get().cloned().unwrap_or_default();
            if let Some(planted) = PLANTED.get() {
                // **With the counts, always.** A forced write that matched an import and
                // then refused every target is indistinguishable from one that landed and
                // changed nothing - and the second is a result while the first is a broken
                // experiment. Reporting the number is what tells them apart (D218).
                let (done, refused) = orbistoun_thunk::forced_write_counts();
                // The same argument for forced returns, which had the same hole: an
                // unmoved fault under a forced answer reads as "the return value is not
                // where that came from", and reads identically when nothing was ever
                // answered. Counted separately rather than folded into the plant counts,
                // because they are different mechanisms and a reader adding them together
                // would learn nothing (D230).
                let answered = orbistoun_thunk::forced_return_count();
                let mut counts = Vec::new();
                if done != 0 || refused != 0 {
                    counts.push(format!("{done} planted, {refused} refused"));
                }
                if answered != 0 {
                    counts.push(format!("{answered} answered"));
                }
                c.experiments = if counts.is_empty() {
                    planted.clone()
                } else {
                    format!("{planted} ({})", counts.join("; "))
                };

                // Asked for and applied zero times. Recorded rather than left to be
                // inferred from a count of nought buried in a conditions line, because
                // that is exactly what was there before and it was read straight past
                // (D241).
                if planted.contains("at *arg") && done == 0 {
                    c.did_nothing
                        .push("ORBISTOUN_WRITE planted nothing".to_owned());
                }
                if planted.contains("answers") && answered == 0 {
                    c.did_nothing
                        .push("ORBISTOUN_RETURN answered no call".to_owned());
                }
            }
            c
        },
        stopped: STOPPED.get().cloned(),
        abi: abi_report(),
        reads: {
            let seen = orbistoun_fs::open::read_stats();
            ReadReport {
                reads: seen.reads,
                short: seen.short,
                bytes: seen.bytes,
            }
        },
        calls: counts
            .iter()
            .map(|(index, calls)| CalledImport {
                index: *index,
                label: labelled(*index),
                calls: *calls,
                implemented: orbistoun_thunk::is_implemented(*index),
                shape: shapes
                    .get(*index)
                    .map(|s| orbistoun_thunk::describe_shape(s))
                    .unwrap_or_default(),
            })
            .collect(),
        fault,
    }
}

/// What to call a stub in a report.
///
/// **Names the slot when it cannot name the function.** A bare `unknown` is the one label
/// that cannot be looked into: two different unlabelled stubs read as the same thing, so a
/// trace showing both looks like one function called twice. The index is always available
/// and always distinguishes them (D366).
fn labelled(index: usize) -> String {
    label_of(index).map_or_else(|| format!("unknown#{index}"), str::to_owned)
}

/// The last calls the guest made, labelled.
///
/// Taken from the ring the dispatcher has always filled. It holds the *first*
/// [`orbistoun_thunk::MAX_RECORDED_CALLS`], and the ring is **circular**, so these are the last
/// calls the guest made however long it ran (D571).
///
/// It was not always: the ring used to fill once and stop, which made this the last 48 of the
/// *first* 8,192 - calls #8,144-#8,191 of runs making four hundred thousand and twelve million,
/// while the printer called them the ones before the fault. That is what D568 found and what
/// making the ring circular fixed.
fn tail_of_recorded() -> Vec<TracedCall> {
    let all = orbistoun_thunk::recorded_calls();
    let from = all.len().saturating_sub(TAIL_CALLS);
    all[from..]
        .iter()
        .map(|c| TracedCall {
            thread: c.thread,
            sequence: c.sequence,
            label: labelled(c.index as usize),
            args: c.args,
            from: c.from,
            returned: c.ret,
        })
        .collect()
}

/// Every guest thread, joined against the calls still in the recorded window.
///
/// The join is what makes either table useful: the thread registry knows names and handles, the
/// ring knows what was called and on which host thread, and only together can a report say *which
/// named thread* went quiet (D651).
fn thread_notes() -> Vec<orbistoun_report::trace::ThreadNote> {
    let recorded = orbistoun_thunk::recorded_calls();
    orbistoun_kernel::thread::all()
        .into_iter()
        .map(|record| {
            let newest = recorded
                .iter()
                .filter(|call| record.host != 0 && call.thread == record.host)
                .max_by_key(|call| call.sequence);
            orbistoun_report::trace::ThreadNote {
                handle: record.handle,
                name: record.name,
                finished: record.finished,
                last_call: newest.map(|call| labelled(call.index as usize)),
                last_sequence: newest.map(|call| call.sequence),
            }
        })
        .collect()
}

/// Writes a trace to wherever [`trace_to`] pointed, if anywhere.
///
/// The findings are derived on read rather than stored: they are a *conclusion* about a
/// trace, and a stored conclusion goes stale the moment the rules that drew it improve.
///
/// Failures are reported and swallowed: losing a trace is bad, and failing a run that
/// otherwise worked because a directory was missing is worse.
pub fn persist(trace: &CallTrace) {
    // Every path that ends a run comes through here, so this is the one place a watchpoint
    // summary is guaranteed to be reached - including a fault, which is the path it was
    // built for. Silent unless something was armed.
    crate::watchpoint::summarise();
    // Same argument, same place: this is the one path every ending run comes through, so
    // it is the only place a shell summary is guaranteed to be reached - including after a
    // fault, which is when somebody most wants to know what the guest was and was not told.
    crate::session::summarise();
    // A reservation the guest could not make is otherwise invisible: `map` answers `NoMemory`
    // and the guest faults far away, so the base and the reason never reach the report. Named
    // here, once, from the host side where a `klog` line is safe (worklog 284, D459-class).
    if let Some(failure) = orbistoun_mem::last_reserve_failure() {
        // The count as well as the last one: two runs can report an identical last failure and
        // still have failed a different number of reservations, which is the difference that
        // matters when they took different paths (D487).
        eprintln!(
            "orbistoun: {} reservation(s) failed, first at {:#x}, last: base={:#x} len={:#x} - {}",
            orbistoun_mem::reserve_failures(),
            orbistoun_mem::first_reserve_failure_base().unwrap_or(0),
            failure.base,
            failure.len,
            failure.reason
        );
    }
    // A diagnostic that intervened says what it did. Silence here means none was asked
    // for; a line reporting nothing fired means the run tested nothing, which is the one
    // conclusion that must never be mistaken for an elimination (D325).
    //
    // Here rather than beside the call summary, which only two endings reach - a timeout
    // and an exhausted budget. **A fault reached neither**, so every intervening diagnostic
    // was silent on the most common ending this project has, which is the ending it was
    // built to explain (D513).
    for fill in [
        orbistoun_kernel::direct_fill_summary(),
        orbistoun_libc::heap_fill_summary(),
        orbistoun_libc::heap_base_summary(),
        // Not a diagnostic - a gap report, and unconditional for that reason. Nothing
        // switched it on and nothing intervened; it says what the run did not do (D514).
        orbistoun_kernel::module_start_summary(),
        orbistoun_kernel::equeue_summary(),
    ]
    .into_iter()
    .flatten()
    {
        eprintln!("orbistoun: {fill}");
    }
    let Some(path) = TRACE_PATH.get() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(trace) {
        Ok(text) => {
            if let Err(e) = std::fs::write(path, text) {
                eprintln!(
                    "orbistoun: could not write the call trace to {}: {e}",
                    path.display()
                );
            }
        }
        Err(e) => eprintln!("orbistoun: could not serialise the call trace: {e}"),
    }
}

/// Exit status used when a guest outruns its time limit.
///
/// Deliberately not in the range the platform uses for faults, so the two can never be
/// confused: "still running when the clock ran out" and "died on a bad pointer" call for
/// completely different next steps.
pub const TIME_LIMIT_EXIT: i32 = 0x0B0E;

/// Exit status for a guest stopped by the call budget.
///
/// Distinct from [`TIME_LIMIT_EXIT`] on purpose. "Ran out of clock" and "made the number of
/// calls it was allowed" look identical in a summary and mean different things: the first
/// varies with the machine and the second does not (D238).
pub const CALL_BUDGET_EXIT: i32 = 0x0B0F;

/// Ends the process if the guest is still running after `seconds`.
///
/// Records that the guest stopped itself, and ends the process.
///
/// Installed as the handler the subsystem crates call, because they cannot call upwards -
/// this is the layer that knows a trace is being written and where it goes.
///
/// **The trace is persisted before exiting**, for the same reason the time limit persists
/// before it kills: a guest that gave up has still said what it wanted, and that is the
/// whole output of the run.
fn guest_stopped(reason: orbistoun_core::StopReason, code: u64) -> ! {
    use std::io::Write as _;

    // Recorded before the trace is collected, so the report says the guest stopped rather
    // than describing an absent fault as a time limit.
    let _ = STOPPED.set(reason.label().to_owned());
    // **The third way a run ends, and the reports were on the other two.** A server does not
    // fault and does not return: it runs until the time limit, which is how `klogsrv`,
    // `shsrv` and `zftpd` all stop. So the two reports that read a record after the guest has
    // stopped were installed on the fault path and the ordinary return, and never fired for
    // any of the payloads that actually work (D387).
    // **All three reporters, through the one function that names them.** This path called two
    // of them by hand and so was the site that missed `paths_opened` when it was added - the
    // third time a reporter has been wired into some of the ways a run can end and not all of
    // them (D387, worklog 425). Naming them here again is what made that possible.
    what_the_guest_asked_for();
    let module = MODULE.get().map_or("unknown", String::as_str);
    let trace = collect_calls(module, "Entered");
    persist(&trace);

    let mut line = Line::new();
    line.text("orbistoun: ").text(reason.label()).text(" (");
    line.hex(code);
    line.text(
        ")
",
    );
    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();

    std::process::exit(orbistoun_core::stop::EXIT_GUEST_STOPPED);
}

/// Reports what the guest managed first. A guest that hangs and is killed from outside
/// takes its call trace with it, which is the one thing the run was for - so the trace
/// is written from in here, where it is still reachable.
///
/// Runs on an ordinary thread rather than in a fault handler, so it may allocate.
pub fn install_stop_handler() {
    orbistoun_core::stop::on_guest_stop(guest_stopped);
}

/// What the guest was pointing at, described rather than dumped raw.
///
/// The address is named against a region because `stack+0x800c90` says something a bare
/// `0x600000800c90` does not - it says the guest handed over a local, which is what
/// distinguishes an out-parameter from a pointer into its own data.
fn collected_dumps() -> Vec<ArgumentDump> {
    // **The arena is registered here, at the one place its name is used.** Every other region
    // is known before the guest is entered; this one does not exist then and grows as the
    // guest maps, so it is read at collection time instead. Here rather than on each of the
    // ways a run can end, because that list is what a reporter keeps being wired into
    // incompletely (D579).
    if let Some((base, len)) = orbistoun_kernel::arena_extent() {
        describe_region(Region::Mappings, base, len);
    }
    orbistoun_thunk::argument_dumps()
        .into_iter()
        .map(|d| ArgumentDump {
            label: label_of(d.index as usize).unwrap_or("unknown").to_owned(),
            slot: d.slot,
            value: d.address,
            // Named against a region only when it pointed at one. A scalar has no region,
            // and inventing one for it would read as though it were an address (D198).
            //
            // An address-shaped value that no region covers gets said out loud instead of
            // rendering as a bare number, because those two look identical and mean
            // opposite things - one is a count, the other is a pointer that is wrong or a
            // region this run never declared (D217).
            //
            // **It says *published*, because that is what was checked.** The dump reads only
            // from spans something published as readable, and for months every mapping a
            // guest made at runtime was absent from that list - so a pointer into a perfectly
            // ordinary heap structure was reported as though the address were wrong. *Not
            // mapped* and *nobody told the dump about it* are different findings and the tool
            // can only establish the second, which is the distinction principle 3 exists for
            // (D579, D580).
            at: match d.pointing {
                orbistoun_thunk::Pointing::Mapped => locate(d.address).map_or_else(
                    || format!("{:#x}", d.address),
                    |(r, o)| format!("{r}+{o:#x}"),
                ),
                orbistoun_thunk::Pointing::Unreadable => describe_unreadable(d.address),
                orbistoun_thunk::Pointing::Scalar => String::new(),
            },
            bytes: if d.pointing.was_read() {
                d.bytes
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            },
            text: if d.pointing.was_read() {
                printable(&d.bytes)
            } else {
                String::new()
            },
        })
        .collect()
}

/// The bytes as text, when they read as text.
///
/// Returned empty rather than as escapes when they do not: a line of dots is noise, and
/// noise beside real hex makes the hex harder to read. A name or a path in here is often
/// the entire answer.
fn printable(bytes: &[u8]) -> String {
    let text: String = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| char::from(*b))
        .collect();
    if text.len() >= 3 && text.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        text
    } else {
        String::new()
    }
}

/// Says what a formatted write could not do, in words a report can print.
///
/// Named here rather than in the crate that produces it: the distinction a reader needs is
/// between "implement this" and "the value never arrived", and only a sentence carries it.
fn describe_format_fault(fault: orbistoun_libc::FormatFault) -> String {
    match fault {
        orbistoun_libc::FormatFault::Unsupported(c) => {
            format!("the %{c} conversion is not implemented")
        }
        orbistoun_libc::FormatFault::FloatingPoint(c) => format!(
            "%{c} takes its argument in a vector register, which the trampoline does not capture"
        ),
        orbistoun_libc::FormatFault::OutOfArguments => {
            "the format needed more arguments than arrive in registers".to_owned()
        }
    }
}

/// Records what this run is subject to, for every trace it produces.
///
/// Set once during setup by the code that knows the limit and the policy, which is the
/// same dependency inversion the stop handler uses: a trace is collected from a fault
/// handler that has no route back up to the configuration (D160).
pub fn record_conditions(conditions: Conditions) {
    let _ = CONDITIONS.set(conditions);
}

/// What the run is subject to. Empty until setup records it.
static CONDITIONS: std::sync::OnceLock<Conditions> = std::sync::OnceLock::new();

/// Every diagnostic in force, if any.
///
/// A second slot rather than a field set with the rest: the conditions are recorded before
/// the import labels exist, and deciding which import an experiment applies to needs them.
/// Merged when the trace is collected.
static PLANTED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records what the run was put under, for the run conditions.
pub fn note_experiments(what: String) {
    let _ = PLANTED.set(what);
}

/// Reports what the guest managed first, if it runs out of time.
///
/// **The sleep is a sampling loop now.** A thread that sleeps for the whole limit and then
/// collects can say the run lasted twenty seconds and nothing else; it cannot say the guest
/// stopped calling anything after two and a half of them, which is the difference between slow
/// and stuck and the only question worth asking of a run that hit the clock (D645).
pub fn start_time_limit(seconds: u64, module: String) {
    std::thread::spawn(move || {
        let quiet = wait_watching_for_silence(seconds);
        note_quiet(quiet);
        note_ended_by(orbistoun_report::trace::RAN_TO_LIMIT);
        let trace = collect_calls(&module, "Entered");
        // Persisted *before* the summary is printed and before the process ends. A
        // guest that had to be stopped is exactly the case where the trace matters
        // most, and exactly the case where nothing else will get a chance to save it.
        persist(&trace);
        summarise_calls(&format!("after {seconds}s"), &trace);
        std::process::exit(TIME_LIMIT_EXIT);
    });
}

/// How often the counters are read while a run is in flight.
///
/// **Cheap enough to ignore and coarse enough to be honest.** Two relaxed loads four times a
/// second cost the guest nothing measurable, and a quarter-second is well under any silence
/// worth reporting - [`Quiet::is_notable`] will not fire below a second.
const SAMPLE: std::time::Duration = std::time::Duration::from_millis(250);

/// Sleeps out the limit, watching for the guest to stop asking the host for anything.
///
/// Returns where the last activity was, not a verdict. The guest may be blocked or may be
/// computing; this says only when it last crossed into the host, and every place the result is
/// printed says the same.
fn wait_watching_for_silence(seconds: u64) -> Quiet {
    let started = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(seconds);
    let mut seen = activity();
    // Zero, not "unknown": a guest that never calls anything was silent for the whole run, and
    // that is a true and useful thing to report rather than a gap.
    let mut last = std::time::Duration::ZERO;
    loop {
        let elapsed = started.elapsed();
        let Some(left) = limit.checked_sub(elapsed) else {
            break;
        };
        std::thread::sleep(SAMPLE.min(left));
        let now = activity();
        if now != seen {
            seen = now;
            last = started.elapsed();
        }
    }
    let run = started.elapsed();
    Quiet {
        silent_ms: u64::try_from(run.saturating_sub(last).as_millis()).unwrap_or(u64::MAX),
        last_activity_ms: u64::try_from(last.as_millis()).unwrap_or(u64::MAX),
        run_ms: u64::try_from(run.as_millis()).unwrap_or(u64::MAX),
        sample_ms: u64::try_from(SAMPLE.as_millis()).unwrap_or(u64::MAX),
    }
}

/// Everything the guest has asked the host for, as one monotonic number.
///
/// Both halves, because either alone is silent about a whole class of guest: a title calls
/// imports and almost no syscalls, an open-toolchain payload builds a gadget and then calls
/// nothing but syscalls. Watching one of them would report the other as permanently quiet.
fn activity() -> u64 {
    orbistoun_thunk::total_calls().saturating_add(orbistoun_thunk::syscall::syscalls_made())
}

/// Records the silence for the trace to carry.
fn note_quiet(quiet: Quiet) {
    let _ = QUIET.set(quiet);
}

/// Records which limit is stopping this run, before the trace that reports it is collected.
///
/// Called from the branch that decided, which is the point: the clock and the budget leave the
/// process by different exit codes, but an exit code is something the *parent* reads and the
/// trace is written before it - so without this both endings reach the report as the same
/// absence and both are described as the clock (D238).
fn note_ended_by(reason: &str) {
    let _ = ENDED_BY.set(reason.to_owned());
}

/// Which limit stopped the run, once one has.
///
/// A fourth slot beside the conditions, the experiments and the silence, for the reason they are
/// all slots: the trace is collected with no route back up to the thread that knows (D160).
/// Empty for a run that ended on its own, so `None` means "no limit fired" rather than "nobody
/// said which".
static ENDED_BY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The silence, once something has measured it.
///
/// A third slot beside the conditions and the experiments, for the same reason they are slots:
/// the trace is collected from a handler with no route back up to the thread that measured it
/// (D160). Empty for every run the clock did not end, which is what makes `None` mean "nobody
/// looked" rather than "there was none".
static QUIET: std::sync::OnceLock<Quiet> = std::sync::OnceLock::new();

/// The module a budgeted run is executing, for the stop callback to name.
static BUDGET_MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// The budget in force, so the summary can say what was reached.
static BUDGET_CALLS: AtomicU64 = AtomicU64::new(0);

/// Stops the guest once it has made `budget` import calls.
///
/// **The deterministic half of the pair.** The wall-clock limit fixes the duration and lets
/// the call count vary, which is backwards: the count is what every verdict is read off,
/// and it moved 13% between identical runs. This fixes the count instead (D238).
///
/// Not a replacement for the clock. A guest that stops calling imports never reaches a
/// budget, so both are installed and either may fire.
pub fn start_call_budget(budget: u64, module: String) {
    let _ = BUDGET_MODULE.set(module);
    BUDGET_CALLS.store(budget, Ordering::Relaxed);
    orbistoun_thunk::install_call_budget(budget, on_budget_reached);
}

/// Writes the trace and ends the process, when the budget is reached.
///
/// A plain `fn` because it is installed as one: the dispatch path holds a function pointer
/// and cannot hold a closure without an allocation the call path must not make
/// (principle 9). What it would have captured lives in the two statics above.
fn on_budget_reached() {
    let module = BUDGET_MODULE.get().map_or("", String::as_str);
    note_ended_by(orbistoun_report::trace::SPENT_THE_BUDGET);
    let trace = collect_calls(module, "Entered");
    persist(&trace);
    // Not "after N calls": the count is already the second half of that line, and saying
    // it twice reads as two different numbers that happen to agree.
    summarise_calls("when its call budget ran out", &trace);
    std::process::exit(CALL_BUDGET_EXIT);
}

/// Writes what the guest called, most-used first.
///
/// The counts are the work list: implementing the top of this list is what moves a
/// guest further, and the order is not guessable from a static import dump.
fn summarise_calls(stopped: &str, trace: &CallTrace) {
    use std::io::Write as _;

    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "orbistoun: the guest was still running {stopped}; {} import calls across {} distinct imports",
        trace.total_calls, trace.distinct
    );
    // **Printed whether or not it is notable.** The number a reader most often wants from a
    // run that hit the clock is when it went quiet, and a line that appears only sometimes
    // teaches people to read its absence as "fine" rather than as "not measured" (D159).
    if let Some(quiet) = trace.quiet {
        let _ = writeln!(err, "orbistoun: it {}", quiet.describe());
    }
    for call in trace.calls.iter().take(MOST_CALLED_REPORTED) {
        // Integer tenths of a percent rather than floating point: the counts run into
        // the hundreds of millions, past the point where an `f64` holds them exactly,
        // and a share that does not quite add up invites distrust of the whole report.
        let tenths = call
            .calls
            .saturating_mul(1000)
            .checked_div(trace.total_calls)
            .unwrap_or(0);
        // Formatted as text, so the width below is a width. A precision on a string
        // truncates it - `{share:5.1}` silently turned "99.9" into "9".
        let share = format!("{}.{}", tenths / 10, tenths % 10);
        let _ = writeln!(
            err,
            "orbistoun:   {:>12} calls ({share:>5}%)  {}",
            call.calls, call.label
        );
    }
    let _ = err.flush();
}

/// How many of the most-called imports are listed when a limit expires.
const MOST_CALLED_REPORTED: usize = 20;

/// Installs the fault reporter, returning whether one is active.
///
/// `false` is not an error: it means this platform reports nothing beyond the exit
/// status, and a caller should say so rather than imply a fault report is coming.
pub fn install() -> bool {
    imp::install()
}

#[cfg(test)]
mod tests {
    use super::{Line, Region, describe_region, describe_unreadable, locate, published_envelope};
    // The fault-report helpers below are a Windows-only subsystem (see the `#[cfg(windows)]`
    // gating on their definitions), so their tests - and the imports those tests need - are
    // gated too, or the Linux/macOS build reports them as unresolved imports and unused symbols.
    #[cfg(windows)]
    use super::{
        is_null_pointer_fault, note_emulator_fault, note_instruction_shape,
        note_null_pointer_to_libc, serviced_interrupt,
    };

    /// **The flipped-buffer window is bounded to an allocated region, and refuses to leave it.**
    ///
    /// `flipped_window` is the bounds check that keeps `flipped_frame_written` from reading a
    /// guest address blind - the D101 boundary the presented rung rests on (9b1f). An address
    /// inside an allocated region yields a window clamped to the region's end; one past the end,
    /// in an unallocated region, or in no region at all, yields nothing to read.
    #[test]
    fn the_flipped_window_stays_inside_an_allocated_region() {
        // One allocated region [0x1000, 0x3000) beside one unallocated [0x4000, 0x5000).
        let regions = [(0x1000_u64, 0x3000_u64, true), (0x4000, 0x5000, false)];

        // Inside the allocated region, the window is capped by `cap`.
        assert_eq!(
            super::flipped_window(0x1000, &regions, 256),
            Some((0x1000, 256))
        );
        // Near the end it is clamped to the region's end, not `cap`: 0x3000 - 0x2f00 = 256.
        assert_eq!(
            super::flipped_window(0x2f00, &regions, 4096),
            Some((0x2f00, 256))
        );
        // The region's last byte still has exactly one byte of room.
        assert_eq!(
            super::flipped_window(0x2fff, &regions, 4096),
            Some((0x2fff, 1))
        );
        // The end itself is outside [start, end), so there is nothing to read.
        assert_eq!(super::flipped_window(0x3000, &regions, 4096), None);
        // An unallocated region is refused even though the address falls inside it.
        assert_eq!(super::flipped_window(0x4000, &regions, 4096), None);
        // An address in no region at all is refused.
        assert_eq!(super::flipped_window(0x9999, &regions, 4096), None);
    }

    /// **A written head reads back as written; a zero head, and an out-of-region address, do
    /// not.** The whole point of the presented rung is that a flip's buffer is *read back*, so
    /// this drives the read on a real buffer - the one thing that proves `frame_written` can be
    /// set at all, rather than being a field that is always false (9b1f, principle 3). A single
    /// non-zero byte in the head is enough; all-zero is the unwritten buffer every corpus title
    /// flips; and an address outside the region is never dereferenced.
    #[test]
    fn a_written_head_reads_back_as_written_and_a_zero_head_does_not() {
        let mut buf = [0_u8; 64];
        let address = buf.as_ptr() as u64;
        let regions = [(address, address + 64, true)];

        // All zero: nothing was written, so the buffer is not a presented frame.
        assert!(!super::frame_written_against(address, &regions, 4096));

        // One byte written into the head flips the answer - read back off the real buffer.
        buf[3] = 0xAB;
        let address = buf.as_ptr() as u64;
        let regions = [(address, address + 64, true)];
        assert!(super::frame_written_against(address, &regions, 4096));

        // An address past the region is refused without a read, even with a written buffer near.
        assert!(!super::frame_written_against(
            address + 1024,
            &regions,
            4096
        ));
    }

    /// **A null-ish fault in orbistoun's code is called the guest's libc pointer, not an emulator
    /// bug.** PPSA02664 faults at `read of 0xa8` inside `orbistoun_libc::memcpy` - the guest handed
    /// an unpopulated pointer to `memcpy`, which the "EMULATOR BUG" message sent a reader to debug
    /// (a correct `memcpy`) instead of the upstream guest state. The threshold decides it, and a real
    /// guest address (well above a page) still reads as an emulator fault, so a genuine emulator bug
    /// is not relabelled away.
    #[cfg(windows)] // exercises the Windows-only fault-report helpers
    #[test]
    fn a_null_ish_fault_in_own_code_is_the_guests_libc_pointer_not_an_emulator_bug() {
        // The classifier: null plus a field offset is the guest's pointer; a guest address is not.
        assert!(
            is_null_pointer_fault(0xa8),
            "read of 0xa8 is null plus an offset"
        );
        assert!(is_null_pointer_fault(0x0), "a bare null too");
        assert!(
            !is_null_pointer_fault(0x7400_01f8_f070),
            "a real guest address is not null-ish, so it stays an emulator fault"
        );

        // The two messages are distinct and neither borrows the other's verdict.
        let mut libc = Line::new();
        note_null_pointer_to_libc(&mut libc);
        let libc_text = String::from_utf8_lossy(libc.as_bytes()).into_owned();
        assert!(
            libc_text.contains("libc") && libc_text.contains("upstream"),
            "the libc-pointer note points upstream, not at orbistoun's logic: {libc_text}"
        );
        assert!(
            !libc_text.contains("EMULATOR BUG"),
            "and it does not carry the emulator-bug verdict: {libc_text}"
        );

        let mut emu = Line::new();
        note_emulator_fault(&mut emu);
        assert!(
            String::from_utf8_lossy(emu.as_bytes()).contains("EMULATOR BUG"),
            "a non-null fault in our code is still called an emulator bug"
        );
    }

    /// **The interrupt glue services an `int` with a handler, resumes past it, and answers - and
    /// declines everything else.** The `CONTEXT` copy is trivial; this pins the part that could be
    /// wrong: which instructions count, where execution resumes, and that an unhandled vector is
    /// left for the fault report (worklog 608).
    #[cfg(windows)] // exercises the Windows-only `serviced_interrupt` glue
    #[test]
    fn a_software_interrupt_with_a_handler_is_serviced_and_resumes_past_it() {
        use orbistoun_kernel::interrupt::{InterruptFrame, clear, install};

        fn answer_7(frame: &mut InterruptFrame) {
            frame.rax = 7;
        }
        const VECTOR: u8 = 0x7f;
        clear(VECTOR);
        install(VECTOR, answer_7);

        // `int 0x7f` at rip: serviced, rax answered, rip advanced two bytes past the trap.
        let at = 0x4000_0000_5000;
        let frame = InterruptFrame {
            rax: 0,
            rip: at,
            ..InterruptFrame::default()
        };
        let resumed = serviced_interrupt(&[0xcd, VECTOR], frame).expect("a handled interrupt");
        assert_eq!(resumed.rax, 7, "the handler answered into rax");
        assert_eq!(
            resumed.rip,
            at + 2,
            "execution resumes past the two-byte int"
        );
        clear(VECTOR);

        // Same instruction, no handler: not serviced, so the fault report gets it.
        assert!(
            serviced_interrupt(&[0xcd, VECTOR], InterruptFrame::default()).is_none(),
            "an int with no handler is left for the report, not silently swallowed"
        );
        // An ordinary instruction is never mistaken for an interrupt.
        assert!(
            serviced_interrupt(&[0x48, 0x8b, 0x00], InterruptFrame::default()).is_none(),
            "a mov is not an interrupt"
        );
        // int 0x41 specifically has no handler - it is measured fatal, so there is nothing to
        // register - and so it is left for the report, which frames it as the trap it is.
        assert!(
            serviced_interrupt(&[0xcd, 0x41], InterruptFrame::default()).is_none(),
            "int 0x41 is measured fatal, so it is never serviced - it falls through to the report"
        );
    }

    /// **A software interrupt names its own vector and its own category, deterministically.**
    ///
    /// This is the report answering the question instead of a person decoding the bytes by hand.
    /// `int 0x41` walled a commercial title for an afternoon because the report said a bare "int"
    /// and framed it as "orbistoun does not intercept ... no import to blame" - which read as a
    /// dead end and sent the investigation after a filesystem path (worklog 603).
    ///
    /// **The categories now diverge on the one vector that was measured, and this pins it.** obSCEne
    /// measured `int 0x41` fatal on hardware (REQ-...b3c2): it is a guest trap whose cause is
    /// upstream, framed as a trap, *not* as an unimplemented kernel entry to characterise. Every
    /// other vector is still unmeasured and keeps the "KERNEL ENTRY, this is orbistoun's gap"
    /// framing - because a vector nobody has measured really might be a service to add. A `ud2` is
    /// checked to be a guest trap too, because conflating the two is how "orbistoun's gap" and "the
    /// guest aborted" get mistaken for each other.
    #[cfg(windows)] // exercises the Windows-only `note_instruction_shape` helper
    #[test]
    fn a_software_interrupt_names_its_vector_and_its_measured_category() {
        // int 0x41: measured fatal, so a guest trap pointing upstream - not an entry to characterise.
        let mut fatal = Line::new();
        note_instruction_shape(&mut fatal, &[0xcd, 0x41], 0xffff_ffff_ffff_ffff);
        let fatal_text = String::from_utf8_lossy(fatal.as_bytes());
        assert!(
            fatal_text.contains("int 0x41") && fatal_text.contains("measured fatal"),
            "the vector is named and said to be measured fatal, not awaiting a handler: {fatal_text}"
        );
        assert!(
            fatal_text.contains("upstream") && !fatal_text.contains("characterise"),
            "a measured-fatal trap points upstream, not at another measurement: {fatal_text}"
        );

        // int 0x42: unmeasured, so still a kernel entry that is orbistoun's gap to characterise.
        let mut entry = Line::new();
        note_instruction_shape(&mut entry, &[0xcd, 0x42], 0xffff_ffff_ffff_ffff);
        let entry_text = String::from_utf8_lossy(entry.as_bytes());
        assert!(
            entry_text.contains("int 0x42")
                && entry_text.contains("KERNEL ENTRY, UNIMPLEMENTED")
                && entry_text.contains("orbistoun's gap"),
            "an unmeasured vector is orbistoun's gap and must be categorised as one: {entry_text}"
        );

        // A ud2 is the guest trapping itself, a different category with a different next step.
        let mut trap = Line::new();
        note_instruction_shape(&mut trap, &[0x0f, 0x0b], 0);
        let trap_text = String::from_utf8_lossy(trap.as_bytes());
        assert!(
            trap_text.contains("GUEST TRAP") && trap_text.contains("upstream"),
            "a self-raised trap points upstream, not at an unimplemented entry: {trap_text}"
        );

        // An ordinary instruction says nothing - the note must not fire on a normal fault.
        let mut ordinary = Line::new();
        note_instruction_shape(&mut ordinary, &[0x48, 0x8b, 0x00], 0x1234);
        assert!(
            !String::from_utf8_lossy(ordinary.as_bytes()).contains("KERNEL ENTRY"),
            "a mov is not a kernel entry"
        );
    }

    #[test]
    fn an_address_inside_a_registered_region_is_named_with_its_offset() {
        // The difference between one bit of information and a work list.
        describe_region(Region::Image, 0x4000_0000_0000, 0x10_0000);
        assert_eq!(locate(0x4000_0000_1234), Some(("image", 0x1234)));
    }

    /// An address outside every region is said to be outside; one merely undeclared is not.
    ///
    /// **Both halves, because the useful sentence is the one that does not always fire.** A
    /// description that appended "outside every region" unconditionally would be true of
    /// nothing in particular and would read as though it had checked (D615).
    ///
    /// These share the process-wide region table with every other test in this module, so the
    /// regions are described here rather than assumed - the envelope is whatever the whole file
    /// has registered, and only its extremes matter.
    #[test]
    fn an_address_beyond_the_envelope_is_distinguished_from_one_merely_undeclared() {
        describe_region(Region::Image, 0x4000_0000_0000, 0x10_0000);
        describe_region(Region::Stack, 0x6000_0000_0000, 0x10_0000);
        let (low, high) = published_envelope().expect("regions were described");
        assert!(low <= 0x4000_0000_0000 && high >= 0x6000_0010_0000);

        // A host address, above everything a guest was given.
        let beyond = describe_unreadable(0x7ff7_620b_cab0);
        assert!(
            beyond.contains("outside every region this run gave the guest"),
            "an address past the envelope must say so: {beyond}"
        );

        // Inside the envelope and in no region: undeclared, which is a different finding and
        // must not borrow the stronger sentence.
        let between = describe_unreadable(0x5000_0000_0000);
        assert!(
            !between.contains("outside every region"),
            "an address between two regions is undeclared, not outside: {between}"
        );
        assert!(
            between.contains("address-shaped"),
            "and still says that much"
        );
    }

    #[test]
    fn an_address_outside_every_region_is_left_unnamed_rather_than_guessed() {
        // Attributing an unmapped address to the nearest region would point a reader
        // at code that had nothing to do with it.
        describe_region(Region::Stubs, 0x7000_0000_0000, 0x1000);
        assert_eq!(locate(0x1234), None);
    }

    #[test]
    fn the_end_of_a_region_is_outside_it() {
        // Off-by-one here would name the first byte of whatever follows.
        describe_region(Region::Stack, 0x6000_0000_0000, 0x1000);
        assert_eq!(locate(0x6000_0000_0FFF), Some(("stack", 0xFFF)));
        assert_eq!(locate(0x6000_0000_1000), None);
    }

    /// The arena has a name, so a pointer into it stops reading like a count.
    ///
    /// **The slot it occupies used to be called `other` and nothing ever registered it**, so
    /// every address a guest mapped at runtime - which is where an allocator puts the
    /// structures a call is handed - fell through `locate` and printed as a bare number
    /// (D579). Registered late in the run rather than before it, because it does not exist
    /// at entry.
    #[test]
    fn an_address_in_the_mapping_arena_is_named() {
        describe_region(Region::Mappings, 0x7400_0000_0000, 0x100_0000);
        assert_eq!(
            locate(0x7400_0089_D210),
            Some(("guest mappings", 0x89_D210))
        );
        assert_eq!(
            locate(0x7400_0100_0000),
            None,
            "past how far the guest reached, so not the arena's to name"
        );
    }

    #[test]
    fn hexadecimal_is_formatted_without_allocating() {
        // Hand-rolled because a handler may run while the allocator lock is held by the
        // code that just crashed.
        let mut line = Line::new();
        line.hex(0);
        assert_eq!(line.as_bytes(), b"0x0");

        let mut line = Line::new();
        line.hex(0xDEAD_BEEF);
        assert_eq!(line.as_bytes(), b"0xdeadbeef");

        let mut line = Line::new();
        line.hex(u64::MAX);
        assert_eq!(line.as_bytes(), b"0xffffffffffffffff");
    }

    #[test]
    fn a_line_truncates_rather_than_overflowing_its_buffer() {
        // The buffer is fixed and the handler cannot grow it; losing the tail of a
        // message is survivable, writing past it is not.
        let mut line = Line::new();
        for _ in 0..200 {
            line.text("some text that is long enough to overrun");
        }
        assert_eq!(line.as_bytes().len(), Line::CAPACITY);
    }
}

/// Lists the run's opening calls, in order, with where each was made from.
///
/// **The head of the sequence, where the trace keeps the tail.** The tail exists for the fault;
/// this exists for the question two runs raise when they diverge - and the position *is* the call
/// ordinal, so it lines up with the mapping record without anybody counting (D603).
pub(crate) fn opening_calls() {
    use std::io::Write as _;

    if !orbistoun_env::TRACE_CALLS.is_set() {
        return;
    }
    let calls = orbistoun_thunk::opening_sequence();
    let mut err = std::io::stderr();
    if calls.is_empty() {
        let _ = writeln!(
            err,
            "orbistoun: the guest made no call at all, which is a finding rather than an empty list"
        );
        let _ = err.flush();
        return;
    }
    let _ = writeln!(err, "orbistoun: its opening {} call(s):", calls.len());
    for (sequence, index, from) in &calls {
        let _ = writeln!(
            err,
            "  call {sequence:<6} {:<52} from {from:#x}",
            label_of(*index as usize).unwrap_or("unknown")
        );
    }
    let _ = err.flush();
}
#[cfg(test)]
mod pointee_tests {
    // `printable_text` is part of the Windows-only fault reporter, so its import and the test
    // that uses it are gated; the breakpoint tests below are cross-platform and stay ungated.
    #[cfg(windows)]
    use super::printable_text;

    /// A terminated run of printable characters is text; everything else is bytes.
    ///
    /// # What this protects
    ///
    /// A fault dump is read closely and believed, so a quoted string in one has to *be* a
    /// string. Two of these cases came out of the run this was built for: `"None"` is the
    /// label a guest passed, and `65 4e f1 22 ...` is the start of a pointer that begins with
    /// two characters and would read as `"eN"` under a looser rule (D522).
    ///
    /// **What it cannot check:** that the guest meant the bytes as text. A four-byte integer
    /// whose bytes all happen to be printable and whose fifth byte is zero is indistinguishable
    /// from a short string, and this reports it as one.
    #[cfg(windows)] // exercises the Windows-only `printable_text` helper
    #[test]
    fn only_terminated_printable_bytes_are_reported_as_text() {
        let mut label = [0_u8; 16];
        label[..4].copy_from_slice(b"None");
        assert_eq!(printable_text(&label).as_deref(), Some("None"));

        assert_eq!(
            printable_text(&[0x65, 0x4e, 0xf1, 0x22, 0xff, 0xee, 0x1f, 0x3c]),
            None,
            "a pointer beginning with two printable bytes is not a string"
        );
        assert_eq!(
            printable_text(&[0; 8]),
            None,
            "an empty run is not a string - every zeroed buffer would otherwise quote one"
        );
        assert_eq!(
            printable_text(b"noterminator"),
            None,
            concat!(
                "an unterminated run is the start of something longer, and quoting it ",
                "would show a fragment as though it were the whole"
            )
        );
        assert_eq!(
            printable_text(b"has	tab ......"),
            None,
            "a control character is not printable, so the run is bytes"
        );
    }

    #[test]
    fn a_breakpoint_is_only_called_stub_padding_where_the_stubs_are() {
        use orbistoun_report::trace::FaultSite;

        // The case the old message assumed for every breakpoint: inside the table.
        assert_eq!(
            super::breakpoint_kind_in(0x1_0040, 0x1_0000, 0x1000),
            FaultSite::BREAKPOINT_IN_STUBS
        );
        // The case that made this necessary. PPSA03416 trapped at an address the region
        // table itself named as the title's own modules, and the report called it stub
        // padding in the same line (D576). Below the table and above it, because an
        // off-by-one on either edge would put that message back.
        assert_eq!(
            super::breakpoint_kind_in(0x0_FFFF, 0x1_0000, 0x1000),
            FaultSite::BREAKPOINT_OUTSIDE_STUBS
        );
        assert_eq!(
            super::breakpoint_kind_in(0x1_1000, 0x1_0000, 0x1000),
            FaultSite::BREAKPOINT_OUTSIDE_STUBS,
            "one past the end is outside"
        );
        assert_eq!(
            super::breakpoint_kind_in(0x1_0FFF, 0x1_0000, 0x1000),
            FaultSite::BREAKPOINT_IN_STUBS,
            "the last byte is inside"
        );
        // No table registered: the question could not be asked, and saying "not stub
        // padding" would be the same overstatement one level down.
        assert_eq!(
            super::breakpoint_kind_in(0x1_0040, 0, 0),
            FaultSite::BREAKPOINT_UNPLACED
        );
    }

    #[test]
    fn every_breakpoint_kind_is_listed_as_an_instruction_address() {
        // The three are read by consumers deciding whether the address is somewhere the
        // guest touched. A kind missing from that list is one a consumer classifies by
        // falling through, which is how a fault kind ends up silently wrong.
        use orbistoun_report::trace::FaultSite;
        for kind in [
            FaultSite::BREAKPOINT_IN_STUBS,
            FaultSite::BREAKPOINT_OUTSIDE_STUBS,
            FaultSite::BREAKPOINT_UNPLACED,
        ] {
            assert!(
                FaultSite::AT_THE_INSTRUCTION.contains(&kind),
                "{kind} is not listed as an instruction address"
            );
            assert!(
                !FaultSite::TOUCHED.contains(&kind),
                "{kind} is not an address the guest asked for"
            );
        }
    }
}

/// Says so when the tables an import index is used against are not the same length.
///
/// **One index, several tables.** A stub slot, a label, a call counter and a binding are all keyed
/// by the same number, and they are built in different places from different counts. When two of
/// them disagree an index means different things depending on which table it is used against - so
/// a call is attributed to the wrong import, and every number about it is confidently wrong
/// (D490, D640).
///
/// Printed only when they disagree, because agreeing is the ordinary state and a line that fires
/// every run is one people learn to skip.
pub(crate) fn tables_disagree() {
    let labels = IMPORT_LABELS.get().map_or(0, Vec::len);
    let counters = orbistoun_thunk::call_counts().len();
    if labels == counters {
        return;
    }
    eprintln!(
        concat!(
            "orbistoun: the tables an import index is used against are different lengths - ",
            "{} label(s), {} call counter(s)"
        ),
        labels, counters
    );
    eprintln!(
        "orbistoun:   an index past the shortest of those names one import and counts another"
    );
}
/// Says so when a bound import was called through a stub anyway.
///
/// **A contradiction between two things this run believes.** An import bound into a placed module
/// is called inside that module; orbistoun never sees it. So a bound import with stub calls cannot
/// happen - and PPSA25872 shows one, nineteen million times, while the binding account says it was
/// bound. Nothing compared the two until this line, and the wall sat there for a month reading as
/// a missing implementation (D640).
pub(crate) fn bound_yet_called() {
    let offenders = bound_but_called();
    if offenders.is_empty() {
        return;
    }
    let total: u64 = offenders.iter().map(|(_, calls)| *calls).sum();
    eprintln!(
        concat!(
            "orbistoun: {} import(s) were bound into a module this title ships and were called ",
            "through a stub anyway, {} time(s) - the binding did not reach the relocation"
        ),
        offenders.len(),
        total
    );
    for (label, calls) in offenders.iter().take(6) {
        eprintln!("orbistoun:   {label}, {calls} call(s)");
    }
}

#[cfg(all(test, windows))]
mod host_module_tests {
    use super::host_module_of;

    /// **An address inside this very binary is named as it, at a stable offset.**
    ///
    /// The whole reason `host_module_of` exists is that a bare host address is not reproducible
    /// across reboots, so the test has to check the two properties that buy the reproducibility:
    /// that a *name* comes back, and that the offset is the distance from the module's own base
    /// rather than anything absolute.
    ///
    /// Uses this test function's own address, because it is the one host address whose module is
    /// known without asking the platform a second time.
    #[test]
    fn an_address_in_this_module_is_named_and_offset_from_its_base() {
        let here = an_address_in_this_module_is_named_and_offset_from_its_base as *const () as usize
            as u64;
        let (name, offset) = host_module_of(here).expect("this binary is a mapped image");

        assert!(
            !name.is_empty() && !name.contains('\\') && !name.contains('/'),
            "the file name and never the path - an absolute path must not reach a record: {name}"
        );
        assert!(
            offset > 0 && offset < here,
            "an offset into the module, not the address itself: {offset:#x} of {here:#x}"
        );
        // The property that makes it worth recording: reconstructing the address from the
        // answer gives back the address, so `name+offset` names this place and no other.
        let (_, again) = host_module_of(here).expect("stable across calls");
        assert_eq!(offset, again, "the same address answers the same offset");

        // A second address in the same module differs by exactly the distance between them,
        // which is what a reader comparing two recorded faults relies on.
        let other = host_module_of as *const () as usize as u64;
        let (other_name, other_offset) = host_module_of(other).expect("also in this binary");
        assert_eq!(other_name, name, "both are in this binary");
        assert_eq!(
            other_offset.abs_diff(offset),
            other.abs_diff(here),
            "offsets keep the distance the addresses had"
        );
    }

    /// **Nothing is invented for an address that is in no module.**
    ///
    /// The negative half, and the one that matters: answering a name for an unmapped address
    /// would put a fabricated location into a record that reads as a measurement.
    #[test]
    fn an_address_in_no_module_is_not_named() {
        assert_eq!(host_module_of(0), None, "the null page is in no module");
        assert_eq!(
            host_module_of(0x0000_7f00_0000_0000),
            None,
            "an address in no mapping has no module to name"
        );
    }
}
