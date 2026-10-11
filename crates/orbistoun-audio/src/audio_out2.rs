//! `libSceAudioOut2` - the second-generation audio output interface, beside `libSceAudioOut`.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceAudioOut2Initialize`, the user calls, the
//! context set-up (`ContextResetParam`, `ContextQueryMemory`, `ContextCreate`) and `PortCreate` are
//! implemented, as measured.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceAudioOut2" {
        "sceAudioOut2ContextAdvance" => 6,
        "sceAudioOut2ContextCreate" => 6,
        "sceAudioOut2ContextDestroy" => 6,
        "sceAudioOut2ContextGetQueueLevel" => 6,
        "sceAudioOut2ContextPush" => 6,
        "sceAudioOut2ContextQueryMemory" => 6,
        "sceAudioOut2ContextResetParam" => 6,
        "sceAudioOut2Initialize" => 6,
        // (context, param, port)
        "sceAudioOut2PortCreate" => 3,
        "sceAudioOut2PortDestroy" => 6,
        "sceAudioOut2PortSetAttributes" => 6,
        // (user, out)
        "sceAudioOut2UserCreate" => 2,
        // (user)
        "sceAudioOut2UserDestroy" => 1,
    }
}

/// What `sceAudioOut2Initialize` answers once the process has initialised the library.
const ALREADY_INITIALIZED: u64 = 0x8026_8004;

/// Whether the process has initialised the library.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// `sceAudioOut2Initialize(...)`: 0 the first time in a process and `0x80268004` after, writing
/// nothing, with every argument zero or a buffer in the first (REQ-ao03,
/// `20261010-125000-eboot.obs.log` 30-42, a process with no earlier audio use; REQ-ao02's
/// `20261010-115300-eboot.log` 3590-3618, after a first call, answered `0x80268004` throughout).
pub(crate) fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        ALREADY_INITIALIZED
    } else {
        0
    }
}

/// The next handle `sceAudioOut2UserCreate` gives: small and non-zero, not an address (D151). The
/// console's were heap addresses (`0x80067fbf0`, `0x80067fc10`), so no value of them is a contract.
static NEXT_USER: AtomicU64 = AtomicU64::new(1);

/// The user `0xff` names: the primary user, answered the same handle as the signed-in user's id
/// (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56).
const PRIMARY_USER: u64 = 0xff;

/// The handles `sceAudioOut2UserCreate` has given and `sceAudioOut2UserDestroy` not yet taken back.
static USERS: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// `sceAudioOut2UserCreate(user, out)`: for the signed-in user or `0xff`, writes an 8-byte handle to
/// `out` and answers 0, with the module loaded or not (REQ-ao01, `20261010-100600-eboot.obs.log`,
/// the 0x40-byte `out` changed in exactly its first 8 bytes; REQ-ao04 for `0xff`). Any other user,
/// and a null `out`, are unmeasured and answer the placeholder.
pub(crate) fn user_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (user, out) = (args[0], args[1]);
    let signed_in = user as u32 == orbistoun_systemservice::signed_in_user();
    if !(signed_in || user == PRIMARY_USER) || out == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let handle = NEXT_USER.fetch_add(1, Ordering::Relaxed);
    USERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(handle);
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(out, handle) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// What `sceAudioOut2UserDestroy` answered for 0 (REQ-cn10 arm 2, `20261009-151440-eboot.obs.log`)
/// and, mid-sweep, for handles that process had not freshly made (REQ-ao01).
const USER_DESTROY_REFUSED: u64 = 0x8026_8010;

/// `sceAudioOut2UserDestroy(user)`: 0 for a handle `sceAudioOut2UserCreate` gave in this process, as
/// a fresh process answered (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56), and `0x80268010`
/// for anything else.
pub(crate) fn user_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let taken = USERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&args[0]);
    if taken { 0 } else { USER_DESTROY_REFUSED }
}

/// The context parameters `sceAudioOut2ContextResetParam` writes, as u32s from offset 0: 8 at
/// +0x00, 1 at +0x0c, 0x100 at +0x10 (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56).
const RESET_PARAM: [u32; 6] = [8, 0, 0, 1, 0x100, 0];

