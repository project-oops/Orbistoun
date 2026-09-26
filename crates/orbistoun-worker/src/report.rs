//! Saying where a guest faulted, from inside the fault.
//!
//! "Read of `0x0` while executing at image+0x1a4c20" says whether the guest dereferenced a null
//! thread pointer, ran off the end of a stub, or jumped somewhere never mapped. The report goes to
//! the error stream, not the newline-delimited JSON protocol stream, where a process dying
//! mid-write would leave a half line that breaks the reader; the parent produces its own structured
//! verdict.
//!
//! A handler runs on a thread that has just faulted, on the guest stack, where allocating risks
//! deadlocking against the code that crashed. So the first message is assembled in a fixed buffer
//! and issued as a single write.

use core::sync::atomic::{AtomicU64, Ordering};

// The shapes this fills in live one layer down, in `orbistoun-report`, so the service layer and
// both shims can see them. Only the producing side is here.
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

/// Names, indexed the same way. Fixed rather than stored, so the handler never chases a pointer the
/// faulting code may have invalidated.
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
    /// One span rather than one per module, because slots are few. The span includes the unmapped
    /// guards between modules, so an address in a guard is named as a module, which overstates by a
    /// granule rather than naming it as orbistoun's own code (D489).
    TitleModules,
    /// The arena guest-requested mappings are placed in, as far as it has been used.
    ///
    /// Registered when the guest stops rather than before it starts, unlike every other region
    /// here, because it does not exist at entry and grows as the guest maps. That serves an
    /// argument dump, collected after the guest stops, but not the fault handler: a run killed from
    /// outside names a faulting address in the arena as a bare number.
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
/// Call before entering the guest.
pub fn describe_region(region: Region, base: u64, len: u64) {
    REGION_BASE[region.slot()].store(base, Ordering::Relaxed);
    REGION_LEN[region.slot()].store(len, Ordering::Relaxed);
}

/// Names the region containing `address`, and the offset into it.
///
/// Pure and so testable: the handler that uses it cannot be stepped through.
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
/// An import bound to a placed module is called inside that module, so orbistoun never counts a
/// call against its stub. A bound import with calls on its stub is a contradiction between the
/// binding account and the call counts, which the report compares (D640).
pub fn name_bound_imports(indices: std::collections::BTreeSet<usize>) {
    let _ = BOUND_TO_MODULES.set(indices);
}

/// Bound imports that were nevertheless called through a stub, with their call counts.
///
/// Empty is the correct and usual answer; a non-empty list names a contradiction between two things
/// this run believes.
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
/// A hash from one of these is the title's own symbol, which no vendor vocabulary holds, so the
/// report does not advise extending one.
pub fn name_title_modules(libraries: Vec<String>) {
    let _ = TITLE_MODULES.set(libraries);
}

/// Functions the guest module names in its own symbol table, sorted by address.
///
/// `(offset into the image, extent, name)`. Empty for a stripped module, as commercial titles are;
/// open-toolchain guests carry names.
static GUEST_CODE: std::sync::OnceLock<Vec<(u64, u64, String)>> = std::sync::OnceLock::new();

/// Records what a guest module calls its own functions, for naming an address in it.
///
/// Called once, before entry. Sorted here rather than at lookup, so the fault path stays
/// allocation-free: a binary search over a slice and a `&str` it does not own.
pub fn name_guest_functions(mut functions: Vec<(u64, u64, String)>) {
    functions.sort_unstable_by_key(|(at, _, _)| *at);
    let _ = GUEST_CODE.set(functions);
}

/// The guest function an image offset falls in, and how far into it.
///
/// Only when the symbol covers the offset: where the producer recorded an extent, an offset past
/// the end of the nearest preceding function is in a gap or something unnamed. Where the extent is
/// zero, the nearest preceding name is offered with the distance beside it, the same hint
/// `own_code_site` gives.
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
/// An address-shaped value no region covers is either a pointer into something this run never
/// declared or a register the call never set: a call of arity three leaves the other captured
/// registers holding whatever the caller last put there. A value outside this envelope is not a
/// guest pointer this run failed to declare. That is a fact about where the value sits, not proof
/// of a stale register: a guest can compute a wild pointer, and `orbistoun-abi` hands out host
/// addresses in two places.
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
/// Split from the rendering so the sentence is testable without a run.
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
/// Pure and split from the lookup, as [`locate`] is. Three answers, because three things can be
/// known: the address is inside the stub table, it is outside a table whose span is known, or no
/// span was registered and the question cannot be asked.
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
/// The effectful half: it reads the two atomics [`describe_region`] filled and hands them to the
/// decision above.
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
/// No allocation and no locks, because a handler may run while the code that just crashed holds the
/// allocator lock.
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
    /// Room for the faulting instruction's bytes and the window before it (about 200 characters of
    /// hex) with the register and frame lines after them. A stack buffer in a fault handler, so the
    /// room is nearly free.
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
    /// Hand-rolled because the formatting machinery allocates, and this runs where allocating may
    /// deadlock.
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

    /// Appends one byte as two hexadecimal digits, unprefixed, for a run of raw bytes. Hand-rolled
    /// for the same reason [`Self::hex`] is.
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
            // And the guest's own name for it, where the module carries one: a function name is a
            // place to start reading. Only the image, because the other regions are orbistoun's and
            // already named.
            if name == REGION_NAMES[Region::Image as usize]
                && let Some((symbol, into)) = guest_code_site(offset)
            {
                self.text(" ").text(symbol).text("+").hex(into);
            }
            self.text(")");
        } else if let Some((name, into)) = orbistoun_thunk::data_symbol_at(value) {
            // Data-import pages are named too, without a sixth slot: a data import is a zeroed page
            // this project handed the guest, outside the regions the handler knows.
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
/// A faulting handler can be re-entered (the report itself may fault, or the exception may be
/// raised again as it unwinds), and a loop of half-written messages would bury the one that
/// mattered.
#[cfg(windows)]
static REPORTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Whether `len` bytes at `address` can be read without faulting.
///
/// The page holding the faulting instruction pointer is mapped for a data fault but not for an
/// execute fault, where `rip` is the unmapped address itself. A fault inside the fault handler ends
/// the process unreported, so the reporter asks first (D471). `VirtualQuery` allocates nothing and
/// reads nothing at `address`, so it is safe on this path.
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
    // describes the mapping at `address` and does not dereference it, so asking about an unmapped
    // address is what it is for.
    let written = unsafe { VirtualQuery(address as *const core::ffi::c_void, &raw mut info, size) };
    if written == 0 || info.State != MEM_COMMIT {
        return false;
    }
    // A guard page is committed and still faults on touch.
    if info.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    // The whole window has to be inside the one region: the next one may be unmapped.
    let end = (info.BaseAddress as u64).saturating_add(info.RegionSize as u64);
    address.saturating_add(len as u64) <= end
}

/// Away from Windows the fault reporter is not implemented, so nothing here is reached. Answering
/// "not readable" keeps the byte windows empty.
#[cfg(not(windows))]
#[cfg(windows)]
const fn readable(_address: u64, _len: usize) -> bool {
    false
}

/// The host module holding `address`, as a file name and the offset into it.
///
/// A bare host address is not reproducible: Windows bases system modules per boot, so the number
/// changes across reboots, and a recorded outcome in `compat/` would change with it, giving
/// `compare` a false signal. A module's own base is stable relative to itself, so `name+offset`
/// reproduces, and it says which host code faulted.
///
/// Only the file name is kept, never the path, because this string is written into `compat/` and an
/// absolute path from this machine must not reach a tracked file. Runs after the fault rather than
/// inside the handler: it allocates.
#[cfg(windows)]
pub(crate) fn host_module_of(address: u64) -> Option<(String, u64)> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};

    if address == 0 {
        return None;
    }
    // SAFETY: every field is a plain integer or pointer, so all-zero is a valid initialised
    // value; `VirtualQuery` overwrites it before it is read.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a live, correctly sized buffer this call owns. `VirtualQuery` only
    // describes the mapping at `address` and never dereferences it, so asking about an address that
    // just faulted is what it is for.
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
    // The allocation base of a mapped image is its module handle, which makes this two calls rather
    // than a module walk. `base == 0` is refused because `GetModuleFileNameW(NULL)` answers the
    // current executable, which would name an address in no mapping as this binary; the negative
    // test pins it.
    let base = info.AllocationBase as u64;
    if base == 0 || address < base {
        return None;
    }

    // Generous rather than `MAX_PATH`: a truncated answer keeps the prefix, which is the directory
    // this throws away, and loses the file name.
    let mut buffer = [0_u16; 1024];
    // SAFETY: `base` is the allocation base reported above and `buffer` is a live array of the
    // length passed. An address that is not a mapped image answers zero.
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
pub(crate) const fn host_module_of(_address: u64) -> Option<(String, u64)> {
    None
}

/// The bytes of the faulting instruction, copied from the instruction pointer.
///
/// Returns a fixed buffer and how many bytes of it are valid, with no allocation, because this runs
/// inside the fault. The read is clamped to the page `ip` sits in and checked readable first, so it
/// never reaches an unmapped neighbour. A null or wrapped pointer yields zero bytes.
#[cfg(windows)]
fn instruction_bytes(ip: u64) -> ([u8; 16], usize) {
    const PAGE: u64 = 0x1000;
    let mut out = [0_u8; 16];
    if ip == 0 {
        return (out, 0);
    }
    let to_page_end = (PAGE - (ip & (PAGE - 1))) as usize;
    let len = to_page_end.min(out.len());
    // Checked, not assumed: an execute fault puts `rip` at the unmapped address (D471).
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

/// The bytes immediately before the faulting instruction: the code that set up the registers it
/// faulted on, which a null write (`mov [rax], rax` with `rax` zero) is read backwards from.
///
/// Clamped to the start of the page `ip` sits in, the mirror of [`instruction_bytes`]'s clamp to
/// the end, so a fault near a page boundary yields fewer bytes rather than reading a neighbour.
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
    // The same check: a window starting one byte below a placeholder the guest jumped to crosses
    // into an unmapped page (D471).
    if !readable(start, len) {
        return (out, 0);
    }
    // SAFETY: `readable` has just confirmed `len` bytes at `start` are committed and readable;
    // `start .. ip` lies within the single page holding `ip`. Read once into `out`, and the borrow
    // is not held past the call.
    let src = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
    out[..len].copy_from_slice(src);
    (out, len)
}

