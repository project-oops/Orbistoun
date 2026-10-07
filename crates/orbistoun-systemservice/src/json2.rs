//! `libSceJson2` - the platform's JSON parser, reached by mangled C++ names.
//!
//! The library's start-up is implemented as obSCEne measured it, link-imported on hardware
//! (REQ-js02, `reports/hardware/20261007-082134-eboot.obs.log`, `130-layout/json2-init`):
//! `Initializer` keeps its state in byte 0, and `initialize` takes the memory it needs from the
//! guest's own `MemAllocator`, whose vtable it calls. The blocks are the library's internal state,
//! which nothing here reads; they are held only to hand back at `terminate`. Parsing is not
//! implemented.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceJson2" {
        "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE" => 6,
        "_ZN3sce4Json11InitializerC1Ev" => 6,
        "_ZN3sce4Json11Initializer9terminateEv" => 6,
        "_ZN3sce4Json12MemAllocatorC2Ev" => 6,
    }
}

const OK: u64 = 0;

/// `initialize` on an initializer already initialized (`arm3-reinit`).
const ALREADY_INITIALIZED: u64 = 0x8084_8111;

/// The allocations `initialize` makes, in order (`arm2-init`, `alloc-size-0`..`-11`).
const INITIALIZE_SIZES: [u64; 12] = [
    0x18, 0x8, 0x18, 0x8, 0x20, 0x30, 0x8, 0x20, 0x68, 0x20, 0x8, 0x8,
];

/// `MemAllocator`'s vtable slots `initialize` and `terminate` call, by byte offset: the probe's
/// subclass put `allocate` in slot 2 and `deallocate` in slot 3, after the two destructors, and
/// the library called them there.
const ALLOCATE_SLOT: u64 = 2 * 8;
const DEALLOCATE_SLOT: u64 = 3 * 8;

/// The parameter block's word the library passes to every allocation unchanged (`param + 8`, seen
/// as each call's third argument).
const PASS_THROUGH_AT: u64 = 8;

/// What an initialized `Initializer` holds: its allocator, the word passed to it, and the blocks it
/// was given.
struct Session {
    allocator: u64,
    pass: u64,
    blocks: Vec<u64>,
}

fn sessions() -> &'static Mutex<BTreeMap<u64, Session>> {
    static SESSIONS: OnceLock<Mutex<BTreeMap<u64, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// `Initializer::Initializer()`: clears byte 0, the initialized flag, and nothing else
/// (`arm1-ctor`, `ctor_written 00`, extent 1).
pub(crate) fn initializer_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's object, whose first byte the constructor owns.
    unsafe { guest::write_bytes(args[0], &[0]) };
    OK
}

/// The vtable slot `slot` bytes into `allocator`'s vtable.
fn virtual_at(allocator: u64, slot: u64) -> Option<u64> {
    // SAFETY: the guest's allocator object and its vtable, both read through the checked accessor.
    let vtable = unsafe { guest::read_u64(allocator) }?;
    // SAFETY: as above.
    unsafe { guest::read_u64(vtable + slot) }.filter(|&entry| entry != 0)
}

/// `Initializer::initialize(const InitParameter*)`: the twelve measured allocations through the
/// parameter block's allocator, then byte 0 set and `0`. A second answers `0x80848111` and
/// allocates nothing. An allocation that fails hands back what it got and answers the placeholder:
/// what the library answers then is unmeasured.
pub(crate) fn initializer_initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (this, param) = (args[0], args[1]);
    let Ok(mut sessions) = sessions().lock() else {
        return u64::from(GuestError::HostFailed.as_raw());
    };
    if sessions.contains_key(&this) {
        return ALREADY_INITIALIZED;
    }
    // SAFETY: the guest's parameter block, read through the checked accessor.
    let read = |at: u64| unsafe { guest::read_u64(at) };
    let (Some(allocator), Some(pass)) = (read(param), read(param + PASS_THROUGH_AT)) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let (Some(allocate), Some(deallocate)) = (
        virtual_at(allocator, ALLOCATE_SLOT),
        virtual_at(allocator, DEALLOCATE_SLOT),
    ) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let mut blocks = Vec::with_capacity(INITIALIZE_SIZES.len());
    for size in INITIALIZE_SIZES {
        // SAFETY: `allocate` is the guest's own function, from the vtable of the allocator it
        // handed over.
        match unsafe { orbistoun_kernel::thread::call_guest(allocate, [allocator, size, pass]) } {
            Some(block) if block != 0 => blocks.push(block),
            _ => {
                hand_back(deallocate, allocator, pass, &blocks);
                return u64::from(GuestError::Unimplemented.as_raw());
            }
        }
    }
    // SAFETY: the guest's object, whose first byte is the flag.
    unsafe { guest::write_bytes(this, &[1]) };
    sessions.insert(
        this,
        Session {
            allocator,
            pass,
            blocks,
        },
    );
    OK
}

/// Hands each block back through the allocator's `deallocate`.
fn hand_back(deallocate: u64, allocator: u64, pass: u64, blocks: &[u64]) {
    for &block in blocks.iter().rev() {
        // SAFETY: the guest's own function, as `allocate` was; `block` is one it handed out.
        unsafe { orbistoun_kernel::thread::call_guest(deallocate, [allocator, block, pass]) };
    }
}

/// `Initializer::terminate()`: every block back through the allocator, byte 0 cleared, `0`
/// (`arm4-term`, `dealloc-calls 0xc`). One not initialized answers the placeholder: unmeasured.
pub(crate) fn initializer_terminate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let this = args[0];
    let session = sessions().lock().ok().and_then(|mut s| s.remove(&this));
    let Some(session) = session else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if let Some(deallocate) = virtual_at(session.allocator, DEALLOCATE_SLOT) {
        hand_back(deallocate, session.allocator, session.pass, &session.blocks);
    }
    // SAFETY: the guest's object, whose first byte is the flag.
    unsafe { guest::write_bytes(this, &[0]) };
    OK
}

/// The base `MemAllocator`'s vtable: zeroed guest memory, its four slots - two destructors,
/// `allocate`, `deallocate` - empty as a pure virtual's are, since the guest's subclass replaces
/// the pointer as soon as the base constructor returns. Mapped once.
fn base_vtable() -> Option<u64> {
    static AT: OnceLock<Option<u64>> = OnceLock::new();
    *AT.get_or_init(|| {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0x4000;
        args[2] = 0x1; // PROT_READ
        args[3] = 0x1000 | 0x2; // MAP_ANON | MAP_PRIVATE
        args[4] = u64::MAX;
        let at = orbistoun_kernel::mmap(&args);
        (at != u64::MAX && at != 0).then_some(at)
    })
}

/// `MemAllocator::MemAllocator()`: writes the base vtable pointer and nothing more
/// (`MemAllocator`, `base-obj-after`: eight bytes written, the rest untouched).
pub(crate) fn mem_allocator_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(vtable) = base_vtable() else {
        return u64::from(GuestError::NoMemory.as_raw());
    };
    // SAFETY: the guest's object, whose first word is its vtable pointer.
    unsafe { guest::write_u64(args[0], vtable) };
    OK
}