/// `sceAudioOut2ContextResetParam(param)`: writes the default context parameters and answers 0.
///
/// Only the 0x18 bytes [`RESET_PARAM`] covers are written. The console's row read zeros out to
/// 0x40, but PPSA04263 hands it a 0x18-byte block on its stack and runs, so the larger extent is
/// not taken as the write.
pub(crate) fn context_reset_param(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = args[0];
    if param == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let bytes: Vec<u8> = RESET_PARAM.iter().flat_map(|w| w.to_le_bytes()).collect();
    // SAFETY: the guest's parameter block, by the call's contract.
    if unsafe { guest::write_bytes(param, &bytes) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The one parameter set whose memory need is measured, and that need: u32s 4, 0x48, 0, 2, 0x100, 1
/// need 0x45698 bytes (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56).
const MEASURED_NEED: ([u32; 6], u64) = ([4, 0x48, 0, 2, 0x100, 1], 0x45698);

/// Reads the six u32s of a context parameter block.
fn read_param(param: u64) -> Option<[u32; 6]> {
    let mut words = [0_u32; 6];
    for (i, word) in words.iter_mut().enumerate() {
        // SAFETY: the guest's parameter block, by the call's contract.
        *word = unsafe { guest::read_u32(param.wrapping_add(i as u64 * 4)) }?;
    }
    Some(words)
}

/// `sceAudioOut2ContextQueryMemory(param, size)`: the bytes a context with these parameters needs,
/// written to `size` as 8 bytes. Answered for the measured parameter set only; any other is
/// unmeasured and answers the placeholder.
pub(crate) fn context_query_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, size) = (args[0], args[1]);
    if param == 0 || size == 0 || read_param(param) != Some(MEASURED_NEED.0) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(size, MEASURED_NEED.1) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Whether `sceAudioOut2ContextCreate` has made the process's first context.
static CONTEXT_MADE: AtomicBool = AtomicBool::new(false);

/// `sceAudioOut2ContextCreate(param, memory, size, context)`: the first context in a process,
/// created in memory of at least the need [`context_query_memory`] answered, written to `context`
/// as the 8-byte handle 0, answering 0 (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56). A
/// second context, other parameters or too little memory are unmeasured and answer the
/// placeholder.
pub(crate) fn context_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, memory, size, context) = (args[0], args[1], args[2], args[3]);
    let measured = param != 0 && read_param(param) == Some(MEASURED_NEED.0);
    if !measured || memory == 0 || context == 0 || size < MEASURED_NEED.1 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if CONTEXT_MADE.swap(true, Ordering::AcqRel) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(context, 0) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The next port `sceAudioOut2PortCreate` gives: a fresh process's ports were numbered from 0 in the
/// order they were made (REQ-ao05, `report-1791713884.txt` 54-99).
static NEXT_PORT: AtomicU64 = AtomicU64::new(0);

/// The port parameters whose creation is measured, as u32s from +0x00: 0, the channels shifted
/// left by 8, the rate and 2, for 2 and 8 channels at 48000 Hz, with an 8-byte user handle at +0x10
/// (REQ-ao05). PPSA04263 rounds a request for 6 channels up to 8 before it calls.
const PORT_CHANNELS: [u32; 2] = [2, 8];
const PORT_RATE: u32 = 48_000;

/// Whether `param` holds port parameters as measured, for a user `sceAudioOut2UserCreate` gave.
fn measured_port(param: u64) -> bool {
    let mut words = [0_u32; 4];
    for (i, word) in words.iter_mut().enumerate() {
        // SAFETY: the guest's parameter block, by the call's contract.
        match unsafe { guest::read_u32(param.wrapping_add(i as u64 * 4)) } {
            Some(value) => *word = value,
            None => return false,
        }
    }
    // SAFETY: the same block's user handle, at +0x10.
    let Some(user) = (unsafe { guest::read_u64(param.wrapping_add(0x10)) }) else {
        return false;
    };
    let [first, layout, rate, fourth] = words;
    let known_user = USERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&user);
    first == 0
        && PORT_CHANNELS
            .iter()
            .any(|&channels| layout == channels << 8)
        && rate == PORT_RATE
        && fourth == 2
        && known_user
}

/// `sceAudioOut2PortCreate(context, param, port)`: on the context `sceAudioOut2ContextCreate` made,
/// with measured parameters, writes the next port number to the 8-byte `port` and answers 0
/// (REQ-ao05, `report-1791713884.txt` 54-99: 2 channels then 8 gave ports 0 and 1). Other
/// parameters, contexts and users are unmeasured and answer the placeholder.
pub(crate) fn port_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (context, param, port) = (args[0], args[1], args[2]);
    let made = CONTEXT_MADE.load(Ordering::Acquire);
    if !made || context != 0 || param == 0 || port == 0 || !measured_port(param) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(port, number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}