/// Reads up to `len` bytes of guest memory at `addr`, clamped to the one page `addr` sits in, and
/// only when [`readable`] confirms the range is committed.
///
/// Allocates, so it runs only in the post-message part of [`emit`]. Empty when nothing there is
/// readable, as for a wild address.
#[cfg(windows)]
fn read_window(addr: u64, len: usize) -> Vec<u8> {
    const PAGE: u64 = 0x1000;
    if addr == 0 || len == 0 {
        return Vec::new();
    }
    let to_page_end = (PAGE - (addr & (PAGE - 1))) as usize;
    let take = len.min(to_page_end);
    if !readable(addr, take) {
        return Vec::new();
    }
    // SAFETY: `readable` has confirmed `take` bytes at `addr` are committed and readable, and
    // `take` is clamped to the remainder of the one page `addr` sits in. Read once into an owned
    // buffer; the borrow does not escape the call.
    let src = unsafe { std::slice::from_raw_parts(addr as *const u8, take) };
    src.to_vec()
}

/// Parses a byte length written in hex (`0x...`) or decimal; the reader for a `+len` suffix.
#[cfg(windows)]
fn parse_len(text: &str) -> Option<usize> {
    let text = text.trim();
    let n = match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok()?,
        None => text.parse::<u64>().ok()?,
    };
    usize::try_from(n).ok()
}

/// Parses `<addr>[+len]`: the address in hex (with or without `0x`), and the optional length after
/// `+` (default 0x100).
#[cfg(windows)]
fn parse_addr_len(text: &str) -> Option<(u64, usize)> {
    let (addr_part, len) = match text.split_once('+') {
        Some((a, l)) => (a.trim(), parse_len(l).unwrap_or(0x100)),
        None => (text.trim(), 0x100),
    };
    let addr = u64::from_str_radix(addr_part.strip_prefix("0x").unwrap_or(addr_part), 16).ok()?;
    Some((addr, len))
}

/// Parses an indirect window spec `[<slot>][+len]`: a slot expression in brackets and the optional
/// length after them (default 0x100). Returns the slot address, where a pointer lives, and the
/// length; the caller dereferences the slot at fault time and dumps what it points to.
///
/// A heap object's address changes every run, but the stack slot that holds its pointer sits at a
/// fixed base, so naming the slot reaches the object across runs. `None` when the brackets are
/// absent or malformed, so a plain `<addr>+len` falls through to [`parse_addr_len`].
#[cfg(windows)]
fn parse_indirect(text: &str, registers: &Registers) -> Option<(u64, usize)> {
    let (slot_part, rest) = text.trim().strip_prefix('[')?.split_once(']')?;
    let len = match rest.trim() {
        "" => 0x100,
        r => parse_len(r.strip_prefix('+')?)?,
    };
    let slot = slot_address(slot_part.trim(), registers)?;
    Some((slot, len))
}

/// The address a slot expression names: a hex address, or a register at the fault plus an optional
/// hex offset (`r15+0x8`), since the register holding a heap object at the fault reaches it when no
/// fixed address does.
#[cfg(windows)]
fn slot_address(expr: &str, registers: &Registers) -> Option<u64> {
    let (base, offset) = match expr.split_once('+') {
        Some((b, o)) => (
            b.trim(),
            u64::from_str_radix(o.trim().strip_prefix("0x").unwrap_or(o.trim()), 16).ok()?,
        ),
        None => (expr, 0),
    };
    let named = match base {
        "rax" => Some(registers.rax),
        "rbx" => Some(registers.rbx),
        "rcx" => Some(registers.rcx),
        "rdx" => Some(registers.rdx),
        "rsi" => Some(registers.rsi),
        "rdi" => Some(registers.rdi),
        "rbp" => Some(registers.rbp),
        "rsp" => Some(registers.rsp),
        "r8" => Some(registers.r8),
        "r9" => Some(registers.r9),
        "r10" => Some(registers.r10),
        "r11" => Some(registers.r11),
        "r12" => Some(registers.r12),
        "r13" => Some(registers.r13),
        "r14" => Some(registers.r14),
        "r15" => Some(registers.r15),
        _ => None,
    };
    let base = match named {
        Some(value) => value,
        None => u64::from_str_radix(base.strip_prefix("0x").unwrap_or(base), 16).ok()?,
    };
    Some(base.wrapping_add(offset))
}

/// Dumps the guest-memory windows `ORBISTOUN_DUMP` asks for, as a hexdump, after a fault.
///
/// For code around a fault in a runtime mapping rather than a placed module, which no static
/// disassembly of the file reaches. `caller` resolves to the window ending at the faulting call
/// site, the recorded return address of the last call, since the fault instruction pointer is
/// usually orbistoun's own libc. `<addr>[+len]` dumps a fixed range. Each row is address-prefixed,
/// ready for a disassembler. It only reads and prints, so `ORBISTOUN_DUMP` observes rather than
/// intervenes.
#[cfg(windows)]
fn dump_at_fault(registers: &Registers) {
    use std::fmt::Write as _;

    let Ok(spec) = std::env::var(orbistoun_env::PEEK.name) else {
        return;
    };
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (start, len, note) = if let Some(rest) = part.strip_prefix("caller") {
            let len = rest.strip_prefix('+').and_then(parse_len).unwrap_or(0x100);
            // The recorded return address of the last call, not a frame-pointer walk: in a guest
            // built without frame pointers `rbp` is not a frame link. `RecordedCall::from` is
            // captured at the call site, one past the call that faulted.
            let return_address = orbistoun_thunk::last_call().map_or(0, |call| call.from);
            if return_address == 0 {
                tracing::warn!(
                    "ORBISTOUN_PEEK caller - no recorded call to take a return address from"
                );
                continue;
            }
            // End the window at the call site, so the instructions that set up the call's registers
            // are captured.
            let before = 0xC0_u64.min(len as u64);
            (
                return_address.saturating_sub(before),
                len,
                " (ending at the faulting call site)".to_owned(),
            )
        } else if let Some((slot, len)) = parse_indirect(part, registers) {
            // Dereference the pointer at the slot and dump what it points to: the slot is at a
            // fixed stack base, the target is on a randomised heap.
            let ptr = read_window(slot, 8);
            if ptr.len() < 8 {
                tracing::warn!(
                    "ORBISTOUN_DUMP indirect [{slot:#x}] - {}",
                    describe_unreadable(slot)
                );
                continue;
            }
            let target = u64::from_le_bytes(ptr[..8].try_into().expect("8 bytes read"));
            (target, len, format!(" via [{slot:#x}] -> {target:#x}"))
        } else if let Some((addr, len)) = parse_addr_len(part) {
            (addr, len, String::new())
        } else {
            tracing::warn!(
                "ORBISTOUN_DUMP {part:?} is not `caller`, `<addr>[+len]` or `[<slot>][+len]`"
            );
            continue;
        };
        let bytes = read_window(start, len);
        if bytes.is_empty() {
            tracing::warn!("ORBISTOUN_DUMP {start:#x} - {}", describe_unreadable(start));
            continue;
        }
        let mut dump = format!(
            "ORBISTOUN_DUMP {:#x}..{:#x} ({} bytes){note}:",
            start,
            start + bytes.len() as u64,
            bytes.len(),
        );
        for (row, chunk) in bytes.chunks(16).enumerate() {
            let mut hex = String::with_capacity(chunk.len() * 3);
            for &b in chunk {
                hex.push(char::from(b"0123456789abcdef"[(b >> 4) as usize]));
                hex.push(char::from(b"0123456789abcdef"[(b & 0xf) as usize]));
                hex.push(' ');
            }
            let _ = write!(dump, "\n  {:#010x}  {hex}", start + (row * 16) as u64);
        }
        tracing::info!("{dump}");
    }
}

/// The most recent calls to each import named with `ORBISTOUN_DUMP`, with the guest-code addresses
/// found on the caller's stack at each.
///
/// Candidates, not a call chain: words are copied from the return address up and those in guest
/// code are listed, so a stale word from an older frame qualifies too, and the line says so. Word 0
/// is the return address itself, so the first entry is exact.
#[cfg(windows)]
fn caller_stacks_at_fault() {
    let stacks = orbistoun_thunk::caller_stacks();
    if stacks.is_empty() {
        return;
    }
    for stack in stacks {
        let label = label_of(stack.index as usize).unwrap_or("unknown");
        let code: Vec<String> = stack
            .words
            .iter()
            .enumerate()
            .filter_map(|(word, &value)| {
                let (region, offset) = locate(value)?;
                is_code_region(region).then(|| format!("+{:#x}: {region}+{offset:#x}", word * 8))
            })
            .collect();
        tracing::info!(
            "caller stack of call {} to {label} (code addresses on the stack, return address first; later ones may be stale): {}",
            stack.sequence,
            if code.is_empty() {
                "none".to_owned()
            } else {
                code.join(", ")
            }
        );
    }
}

/// Whether a region named by [`locate`] holds guest code: the image or the title's own modules.
fn is_code_region(region: &str) -> bool {
    region == REGION_NAMES[Region::Image.slot()]
        || region == REGION_NAMES[Region::TitleModules.slot()]
}

/// Written explicitly so a report line is one line however the stream is buffered.
#[cfg(windows)]
const NEWLINE: &str = "
";

/// Says that the guest used one of orbistoun's own placeholder answers as an address.
///
/// To the check above this looks like a fault in orbistoun's code: a stub answers a placeholder,
/// the guest jumps through it, and the instruction pointer is outside every placed region. The
/// diagnosis is the opposite: an unimplemented function answered as designed (D670), so the header
/// names the import to implement rather than printing a host stack trace.
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

/// A fault address at or below this is a null pointer plus a field offset, not an address the guest
/// computed. Matches `orbistoun-report`'s `NEAR_NULL` (a page).
#[cfg(windows)]
const NEAR_NULL_FAULT: u64 = 0x1000;

/// Whether a fault in orbistoun's own code is at a null-ish address, the guest's pointer handed to
/// a libc shim, rather than an emulator logic bug. Pure, so the threshold is tested without a real
/// fault.
#[cfg(windows)]
fn is_null_pointer_fault(faulting_address: u64) -> bool {
    faulting_address < NEAR_NULL_FAULT
}

/// Says, unmistakably, that the fault is in orbistoun's own code rather than the guest's.
///
/// The header names the guest's last import call for context, which could read as "the guest
/// faulted in this function". It is spelled out that the function that faulted is in the host stack
/// below, never the import above.
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

/// The refinement of [`note_emulator_fault`] for a null-ish faulting address in orbistoun's code.
///
/// A null-plus-small-offset fault address is the signature of the guest handing an unpopulated
/// pointer to a libc function (`memcpy`, `strlen`, `memset`), which the host stack shows as
/// `orbistoun_libc`. The guest's own libc would fault the same way on the hardware, so the cause is
/// upstream, the call that should have filled the pointer. Stated conditionally, because a null
/// dereference in orbistoun's own logic lands here too, and the host stack tells the two apart.
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
/// The decision half of the interrupt glue, pure so it is tested without a real trap: the `CONTEXT`
/// read and write-back stay in the vectored handler. Returns the frame to resume from when the
/// instruction is an `int n` with a registered handler, `None` otherwise. The resume point is set
/// past the two-byte `int` before the handler runs, so a handler that touches nothing returns to
/// the instruction after the trap.
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
/// A guest executing `syscall`, `sysenter`, `int`, `hlt` or `ud2` has gone below the library
/// boundary this project intercepts: there is no import to name. The host turns the
/// general-protection fault such an instruction (or a misaligned SSE access) raises into an access
/// violation at `0xffffffffffffffff`, which is not a pointer at all (D384).
#[cfg(windows)]
fn note_instruction_shape(line: &mut Line, opcode: &[u8], faulting_address: u64) {
    // A software interrupt carries its vector in the byte after `0xcd`, and the vector names which
    // kernel entry the guest took. Written straight into the line, never through `format!`, because
    // this runs in the fault handler. The classification is
    // `orbistoun_report::trace::classify_trap`, the one the ranked finding uses, so the crash print
    // and the work list agree; it is pure and allocation-free.
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
        // `int 0x41` is fatal on hardware: a guest trap with an upstream cause, headed and
        // explained as a trap even though `classify_trap` decodes its bytes as a `KernelEntry`
        // shape. Every other vector keeps the kernel-entry framing.
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
                // The vector names the kernel entry exactly.
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
            // A bare `int 0x41` faults on hardware too, so there is no handler to add: the guest
            // took a path the hardware would fault on, which puts the cause upstream, as with
            // `ud2`.
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
            // orbistoun resolves the library boundary (imports) and the syscall-gadget path (D376)
            // but implements no interrupt or trap vectors, so this is orbistoun's gap, stated with
            // the next step.
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

/// The host's view of the page holding `address` (protection, state, kind, region) and every recent
/// change orbistoun's page guards made to it, oldest first.
#[cfg(windows)]
fn note_page(line: &mut Line, address: u64) {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_FREE, MEM_IMAGE, MEM_MAPPED, MEM_PRIVATE, MEM_RESERVE,
        MEMORY_BASIC_INFORMATION, PAGE_NOACCESS, PAGE_READONLY, VirtualQuery,
    };
    let protection_name = |protection: u32| match protection {
        PAGE_NOACCESS => "no access",
        PAGE_READONLY => "read-only",
        0x04 => "read-write",
        0x20 => "execute-read",
        0x40 => "execute-read-write",
        _ => "other",
    };
    let mut info = MEMORY_BASIC_INFORMATION {
        BaseAddress: std::ptr::null_mut(),
        AllocationBase: std::ptr::null_mut(),
        AllocationProtect: 0,
        PartitionId: 0,
        RegionSize: 0,
        State: 0,
        Protect: 0,
        Type: 0,
    };
    // SAFETY: querying any address is safe; the structure is the size passed and outlives the call.
    let answered = unsafe {
        VirtualQuery(
            std::ptr::with_exposed_provenance::<core::ffi::c_void>(
                usize::try_from(address).unwrap_or(0),
            ),
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    line.text("  page ").hex(address & !0xfff).text(": ");
    if answered == 0 {
        line.text("the host would not describe it").text(NEWLINE);
    } else {
        let state = match info.State {
            MEM_COMMIT => "committed",
            MEM_RESERVE => "reserved, not committed",
            MEM_FREE => "free - nothing mapped",
            _ => "unknown state",
        };
        let kind = match info.Type {
            MEM_MAPPED => "mapped view",
            MEM_PRIVATE => "private",
            MEM_IMAGE => "image",
            _ => "",
        };
        line.text(state).text(" ").text(kind);
        if info.State == MEM_COMMIT {
            line.text(", ")
                .text(protection_name(info.Protect))
                .text(" (")
                .hex(u64::from(info.Protect))
                .text(")");
        }
        line.text(", region ")
            .hex(info.BaseAddress as u64)
            .text("+")
            .hex(info.RegionSize as u64)
            .text(NEWLINE);
    }
    let mut changes = [crate::page_guard::Change::default(); 6];
    match crate::page_guard::changes_touching(address, &mut changes) {
        None => {
            line.text("  orbistoun's page-guard history was busy and could not be read")
                .text(NEWLINE);
        }
        Some(0) => {
            line.text(
                "  orbistoun's page guards never changed this page (of their latest 256 changes)",
            )
            .text(NEWLINE);
        }
        Some(count) => {
            line.text("  orbistoun's page guards changed this page, oldest first:")
                .text(NEWLINE);
            for change in &changes[..count] {
                line.text("    #")
                    .hex(change.sequence)
                    .text(" ")
                    .hex(change.base)
                    .text("+")
                    .hex(change.len)
                    .text(" to ")
                    .text(protection_name(change.to))
                    .text(if change.ok {
                        ""
                    } else {
                        " - refused by the host"
                    })
                    .text(NEWLINE);
            }
        }
    }
}

/// Emits one fault report.
///
/// Shared by both platforms so the wording and the region attribution cannot drift. `kind` carries
/// its own preposition ("read of", "illegal instruction at"), so nothing is inserted here.
#[cfg(windows)]
fn emit(kind: &str, faulting_address: u64, instruction_pointer: u64, registers: Registers) {
    use std::io::Write as _;

    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let inside = write_fault_line(kind, faulting_address, instruction_pointer, &registers);

    // The faulting page as the host holds it, and what this process did to its protection: a page
    // left inaccessible by orbistoun's own guards and a page the guest never had look alike from
    // the fault alone. Its own line, still allocation-free.
    if faulting_address != u64::MAX {
        let mut page = Line::new();
        note_page(&mut page, faulting_address);
        // Written directly for the same reason as the line above: still allocation-free.
        let _ = std::io::stderr().write_all(page.as_bytes());
        let _ = std::io::stderr().flush();
    }

    // Guest memory dumped at the fault, if `ORBISTOUN_DUMP` asks. After the allocation-free message
    // and in the same allocating context as the trace below; it reads guest memory the
    // readable-checked way the byte windows above do.
    dump_at_fault(&registers);
    caller_stacks_at_fault();

    // The host stack, only when the fault is orbistoun's own code.
    //
    // The lines above are written before this runs, because capturing a backtrace allocates, takes
    // locks and reads the symbol file. A guest faulting on its own pointer has a host stack of the
    // dispatch machinery, which is noise; a fault in orbistoun's code has one that is the answer.
    if inside.is_some() {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let mut said = String::from("  the host stack that got there:");
        for one in backtrace.to_string().lines().take(40) {
            said.push_str("\n  ");
            said.push_str(one);
        }
        tracing::warn!("{said}");
    }

    // What the guest asked the kernel for directly, which a fault is often the end of. This
    // allocates, and runs after everything that must not.
    what_the_guest_asked_for();

    persist_fault_trace(kind, faulting_address, instruction_pointer, registers);
}

/// Writes the allocation-free first line of a fault report and returns the import it was inside.
#[cfg(windows)]
#[inline]
fn write_fault_line(
    kind: &str,
    faulting_address: u64,
    instruction_pointer: u64,
    registers: &Registers,
) -> Option<&'static str> {
    use std::io::Write as _;

    let mut line = Line::new();
    line.text("orbistoun: guest fault: ").text(kind).text(" ");
    line.address(faulting_address);
    line.text(" while executing at ");
    line.address(instruction_pointer);

    // Naming the import turns "somewhere in the emulator" into "in this function". Everything up to
    // here is allocation-free: the label is a `&'static str` from the import table.
    let inside = inside_import_name(instruction_pointer);
    if let Some(name) = inside {
        line.text(" (inside ").text(name).text(")");
    }
    line.text(NEWLINE);

    // `inside` is set only when the instruction pointer is outside the guest image, which cannot
    // tell orbistoun's code from its own placeholder answers used as a jump target. Checked first,
    // because the two messages send a reader to opposite places.
    let placeholder = orbistoun_core::placeholder_named(instruction_pointer)
        .or_else(|| orbistoun_core::placeholder_named(faulting_address));
    if let Some((what, exact)) = placeholder {
        note_placeholder_fault(&mut line, what, exact);
    } else if inside.is_some() {
        // A null-ish faulting address in orbistoun's code is usually the guest handing a libc shim
        // an unpopulated pointer, said apart so the reader does not debug a correct `memcpy`.
        if is_null_pointer_fault(faulting_address) {
            note_null_pointer_to_libc(&mut line);
        } else {
            note_emulator_fault(&mut line);
        }
    }

    // Where in orbistoun's code, when it is orbistoun's code. `inside` names the last import
    // called, which is an attribution rather than a location. The address is randomised per run, so
    // it is printed as a distance from a function in this file: add it to `emit`'s offset in the
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

    // The faulting instruction itself. In a wrapper-encoded guest the ELF `p_offset` fields do not
    // locate the loaded bytes, so `image+0x...` names a disassembly nobody can reach from the file.
    // These bytes come from where the guest was executing, ready for a disassembler.
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

    // Names a privileged or trap faulting instruction, and explains the all-ones address, when they
    // apply (D384).
    note_instruction_shape(&mut line, &opcode[..len], faulting_address);

    // The path through the guest's own code, which an instruction pointer alone cannot give.
    // Written before the registers because it is read first.
    for frame in walk_frames(registers.rbp) {
        line.text("  from ");
        line.address(frame.return_address);
        line.text(NEWLINE);
    }

    // The stack pointer is on the first line: "the guest ran out of stack" and "a pointer was
    // wrong" look identical without it.
    line.text("  rsp ");
    line.address(registers.rsp);
    line.text("  rdi ");
    line.hex(registers.rdi);
    line.text("  rax ");
    line.hex(registers.rax);
    line.text(NEWLINE);

    // Written directly, not through `tracing`: this is the allocation-free part of the exception
    // handler.
    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();
    inside
}

/// Collects the call trace with the fault site attached and persists it.
#[cfg(windows)]
fn persist_fault_trace(
    kind: &str,
    faulting_address: u64,
    instruction_pointer: u64,
    registers: Registers,
) {
    // Then the call trace: a guest that faults has still said what it wanted.
    //
    // This allocates, which the formatting above does not. A vectored handler runs in ordinary user
    // context on a thread that faulted on a guest pointer, so the allocator is not what broke, and
    // the alternative is discarding the run's only output.
    let module = MODULE.get().map_or("unknown", String::as_str);
    // A region orbistoun placed if it is one, otherwise the host module the address falls in, so
    // the outcome reproduces across reboots. The bare address survives in `instruction_pointer`.
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
            // The handler runs on the faulting thread, so this is that thread's handle. Zero means
            // nothing claimed this host thread, so it is `None` rather than a number nobody issued.
            thread: Some(orbistoun_kernel::thread::current()).filter(|h| *h != 0),
            // The same identifier the recorded calls carry, so the tail can be filtered to this
            // thread (D621).
            host_thread: Some(orbistoun_thunk::current_thread()),
            registers: Some(registers),
            pointees: describe_pointees(&registers),
            frames: walk_frames(registers.rbp),
            // The faulting instruction's bytes, so the ranked findings classify the trap the way
            // the live print does.
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
/// A fault prints sixteen bare values; this reads what the pointer-shaped ones point at. Readable
/// is asked, never assumed, two ways: a register points into a published guest range (`is_mapped`,
/// the argument dumper's question), or at host memory this run allocated for the guest that no
/// published range covers, such as an `orbistoun-libc` heap result where a guest's C++ objects
/// live. The second is VirtualQuery-checked and page-clamped, and a wild value reads back empty and
/// is dropped. A scalar is left out, so the lines that point at objects are not buried.
#[cfg(windows)]
fn describe_pointees(registers: &Registers) -> Vec<String> {
    if !orbistoun_thunk::ranges_known() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, value) in registers.named() {
        if value == 0 {
            continue;
        }
        let bytes: Vec<u8> = if orbistoun_thunk::is_mapped(value) {
            let Ok(at) = usize::try_from(value) else {
                continue;
            };
            // SAFETY: `is_mapped` says this address is inside a region this run published as
            // readable, and sixteen bytes from it stay inside it: the ranges are page-granular and
            // none is shorter.
            let b: [u8; 16] = unsafe {
                std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<[u8; 16]>(at))
            };
            b.to_vec()
        } else {
            // Not in a published range, but perhaps a heap allocation this run handed the guest.
            // `read_window` checks and page-clamps, and returns empty for a wild address.
            read_window(value, 16)
        };
        if bytes.is_empty() {
            continue;
        }
        let hex = bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        // Data-import pages are named too, though they are not one of the five regions.
        let where_ = locate(value).map_or_else(
            || {
                orbistoun_thunk::data_symbol_at(value).map_or_else(
                    || format!("{value:#x}"),
                    |(name, into)| format!("data {name}+{into:#x}"),
                )
            },
            |(region, offset)| format!("{region}+{offset:#x}"),
        );
        // The text form only when it is text: a run of printable bytes ending in a terminator is a
        // string a guest passed, and rendering arbitrary bytes as characters would invent one.
        let text = printable_text(&bytes).map_or_else(String::new, |t| format!("  {t:?}"));
        out.push(format!("{name} -> {where_} = {hex}{text}"));
    }
    out
}

/// The text `bytes` holds, when it holds text and not merely bytes that could be read as some.
///
/// All three must hold: something before the terminator, a terminator inside the window (otherwise
/// the text would be a fragment), and every character printable. `"None"` passes; a pointer that
/// happens to begin `0x65 0x4e` does not. Rendering bytes as characters would invent a string.
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
/// The guest names the mounts it needs, so a mount added afterwards answers a request that was
/// made. Read here rather than printed on the guest's stack (D381).
pub(crate) fn paths_wanted() {
    let wanted = orbistoun_fs::wanted::unanswered();
    if wanted.is_empty() {
        return;
    }
    let mut said = format!(
        "the guest asked for {} path{} nothing here holds:",
        wanted.len(),
        if wanted.len() == 1 { "" } else { "s" }
    );
    for path in &wanted {
        said.push_str("\n  ");
        said.push_str(path);
        orbistoun_core::klog::note(&format!("orbistoun: no such path {path}"));
    }
    said.push_str(
        "\n  each is a directory or file the platform has and the mount table does not - a work item, spelled by the thing that wanted it",
    );
    tracing::info!("{said}");
}

/// Says what the guest opened, when the run was asked to record it.
///
/// The half [`paths_wanted`] cannot show: a guest can open what it asked for and still read nothing
/// useful. Off unless `ORBISTOUN_TRACE_OPENS` is set, because recording successes costs on the
/// guest's stack. A run that was recording and opened nothing says so, which is why this asks the
/// recorder whether it was on.
pub(crate) fn paths_opened() {
    if !orbistoun_fs::opened::recording() {
        return;
    }
    let opened = orbistoun_fs::opened::answered();
    if opened.is_empty() {
        tracing::info!(
            "the guest opened nothing - it read no file at all, which is a finding rather than an empty list"
        );
        return;
    }
    tracing::info!(
        "{}",
        indented(
            format!(
                "the guest opened {} path{}:",
                opened.len(),
                if opened.len() == 1 { "" } else { "s" }
            ),
            &opened
        )
    );

    // And what each read got, which for a title that reads zero bytes is the finding.
    let reads = orbistoun_fs::opened::reads_made();
    if reads.is_empty() {
        return;
    }
    tracing::info!(
        "{}",
        indented(
            format!("and read from them {} time(s):", reads.len()),
            &reads
        )
    );
}

/// A header followed by each item on its own line, indented under it: one log event for what
/// reads as one block.
fn indented<T: std::fmt::Display>(header: String, items: &[T]) -> String {
    use std::fmt::Write as _;

    let mut block = header;
    for item in items {
        let _ = write!(block, "\n  {item}");
    }
    block
}

/// Every mapping the guest was given, in the order it was given them.
///
/// Successful reservations are listed so a pointer into guest memory can be traced to the call that
/// produced it, and two runs' arenas compared placement by placement. The arena is bump-allocated,
/// so the order is the finding.
pub(crate) fn maps_given() {
    use std::fmt::Write as _;

    if !orbistoun_kernel::mapped::recording() {
        return;
    }
    let given = orbistoun_kernel::mapped::given();
    if given.is_empty() {
        tracing::info!(
            "the guest was given no mapping at all, which is a finding rather than an empty list"
        );
        return;
    }
    // Cumulative, because a diff asks which placement first moved, and an address alone does not
    // say how much was placed before it.
    let mut total = 0_u64;
    let mut said = format!("the guest was given {} mapping(s):", given.len());
    for (index, m) in given.iter().enumerate() {
        total = total.saturating_add(m.len);
        let _ = write!(
            said,
            "\n  {index:4}  call {:<8}  {:#018x} +{:#x}  {}{}  (cumulative {:#x})  during {}{}",
            m.at_call,
            m.base,
            m.len,
            if m.readable { "r" } else { "-" },
            if m.hinted { " asked-for" } else { " arena    " },
            total,
            // Named here, the layer that holds the import table. An unnamed index is still stable
            // across runs.
            m.during
                .and_then(|i| label_of(i as usize))
                .unwrap_or("nothing yet"),
            // A refusal is an event the guest asked for and did not get.
            m.refused
                .as_deref()
                .map_or_else(String::new, |why| format!("  REFUSED: {why}"))
        );
    }
    tracing::info!("{said}");
}

/// Says what the guest formatted, which is usually how it explains itself.
///
/// The last ones, because a title says the interesting thing just before it stops. Printed after
/// the guest has stopped (D381).
pub(crate) fn what_it_said() {
    if !orbistoun_libc::said::recording() {
        return;
    }
    let said = orbistoun_libc::said::rendered();
    if said.is_empty() {
        tracing::info!(
            "the guest formatted nothing at all, which is a finding rather than an empty list"
        );
        return;
    }
    tracing::info!(
        "{}",
        indented(
            format!("the guest formatted {} string(s):", said.len()),
            &said
        )
    );
}

/// Says when the argument dump ran out of room.
///
/// A dump wanted and not taken reads as a call that passed nothing worth showing, so a run that
/// dropped any says by how much.
pub(crate) fn dumps_dropped() {
    let dropped = orbistoun_thunk::dumps_dropped();
    if dropped == 0 {
        return;
    }
    tracing::warn!(
        "{dropped} argument dump(s) wanted after the buffer was full - name an import with ORBISTOUN_DUMP to spend the room on it"
    );
}

/// Says when the argument dump ran out of room to remember where it may read.
///
/// A dropped range and a wrong pointer print identically, so a run that dropped any says so.
pub(crate) fn ranges_dropped() {
    let dropped = orbistoun_thunk::dropped_ranges();
    if dropped == 0 {
        return;
    }
    tracing::warn!(
        "{dropped} readable range(s) could not be remembered - an argument pointing into one of them reports as unreadable, and that is this run's blind spot rather than a bad pointer"
    );
}

/// Everything the guest asked the system for, in one call.
///
/// One function for every ending, so a guest that faults and one that stops cleanly report the same
/// records.
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
/// Read here rather than printed on the dispatch path, which runs on the guest's stack and only
/// sets a bit (D381). Called from every path a run can end by, after the guest has run.
pub(crate) fn syscalls_asked_for() {
    use std::fmt::Write as _;

    // The sequence first, because when is what says what the guest was doing (D388).
    let sequence = orbistoun_thunk::syscall::syscalls_in_order();
    if !sequence.made.is_empty() {
        let mut said = format!("the guest made {} syscalls, in this order:", sequence.total);
        for (position, asked) in sequence.made.iter().enumerate() {
            let called = asked.name.unwrap_or("nothing here implements it");
            let _ = write!(
                said,
                "\n  {position:3}  {:5}  {called}  ({:#x})",
                asked.number, asked.argument
            );
            orbistoun_core::klog::note(&format!("orbistoun: syscall {} - {called}", asked.number));
        }
        let kept = sequence.made.len() as u64;
        if sequence.total > kept {
            let _ = write!(
                said,
                "\n  and {} more, past what this run records in order",
                sequence.total - kept
            );
        }
        tracing::info!("{said}");
    }

    for (number, name) in orbistoun_thunk::syscall::syscalls_asked_for() {
        if let Some(name) = name {
            tracing::info!(
                "the guest asked the kernel for call {number} directly, which is {name}"
            );
        } else {
            tracing::info!(
                "the guest asked the kernel for call {number} directly, and nothing here implements it"
            );
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::emit;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, CONTEXT, EXCEPTION_POINTERS,
    };

    /// Let the exception carry on to whatever would otherwise have handled it.
    ///
    /// This reports and gets out of the way: swallowing the fault would leave a guest running with
    /// corrupt state.
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
    /// Correct for a watched access, which has already happened with nothing broken; wrong for
    /// every other exception this handler sees.
    const CONTINUE_EXECUTION: i32 = -1;

    /// What the first parameter of an access violation means.
    const ACCESS_WAS_WRITE: usize = 1;
    /// A fetch from a page with no execute permission.
    const ACCESS_WAS_EXECUTE: usize = 8;

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // Read one field per block, as the lints require: each read is a separate dereference of a
        // pointer the operating system owns, stated once per block.

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
        // SAFETY: same context record, read once as a whole: `CONTEXT` is `Copy`, so this is one
        // dereference and one copy.
        let ctx = unsafe { *context };

        let registers = registers_of(&ctx);

        // Taken before anything else, because a watched access is not a failure. The debug-status
        // register says which watchpoints fired; it is cleared before resuming so the next trap is
        // not read as this one repeating.
        if code == SINGLE_STEP {
            // SAFETY: `context` is the operating system's live context record for this call.
            return unsafe { resume_watched(context, ctx.Dr6, rip, &registers) };
        }

        // A guest thread-local access whose `fs` base the host reset (D433): put the base back and
        // let the instruction run again. Only for access violations, and only when the base has
        // reverted to zero, so a genuine fault still reaches the report. Sound because a zero base
        // sends every `fs:` access into the unmapped 2 GiB around zero, so it faults here rather
        // than reading wrong data.
        if code == ACCESS_VIOLATION && crate::tls_backstop::restore_if_reverted() {
            return CONTINUE_EXECUTION;
        }

        // A read or write of a deferred copy's destination carries the copy out (D717): the command
        // processor's copy of a colour target was left waiting with its destination guarded, and
        // this is the first touch. The bytes are written, the pages made accessible, and the access
        // runs as though they had been there. Parameter zero is the access kind (0 read, 1 write, 8
        // execute, never ours), parameter one the address. A write to a protected colour target
        // marks it written: its pages get their protection back and the write runs. Asked first, so
        // a range released and still remembered as resolved below never answers for pages protected
        // again since.
        if code == ACCESS_VIOLATION
            && count >= 2
            && parameters[0] == 1
            && orbistoun_gpu::agc_driver::written_at(parameters[1] as u64)
        {
            return CONTINUE_EXECUTION;
        }
        if code == ACCESS_VIOLATION
            && count >= 2
            && matches!(parameters[0], 0 | 1)
            && orbistoun_gpu::agc_driver::carry_out_at(parameters[1] as u64)
        {
            return CONTINUE_EXECUTION;
        }

        // A software interrupt with a registered handler is serviced, not reported. A guest
        // reaching the kernel by `int n` raises a general-protection fault the host delivers as an
        // access violation (D384); with a handler, it runs against the guest's registers and
        // resumes past the instruction, as the kernel's own interrupt gate would. Without one the
        // trap falls through to the fault report.
        if matches!(code, ACCESS_VIOLATION | ILLEGAL_INSTRUCTION)
            && let Some(next) = serviced_context(&ctx, rip)
        {
            // SAFETY: `context` is the operating system's live context record for this call;
            // writing the whole record it owns is how a serviced interrupt resumes.
            unsafe {
                *context = next;
            }
            return CONTINUE_EXECUTION;
        }

        let Some((kind, at)) = fault_kind(code, count, &parameters, rip) else {
            return CONTINUE_SEARCH;
        };

        emit(kind, at, rip, registers);
        CONTINUE_SEARCH
    }

    /// Copies the general-purpose registers out of a host context record.
    #[inline]
    fn registers_of(ctx: &CONTEXT) -> super::Registers {
        super::Registers {
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
        }
    }

    /// Answers a debug exception: resumes a watched access, or passes on one that is not ours.
    ///
    /// # Safety
    ///
    /// `context` is the operating system's live context record for the current exception.
    #[inline]
    unsafe fn resume_watched(
        context: *mut CONTEXT,
        dr6: u64,
        rip: u64,
        registers: &super::Registers,
    ) -> i32 {
        match crate::watchpoint::note(dr6, rip, registers) {
            crate::watchpoint::Trap::NotOurs => CONTINUE_SEARCH,
            crate::watchpoint::Trap::Data => {
                // SAFETY: the context record is the one the operating system passed in, live for
                // this call, and this writes a single field it owns before resuming.
                unsafe {
                    (*context).Dr6 = 0;
                }
                CONTINUE_EXECUTION
            }
            crate::watchpoint::Trap::Execute { disarm } => {
                // An execute breakpoint is one-shot: clearing its enable bit lets the instruction
                // run now instead of trapping again.
                // SAFETY: the OS's context, live for this call; a field it owns.
                unsafe {
                    (*context).Dr6 = 0;
                }
                // SAFETY: as above; clears the enable bits of the fired execute slots.
                unsafe {
                    (*context).Dr7 &= !disarm;
                }
                CONTINUE_EXECUTION
            }
        }
    }

    /// Runs a registered software-interrupt handler and returns the context to resume with.
    #[inline]
    fn serviced_context(ctx: &CONTEXT, rip: u64) -> Option<CONTEXT> {
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
        let resumed = super::serviced_interrupt(&opcode[..len], frame)?;
        // The register fields are updated on a copy of the context and the whole record written
        // back in one store, keeping a single unsafe operation.
        let mut next = *ctx;
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
        Some(next)
    }

    /// Names the fault and the address it concerns, or `None` for an exception that is not ours.
    #[inline]
    fn fault_kind(
        code: i32,
        count: u32,
        parameters: &[usize; 15],
        rip: u64,
    ) -> Option<(&'static str, u64)> {
        // An access violation reports what was attempted and where; the others carry no parameters,
        // so the faulting address is the instruction itself. The strings come from the two lists on
        // `FaultSite`, so a consumer telling "the guest touched this address" from "this is the
        // faulting instruction" reads one copy.
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
            // The address decides which of the three breakpoint kinds this is.
            BREAKPOINT => (super::breakpoint_kind(rip), rip),
            STACK_OVERFLOW => (at_instruction[2], rip),
            // Anything else is not ours to explain: debuggers and language runtimes raise
            // exceptions routinely.
            _ => return None,
        };
        Some((kind, at))
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
    /// Not implemented away from Windows.
    ///
    /// Reporting from a signal handler needs `ucontext` to reach the instruction pointer, which the
    /// crates in use here do not expose, and a report without it would omit what was executing.
    pub(super) fn install() -> bool {
        false
    }
}

/// Names for stub indices, so a call trace says what the guest wanted.
///
/// Set before entering the guest. Empty is legitimate (a module with no readable import table still
/// runs), and the summary then prints bare indices.
static IMPORT_LABELS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Where each implementation starts, sorted, so a fault inside one can be named.
///
/// The binary carries no symbols on this toolchain, so a table of function pointers is what names
/// an address in orbistoun's own code (D380).
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
/// Nearest preceding, with the distance printed: a fault inside an inlined or called library
/// routine names the last implementation before it, which is a hint, and a large offset is visibly
/// not a match.
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
/// Indexed by dynamic symbol index, matching the stub table. An empty entry means the symbol is not
/// an import and is never called.
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
/// A trace that exists only on a terminal dies with the run that produced it.
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
/// Bounded because the chain is guest-controlled: a corrupt frame pointer can form a cycle.
pub const MAX_FRAMES: usize = 12;

/// Walks the frame-pointer chain from `rbp`.
///
/// The chain of return addresses says who called the faulting code, which the instruction pointer
/// alone does not. Best effort: optimised code often omits the frame pointer, and an empty walk is
/// that case rather than a failure. Every read is bounds-checked against the stack region first,
/// because a second fault in the handler would replace the report with silence.
#[cfg(windows)]
fn walk_frames(rbp: u64) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut current = rbp;

    for _ in 0..MAX_FRAMES {
        // Both the saved pointer and the return address must lie inside the stack, and a frame must
        // be aligned.
        if current == 0 || current % 8 != 0 || locate(current).is_none_or(|(r, _)| r != "stack") {
            break;
        }
        let Ok(at) = usize::try_from(current) else {
            break;
        };
        // SAFETY: `current` was just confirmed to lie inside the guest stack region this process
        // reserved and still holds, and to be eight-byte aligned. This is the saved frame pointer a
        // prologue pushed.
        let saved = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) };
        // SAFETY: the word above it, the return address the call placed there, inside the same
        // validated frame.
        let return_address =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at + 8)) };
        if return_address == 0 {
            break;
        }
        frames.push(Frame {
            return_address,
            frame_pointer: current,
        });
        // A chain must climb; anything else is a cycle.
        if saved <= current {
            break;
        }
        current = saved;
    }
    frames
}

/// Names the import this thread was inside, when the fault was not in guest code.
///
/// An instruction pointer inside a region orbistoun placed is the guest running its own code.
/// Outside every placed region it is orbistoun's code running on the guest's behalf, and the import
/// the faulting thread is inside is the one that faulted. The faulting thread's own current call is
/// used, not the newest call any thread made (D621). Allocation-free: the label is a `&'static
/// str`.
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
/// The first argument comes from the ordered log rather than the seen-bitmap, which records only
/// that a number was asked for.
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

/// The report the command-buffer driver recorded for the last command buffer a guest submitted,
/// summarised for the run report, or `None` if no guest reached a submission.
fn submission_summary() -> Option<SubmissionSummary> {
    orbistoun_gpu::agc_driver::last_submission_report().map(|report| SubmissionSummary {
        packets: report.packets,
        register_writes: report.register_writes,
        draws: report.draws,
        shaders_found: report.shaders_found,
        addresses_resolved: report.addresses_resolved,
        addresses_unresolved: report.addresses_unresolved,
        shaders_translated: report.shaders_translated,
        shader_failures: report
            .failures
            .iter()
            .map(|f| format!("{} at {:#x}: {}", f.stage, f.address, f.reason))
            .collect(),
    })
}

/// The head window of the flipped buffer to read, as `(address, len)`, or `None` when the address
/// is not inside any allocated region.
///
/// Pure, so the bounds arithmetic is testable. A guest can flip a buffer registered with any
/// address, so the range is checked against the run's allocated regions before a byte is read
/// (D101). The window is capped and never runs past the end of its region.
fn flipped_window(address: u64, regions: &[(u64, u64, bool)], cap: u64) -> Option<(u64, usize)> {
    let room = regions
        .iter()
        .filter(|(_, _, allocated)| *allocated)
        .find(|(start, end, _)| address >= *start && address < *end)
        .map(|(_, end, _)| end - address)?;
    let len = usize::try_from(room.min(cap)).ok()?;
    (len > 0).then_some((address, len))
}

/// Whether the head of `address`, bounded to `regions` and capped at `cap`, holds a byte the guest
/// wrote (any non-zero one).
///
/// The read-and-decide behind [`flipped_frame_written`], split out so the bounds check and the read
/// can be exercised on a real buffer in a test. False when the address is outside every allocated
/// region or the window is all zero.
fn frame_written_against(address: u64, regions: &[(u64, u64, bool)], cap: u64) -> bool {
    let Some((at, len)) = flipped_window(address, regions, cap) else {
        return false;
    };
    let Ok(at) = usize::try_from(at) else {
        return false;
    };
    // SAFETY: `flipped_window` returned this range only after finding it inside an allocated
    // region, and the caller guarantees those regions describe currently mapped memory: in a run
    // the guest's own with the map locked, in a test a live host buffer. `len` was clamped to the
    // region's end.
    unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), len) }
        .iter()
        .any(|&b| b != 0)
}

/// Whether the buffer the guest last flipped holds bytes it wrote: the framebuffer signal behind
/// `Reach::Presented`.
///
/// The address comes from the video crate. It is bounds-checked against the run's allocated regions
/// before it is read, and only a bounded head is read: an unwritten buffer is zero throughout while
/// a written one differs at its start. False when nothing flipped, when the address lies outside
/// every allocated region, or when the window is all zero.
fn flipped_frame_written() -> bool {
    /// The head read of a flipped buffer, in bytes: one page, enough for a written frame to show at
    /// its start and cheap to read.
    const WINDOW: u64 = 4096;

    // The decoded shape is available here; the head window below is a fixed page.
    let Some((address, _shape)) = orbistoun_video::last_flipped_buffer() else {
        return false;
    };
    let Ok(map) = orbistoun_kernel::direct::map().lock() else {
        return false;
    };
    // The regions are copied out so the read-and-decide is a function of data, and the lock is held
    // across it so a validated region cannot be unmapped underneath the read.
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
    // What the watched region became, printed here because this is the one point reached on both
    // endings, a fault and a time limit.
    let changed = crate::watch::changes();
    if !changed.is_empty() {
        let mut said = String::from("watched region:");
        for line in &changed {
            said.push('\n');
            said.push_str(line);
        }
        tracing::info!("{said}");
    }

    let mut counts: Vec<(usize, u64)> = orbistoun_thunk::call_counts()
        .into_iter()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .collect();
    counts.sort_unstable_by_key(|(_, calls)| std::cmp::Reverse(*calls));

    // Read once, beside the counts, and indexed by the same import index, so each row's inferred
    // signature comes from the same snapshot as its call count.
    let shapes = orbistoun_thunk::arg_shapes();

    CallTrace {
        module: module.to_owned(),
        reached: reached.to_owned(),
        // Summed from the same snapshot the rows are drawn from rather than the global counter,
        // which moves while the guest runs, so the total matches the rows beneath it.
        total_calls: counts.iter().map(|(_, n)| *n).sum(),
        distinct: counts.len(),
        // Asked of the port table rather than counted from the rows, because a submission a port
        // refused is still a call to something implemented (D558).
        frames: orbistoun_video::flips_accepted(),
        // Whether the buffer that flip carried holds bytes the guest wrote, read back and
        // bounds-checked: the signal that lifts `flipped` to `presented`.
        frame_written: flipped_frame_written(),
        // The first command buffer a guest handed to `sceAgcDriverSubmitDcb`, summarised.
        submission: submission_summary(),
        // Recorded, not just printed, so a guest that talks to the kernel by number leaves
        // something for the work list to rank (D401).
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
            // Merged at read time, because the conditions are recorded before the import labels
            // exist and resolving which import to plant into needs them.
            let mut c = linked_conditions();
            if let Some(planted) = PLANTED.get() {
                // With the counts, always: a forced write that matched an import and refused every
                // target otherwise looks like one that landed and changed nothing.
                let (done, refused) = orbistoun_thunk::forced_write_counts();
                // Forced returns are counted separately from the plants, since they are a different
                // mechanism: an unmoved fault under a forced answer reads identically when nothing
                // was ever answered (D166).
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

                // Asked for and applied zero times: recorded as its own line rather than a count of
                // nought in the conditions line (D227).
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
/// Names the slot when it cannot name the function, so two different unlabelled stubs never read as
/// one function called twice (D366).
fn labelled(index: usize) -> String {
    label_of(index).map_or_else(|| format!("unknown#{index}"), str::to_owned)
}

/// The last calls the guest made, labelled.
///
/// Taken from the circular ring the dispatcher fills, so these are the last calls however long the
/// guest ran (D571).
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
/// The thread registry knows names and handles, the ring knows what was called on which host
/// thread, and only together do they say which named thread went quiet.
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
/// The findings are derived on read rather than stored, so they follow the current rules. Failures
/// are reported and swallowed: failing a run because a directory was missing would be worse than
/// losing its trace.
pub fn persist(trace: &CallTrace) {
    // Every path that ends a run comes through here, so a watchpoint summary is reached on every
    // ending, including a fault. Silent unless something was armed.
    crate::watchpoint::summarise();
    // The same place for the shell summary, including after a fault, when somebody most wants to
    // know what the guest was and was not told.
    crate::session::summarise();
    // A reservation the guest could not make is otherwise invisible: `map` answers `NoMemory` and
    // the guest faults far away. Named here, from the host side where a `klog` line is safe.
    if let Some(failure) = orbistoun_mem::last_reserve_failure() {
        // The count as well as the last one: two runs can share a last failure and still have
        // failed a different number of reservations.
        tracing::warn!(
            "{} reservation(s) failed, first at {:#x}, last: base={:#x} len={:#x} - {}",
            orbistoun_mem::reserve_failures(),
            orbistoun_mem::first_reserve_failure_base().unwrap_or(0),
            failure.base,
            failure.len,
            failure.reason
        );
    }
    // A diagnostic that intervened says what it did (D325). Silence means none was asked for; a
    // line reporting that nothing fired means the run tested nothing. Here, on the path every
    // ending reaches, including a fault.
    for fill in [
        orbistoun_kernel::direct_fill_summary(),
        orbistoun_libc::heap_fill_summary(),
        orbistoun_libc::heap_base_summary(),
        // Not a diagnostic but a gap report, unconditional for that reason: it says what the run
        // did not do (D515).
        orbistoun_kernel::module_start_summary(),
        orbistoun_kernel::equeue_summary(),
    ]
    .into_iter()
    .flatten()
    {
        tracing::info!("{fill}");
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
                tracing::warn!("could not write the call trace to {}: {e}", path.display());
            }
        }
        Err(e) => tracing::warn!("could not serialise the call trace: {e}"),
    }
}

/// Exit status used when a guest outruns its time limit.
///
/// Outside the range the platform uses for faults, so "still running when the clock ran out" and
/// "died on a bad pointer" are never confused.
pub const TIME_LIMIT_EXIT: i32 = 0x0B0E;

/// Exit status for a guest stopped by the call budget.
///
/// Distinct from [`TIME_LIMIT_EXIT`]: running out of clock varies with the machine, and making the
/// allowed number of calls does not (D238).
pub const CALL_BUDGET_EXIT: i32 = 0x0B0F;

/// Records that the guest stopped itself, and ends the process.
///
/// Installed as the handler the subsystem crates call, since they cannot call upwards. The trace is
/// persisted before exiting: a guest that gave up has still said what it wanted.
fn guest_stopped(reason: orbistoun_core::StopReason, code: u64) -> ! {
    // Recorded before the trace is collected, so the report says the guest stopped rather than
    // describing an absent fault as a time limit.
    let _ = STOPPED.set(reason.label().to_owned());
    // Every reporter, through the one function that names them, as on the fault path and the
    // ordinary return (D387).
    what_the_guest_asked_for();
    let module = MODULE.get().map_or("unknown", String::as_str);
    let trace = collect_calls(module, "Entered");
    persist(&trace);

    tracing::info!("{} ({code:#x})", reason.label());

    std::process::exit(orbistoun_core::stop::EXIT_GUEST_STOPPED);
}

/// Reports what the guest managed first. A guest that hangs and is killed from outside takes its
/// call trace with it, so the trace is written from in here.
///
/// Runs on an ordinary thread rather than in a fault handler, so it may allocate.
pub fn install_stop_handler() {
    orbistoun_core::stop::on_guest_stop(guest_stopped);
}

/// What the guest was pointing at, described rather than dumped raw.
///
/// Named against a region, because `stack+0x800c90` says the guest handed over a local, which
/// distinguishes an out-parameter from a pointer into its own data.
fn collected_dumps() -> Vec<ArgumentDump> {
    // The arena is registered here, where its name is used: it does not exist at entry and grows as
    // the guest maps, so it is read at collection time.
    if let Some((base, len)) = orbistoun_kernel::arena_extent() {
        describe_region(Region::Mappings, base, len);
    }
    orbistoun_thunk::argument_dumps()
        .into_iter()
        .map(|d| ArgumentDump {
            label: label_of(d.index as usize).unwrap_or("unknown").to_owned(),
            slot: d.slot,
            value: d.address,
            // Named against a region only when it pointed at one; a scalar has no region.
            //
            // An address-shaped value no region covers is said out loud rather than rendered as a
            // bare number, because a count and a wrong or undeclared pointer look identical. It
            // says "published" because that is what was checked: the dump reads only spans
            // something published as readable, and cannot establish that an address is unmapped.
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
/// Empty rather than escaped when they do not: a line of dots beside real hex is noise, while a
/// name or path here is often the answer.
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
/// The reader needs to tell "implement this" from "the value never arrived", which only a sentence
/// carries.
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
/// Set once during setup by the code that knows the limit and the policy: a trace is collected from
/// a fault handler with no route back up to the configuration.
pub fn record_conditions(conditions: Conditions) {
    let _ = CONDITIONS.set(conditions);
}

/// What the run is subject to. Empty until setup records it.
static CONDITIONS: std::sync::OnceLock<Conditions> = std::sync::OnceLock::new();

/// Every diagnostic in force, if any.
///
/// A second slot, because the conditions are recorded before the import labels exist and deciding
/// which import an experiment applies to needs them. Merged when the trace is collected.
static PLANTED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records what the run was put under, for the run conditions.
pub fn note_experiments(what: String) {
    let _ = PLANTED.set(what);
}

/// The digest of the link plan this run applied (D724).
///
/// A third slot, because linking happens after the conditions are recorded.
static LINK_PLAN: std::sync::OnceLock<(String, &'static str, Vec<String>)> =
    std::sync::OnceLock::new();

/// Records which link plan the run applied, how it stood against the stored one, and on a mismatch
/// what differed, for the run conditions.
pub fn note_link_plan(digest: String, stored: &'static str, differs: Vec<String>) {
    let _ = LINK_PLAN.set((digest, stored, differs));
}

/// The recorded conditions, with the link plan merged in.
fn linked_conditions() -> Conditions {
    let mut conditions = CONDITIONS.get().cloned().unwrap_or_default();
    if let Some((plan, stored, differs)) = LINK_PLAN.get() {
        conditions.link_plan.clone_from(plan);
        (*stored).clone_into(&mut conditions.link_plan_stored);
        conditions.link_plan_differs.clone_from(differs);
    }
    conditions
}

/// Reports what the guest managed first, if it runs out of time.
///
/// The wait is a sampling loop, so the report can say when the guest stopped calling anything,
/// which separates slow from stuck.
pub fn start_time_limit(seconds: u64, module: String) {
    std::thread::spawn(move || {
        let quiet = wait_watching_for_silence(seconds);
        note_quiet(quiet);
        note_ended_by(orbistoun_report::trace::RAN_TO_LIMIT);
        // The same reporters every other ending runs: the syscall census, the paths the guest asked
        // for and the paths it opened (D387).
        what_the_guest_asked_for();
        let trace = collect_calls(&module, "Entered");
        // A guest that submitted and then waited out the clock still made a submission, and it
        // reaches the backend here as on the ordinary return. After the trace, because rendering
        // takes the submission the trace's summary reads.
        crate::render::render_and_log_last_submission();
        // Persisted before the summary is printed and before the process ends: nothing else will
        // get a chance to save it.
        persist(&trace);
        summarise_calls(&format!("after {seconds}s"), &trace);
        std::process::exit(TIME_LIMIT_EXIT);
    });
}

/// How often the counters are read while a run is in flight.
///
/// Two relaxed loads four times a second cost the guest nothing measurable, and a quarter second is
/// well under any silence worth reporting: [`Quiet::is_notable`] does not fire below a second.
const SAMPLE: std::time::Duration = std::time::Duration::from_millis(250);

/// Sleeps out the limit, watching for the guest to stop asking the host for anything.
///
/// Returns where the last activity was, not a verdict: the guest may be blocked or computing, and
/// this says only when it last crossed into the host.
fn wait_watching_for_silence(seconds: u64) -> Quiet {
    let started = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(seconds);
    let mut seen = activity();
    // Zero, not "unknown": a guest that never calls anything was silent for the whole run.
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
/// Both halves, because a title calls imports and few syscalls while an open-toolchain payload
/// calls almost nothing but syscalls.
fn activity() -> u64 {
    orbistoun_thunk::total_calls().saturating_add(orbistoun_thunk::syscall::syscalls_made())
}

/// Records the silence for the trace to carry.
fn note_quiet(quiet: Quiet) {
    let _ = QUIET.set(quiet);
}

/// Records which limit is stopping this run, before the trace that reports it is collected.
///
/// The clock and the budget leave by different exit codes, but the parent reads those after the
/// trace is written, so without this both endings would be described as the clock (D238).
fn note_ended_by(reason: &str) {
    let _ = ENDED_BY.set(reason.to_owned());
}

/// Which limit stopped the run, once one has.
///
/// A slot, like the conditions, the experiments and the silence, because the trace is collected
/// with no route back up to the thread that knows. `None` means no limit fired.
static ENDED_BY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The silence, once something has measured it.
///
/// A slot for the same reason; `None` means nobody looked, which is every run the clock did not
/// end.
static QUIET: std::sync::OnceLock<Quiet> = std::sync::OnceLock::new();

/// The module a budgeted run is executing, for the stop callback to name.
static BUDGET_MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// The budget in force, so the summary can say what was reached.
static BUDGET_CALLS: AtomicU64 = AtomicU64::new(0);

/// Stops the guest once it has made `budget` import calls.
///
/// The deterministic half of the pair: the wall-clock limit fixes the duration and lets the call
/// count vary, and this fixes the count (D238). Not a replacement for the clock: a guest that stops
/// calling imports never reaches a budget, so both are installed and either may fire.
pub fn start_call_budget(budget: u64, module: String) {
    let _ = BUDGET_MODULE.set(module);
    BUDGET_CALLS.store(budget, Ordering::Relaxed);
    orbistoun_thunk::install_call_budget(budget, on_budget_reached);
}

/// Writes the trace and ends the process, when the budget is reached.
///
/// A plain `fn` because the dispatch path holds a function pointer and cannot hold a closure
/// without allocating. What it would have captured lives in the two statics above.
fn on_budget_reached() {
    let module = BUDGET_MODULE.get().map_or("", String::as_str);
    note_ended_by(orbistoun_report::trace::SPENT_THE_BUDGET);
    // Every reporter, through the one function that names them. This runs once, after the last call
    // the budget allows, so its allocation is the same trade `collect_calls` makes.
    what_the_guest_asked_for();
    let trace = collect_calls(module, "Entered");
    crate::render::render_and_log_last_submission();
    persist(&trace);
    // Not "after N calls": the count is already the second half of that line.
    summarise_calls("when its call budget ran out", &trace);
    std::process::exit(CALL_BUDGET_EXIT);
}

/// Writes what the guest called, most-used first.
///
/// The counts are the work list: implementing the top of this list moves a guest further, and the
/// order is not guessable from a static import dump.
fn summarise_calls(stopped: &str, trace: &CallTrace) {
    use std::fmt::Write as _;

    let mut said = format!(
        "the guest was still running {stopped}; {} import calls across {} distinct imports",
        trace.total_calls, trace.distinct
    );
    // Printed whether or not it is notable, so its absence is never read as "fine".
    if let Some(quiet) = trace.quiet {
        let _ = write!(said, "\nit {}", quiet.describe());
    }
    for call in trace.calls.iter().take(MOST_CALLED_REPORTED) {
        // Integer tenths of a percent rather than floating point: the counts run past what an `f64`
        // holds exactly.
        let tenths = call
            .calls
            .saturating_mul(1000)
            .checked_div(trace.total_calls)
            .unwrap_or(0);
        // Formatted as text, so the width below is a width: a precision on a string truncates it.
        let share = format!("{}.{}", tenths / 10, tenths % 10);
        let _ = write!(
            said,
            "\n  {:>12} calls ({share:>5}%)  {}",
            call.calls, call.label
        );
    }
    tracing::info!("{said}");
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
    // The fault-report helpers are Windows-only, so their tests and the imports those tests need
    // are gated too.
    #[cfg(windows)]
    use super::{
        is_null_pointer_fault, note_emulator_fault, note_instruction_shape,
        note_null_pointer_to_libc, parse_addr_len, parse_indirect, serviced_interrupt,
    };

    /// The indirect peek names a slot, not the object: the bracket form yields the slot, a plain
    /// `<addr>+len` does not parse as indirect, and malformed brackets dereference nothing.
    #[cfg(windows)]
    #[test]
    fn an_indirect_peek_parses_the_slot_and_leaves_direct_specs_alone() {
        let regs = orbistoun_report::trace::Registers::default();
        // A bracketed slot with an explicit length, and with the default length when none is given.
        assert_eq!(
            parse_indirect("[0x6000007fc0b8]+0x80", &regs),
            Some((0x6000_007f_c0b8, 0x80))
        );
        assert_eq!(
            parse_indirect("[0x6000007fc0b8]", &regs),
            Some((0x6000_007f_c0b8, 0x100))
        );
        // A plain direct spec is not indirect, so it is left to `parse_addr_len`, which accepts it.
        assert_eq!(parse_indirect("0x6000007fc0b8+0x80", &regs), None);
        assert_eq!(
            parse_addr_len("0x6000007fc0b8+0x80"),
            Some((0x6000_007f_c0b8, 0x80))
        );
        // Malformed brackets do not parse as indirect: no closing bracket, and trailing text that
        // is not a `+len`.
        assert_eq!(parse_indirect("[0x10", &regs), None);
        assert_eq!(parse_indirect("[0x10]garbage", &regs), None);
    }

    /// A slot can be a register at the fault plus an offset; an unknown name is refused, not
    /// read as zero.
    #[cfg(windows)]
    #[test]
    fn an_indirect_peek_takes_a_register_base() {
        let regs = orbistoun_report::trace::Registers {
            r15: 0x7400_0204_78a0,
            ..orbistoun_report::trace::Registers::default()
        };
        assert_eq!(
            parse_indirect("[r15+0x8]+0x40", &regs),
            Some((0x7400_0204_78a8, 0x40))
        );
        assert_eq!(
            parse_indirect("[r15]", &regs),
            Some((0x7400_0204_78a0, 0x100))
        );
        assert_eq!(parse_indirect("[r99+0x8]", &regs), None);
    }

    /// The flipped-buffer window is bounded to an allocated region: clamped to its end, and nothing
    /// for an address past it, in an unallocated region, or in no region.
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

    /// A written head reads back as written; a zero head and an out-of-region address do not.
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

    /// A null-ish fault in orbistoun's code is called the guest's libc pointer, not an emulator
    /// bug, while a real guest address still reads as an emulator fault.
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

    /// The interrupt glue services an `int` with a handler and resumes past it, and declines
    /// everything else.
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
        // int 0x41 has no handler, since it is fatal on hardware, so it is left for the report.
        assert!(
            serviced_interrupt(&[0xcd, 0x41], InterruptFrame::default()).is_none(),
            "int 0x41 is measured fatal, so it is never serviced - it falls through to the report"
        );
    }

    /// A software interrupt names its own vector and category: `int 0x41` as a trap with an
    /// upstream cause, an unmeasured vector as a kernel entry that is orbistoun's gap, and `ud2` as
    /// a guest trap.
    #[cfg(windows)] // exercises the Windows-only `note_instruction_shape` helper
    #[test]
    fn a_software_interrupt_names_its_vector_and_its_measured_category() {
        // int 0x41: fatal on hardware, so a guest trap pointing upstream.
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

        // int 0x42: unmeasured, so a kernel entry that is orbistoun's gap to characterise.
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

    /// An address in a registered region is named with its region and offset.
    #[test]
    fn an_address_inside_a_registered_region_is_named_with_its_offset() {
        describe_region(Region::Image, 0x4000_0000_0000, 0x10_0000);
        assert_eq!(locate(0x4000_0000_1234), Some(("image", 0x1234)));
    }

    /// An address outside every region is said to be outside; one merely undeclared is not.
    ///
    /// The regions are described here because the region table is process-wide and shared with
    /// every test in this module; only the envelope's extremes matter.
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

        // Inside the envelope and in no region: undeclared, which must not borrow the stronger
        // sentence.
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

    /// An address outside every region is left unnamed rather than guessed.
    #[test]
    fn an_address_outside_every_region_is_left_unnamed_rather_than_guessed() {
        // Attributing an unmapped address to the nearest region would point at unrelated code.
        describe_region(Region::Stubs, 0x7000_0000_0000, 0x1000);
        assert_eq!(locate(0x1234), None);
    }

    /// A region's end is outside it.
    #[test]
    fn the_end_of_a_region_is_outside_it() {
        // Off-by-one here would name the first byte of whatever follows.
        describe_region(Region::Stack, 0x6000_0000_0000, 0x1000);
        assert_eq!(locate(0x6000_0000_0FFF), Some(("stack", 0xFFF)));
        assert_eq!(locate(0x6000_0000_1000), None);
    }

    /// The mapping arena has a name, so a pointer into it does not read like a count.
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

    /// Hexadecimal formatting needs no allocation.
    #[test]
    fn hexadecimal_is_formatted_without_allocating() {
        // Hand-rolled because a handler may run while the code that just crashed holds the
        // allocator lock.
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

    /// A line truncates rather than overflowing its buffer.
    #[test]
    fn a_line_truncates_rather_than_overflowing_its_buffer() {
        // The buffer is fixed: losing the tail of a message is survivable, writing past it is not.
        let mut line = Line::new();
        for _ in 0..200 {
            line.text("some text that is long enough to overrun");
        }
        assert_eq!(line.as_bytes().len(), Line::CAPACITY);
    }
}

/// Lists the run's opening calls, in order, with where each was made from.
///
/// The head of the sequence, where the trace keeps the tail: the position is the call ordinal, so
/// it lines up with the mapping record when two runs diverge.
pub(crate) fn opening_calls() {
    use std::fmt::Write as _;

    if !orbistoun_env::TRACE_CALLS.is_set() {
        return;
    }
    let calls = orbistoun_thunk::opening_sequence();
    if calls.is_empty() {
        tracing::info!(
            "the guest made no call at all, which is a finding rather than an empty list"
        );
        return;
    }
    let mut said = format!("its opening {} call(s):", calls.len());
    for (sequence, index, from) in &calls {
        let _ = write!(
            said,
            "\n  call {sequence:<6} {:<52} from {from:#x}",
            label_of(*index as usize).unwrap_or("unknown")
        );
    }
    tracing::info!("{said}");
}
#[cfg(test)]
mod pointee_tests {
    // `printable_text` is part of the Windows-only fault reporter, so its import and test are
    // gated; the breakpoint tests below are cross-platform.
    #[cfg(windows)]
    use super::printable_text;

    /// A terminated run of printable characters is text; everything else is bytes.
    ///
    /// A quoted string in a fault dump has to be a string: `"None"` passes, and a pointer whose
    /// first two bytes are printable does not. It cannot tell a short string from a four-byte
    /// integer whose bytes are printable followed by a zero.
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

    /// A breakpoint is called stub padding only inside the stub table.
    #[test]
    fn a_breakpoint_is_only_called_stub_padding_where_the_stubs_are() {
        use orbistoun_report::trace::FaultSite;

        // Inside the table: stub padding.
        assert_eq!(
            super::breakpoint_kind_in(0x1_0040, 0x1_0000, 0x1000),
            FaultSite::BREAKPOINT_IN_STUBS
        );
        // Below the table and above it: not stub padding, at both edges.
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
        // No table registered: the question could not be asked, so "not stub padding" would
        // overstate.
        assert_eq!(
            super::breakpoint_kind_in(0x1_0040, 0, 0),
            FaultSite::BREAKPOINT_UNPLACED
        );
    }

    /// Every breakpoint kind is listed as an instruction address.
    #[test]
    fn every_breakpoint_kind_is_listed_as_an_instruction_address() {
        // Consumers read these to decide whether the address is somewhere the guest touched; a
        // missing kind would be classified by falling through.
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
/// A stub slot, a label, a call counter and a binding are keyed by one index but built from
/// different counts; when they disagree, a call is attributed to the wrong import (D490). Printed
/// only when they disagree, so the line is not skipped as routine.
pub(crate) fn tables_disagree() {
    let labels = IMPORT_LABELS.get().map_or(0, Vec::len);
    let counters = orbistoun_thunk::call_counts().len();
    if labels == counters {
        return;
    }
    tracing::warn!(
        concat!(
            "the tables an import index is used against are different lengths - ",
            "{} label(s), {} call counter(s)\n",
            "  an index past the shortest of those names one import and counts another"
        ),
        labels,
        counters
    );
}
/// Says so when a bound import was called through a stub anyway.
///
/// An import bound into a placed module is called inside that module, so a bound import with stub
/// calls is a contradiction between two things this run believes (D640).
pub(crate) fn bound_yet_called() {
    let offenders = bound_but_called();
    if offenders.is_empty() {
        return;
    }
    let total: u64 = offenders.iter().map(|(_, calls)| *calls).sum();
    let mut said = format!(
        concat!(
            "{} import(s) were bound into a module this title ships and were called ",
            "through a stub anyway, {} time(s) - the binding did not reach the relocation"
        ),
        offenders.len(),
        total
    );
    for (label, calls) in offenders.iter().take(6) {
        let _ = std::fmt::Write::write_fmt(&mut said, format_args!("\n  {label}, {calls} call(s)"));
    }
    tracing::warn!("{said}");
}

#[cfg(all(test, windows))]
mod host_module_tests {
    use super::host_module_of;

    /// An address inside this binary is named as it, at an offset from the module's own base, which
    /// is what makes the name reproducible across reboots.
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
        // Reconstructing the address from the answer gives the address back, so `name+offset` names
        // this place and no other.
        let (_, again) = host_module_of(here).expect("stable across calls");
        assert_eq!(offset, again, "the same address answers the same offset");

        // A second address in the same module differs by exactly the distance between them.
        let other = host_module_of as *const () as usize as u64;
        let (other_name, other_offset) = host_module_of(other).expect("also in this binary");
        assert_eq!(other_name, name, "both are in this binary");
        assert_eq!(
            other_offset.abs_diff(offset),
            other.abs_diff(here),
            "offsets keep the distance the addresses had"
        );
    }

    /// A fault report says whose protection a page has: a guarded page is named no-access with its
    /// guard listed; the same page released reads read-write with guard and release listed; a page
    /// no guard touched says so.
    #[test]
    fn a_fault_report_names_a_page_guards_protection_and_history() {
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        };
        let text = |address| {
            let mut line = super::Line::new();
            super::note_page(&mut line, address);
            String::from_utf8_lossy(line.as_bytes()).into_owned()
        };
        // SAFETY: a fresh page at an address of the host's choosing, released below.
        let page = unsafe {
            VirtualAlloc(
                std::ptr::null(),
                0x1000,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        assert!(!page.is_null());
        let base = page as usize as u64;
        // A page no guard is ever put on (this test's own stack) is said to be untouched. The page
        // allocated above may not be: the host reuses addresses, and another test may have guarded
        // the same range.
        let on_stack = 0u64;
        let untouched = text(std::ptr::from_ref(&on_stack) as usize as u64);
        assert!(
            untouched.contains("read-write") && untouched.contains("never changed this page"),
            "{untouched}"
        );
        let had = crate::page_guard::guard(base, 0x1000).expect("guarded");
        let guarded = text(base);
        assert!(
            guarded.contains("committed private, no access") && guarded.contains(" to no access"),
            "{guarded}"
        );
        assert!(crate::page_guard::release(base, 0x1000, had));
        let released = text(base);
        assert!(
            released.contains(", read-write") && released.contains(" to read-write"),
            "{released}"
        );
        // SAFETY: the page is this test's and nothing refers to it now.
        unsafe { VirtualFree(page, 0, MEM_RELEASE) };
    }

    /// Nothing is invented for an address that is in no module: a name would be a fabricated
    /// location in a record read as a measurement.
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
