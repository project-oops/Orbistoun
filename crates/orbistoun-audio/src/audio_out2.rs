//! `libSceAudioOut2` - the second-generation audio output interface, beside `libSceAudioOut`.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceAudioOut2Initialize`, the user calls, the
//! context set-up (`ContextResetParam`, `ContextQueryMemory`, `ContextCreate`), `PortCreate` and
//! `GetSpeakerArrayMemorySize` are implemented, as measured.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

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
        // (speakers, height, order)
        "sceAudioOut2GetSpeakerArrayMemorySize" => 3,
        // (info, flag)
        "sceAudioOut2GetSpeakerInfo" => 2,
        "sceAudioOut2Initialize" => 6,
        // (context, param, port)
        "sceAudioOut2PortCreate" => 3,
        // (port)
        "sceAudioOut2PortDestroy" => 1,
        // (port, state)
        "sceAudioOut2PortGetState" => 2,
        // (port, attributes, count)
        "sceAudioOut2PortSetAttributes" => 3,
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

/// PPSA21564's parameter set and its need: u32s 0x14, 0x200, 0, 1, 0x200, 1 need 0x78e9c bytes
/// (REQ-ac01, `report-1791826929.txt` 189-194), and make a process's first context
/// (REQ-ac02, `report-1791833977.txt` 30-66).
const TITLE_NEED: ([u32; 6], u64) = ([0x14, 0x200, 0, 1, 0x200, 1], 0x78e9c);

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
    let words = if param == 0 { None } else { read_param(param) };
    let Some(need) = [MEASURED_NEED, TITLE_NEED]
        .into_iter()
        .find(|(set, _)| Some(*set) == words)
        .map(|(_, need)| need)
    else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if size == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(size, need) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Which parameter set made the process's first context: [`NO_CONTEXT`], [`MEASURED_SET`] or
/// [`TITLE_SET`]. What ports a context takes depends on it (REQ-ao07 against REQ-ac02).
static CONTEXT_SET: AtomicU8 = AtomicU8::new(NO_CONTEXT);
/// No context has been made.
const NO_CONTEXT: u8 = 0;
/// [`MEASURED_NEED`]'s set made it.
const MEASURED_SET: u8 = 1;
/// [`TITLE_NEED`]'s set made it.
const TITLE_SET: u8 = 2;

/// `sceAudioOut2ContextCreate(param, memory, size, context)`: the first context in a process,
/// created in memory of at least the need [`context_query_memory`] answered, written to `context`
/// as the 8-byte handle 0, answering 0 (REQ-ao04, `20261010-231900-eboot.obs.log` 30-56). A
/// second context, other parameters or too little memory are unmeasured and answer the
/// placeholder.
pub(crate) fn context_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, memory, size, context) = (args[0], args[1], args[2], args[3]);
    let words = if param == 0 { None } else { read_param(param) };
    let Some((set, need)) = [(MEASURED_SET, MEASURED_NEED), (TITLE_SET, TITLE_NEED)]
        .into_iter()
        .find(|(_, (measured, _))| Some(*measured) == words)
        .map(|(set, (_, need))| (set, need))
    else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if memory == 0 || context == 0 || size < need {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if CONTEXT_SET
        .compare_exchange(NO_CONTEXT, set, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(context, 0) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Whether `context` is the context `sceAudioOut2ContextCreate` made, the only one a process has
/// here: handle 0.
fn is_made_context(context: u64) -> bool {
    context == 0 && CONTEXT_SET.load(Ordering::Acquire) != NO_CONTEXT
}

/// The handles measured refused as no context: 1 and -1 (REQ-24b2).
fn is_refused_context(context: u64) -> bool {
    context == 1 || context == u64::MAX
}

/// `sceAudioOut2ContextAdvance(context)`: 0 on the context made, twice running as well, and
/// `0x80268001` for 1 and -1 (REQ-24b2, `report-1791836505.txt` 93-97). Any other handle is
/// unmeasured and answers the placeholder.
pub(crate) fn context_advance(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let context = args[0];
    if is_refused_context(context) {
        return INVALID_ARGUMENT;
    }
    if is_made_context(context) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceAudioOut2ContextGetQueueLevel(context, queued, available)`: on the context made, writes 0
/// to `queued` and 2 to `available`, each a u32 over the low half of its slot, and answers 0; a
/// null slot is skipped, both null answer `0x80268001`, as do contexts 1 and -1 (REQ-24b2,
/// `report-1791836505.txt` 85-92). 0 and 2 are the level before any push and the level the
/// measured pushes settled back to (0 and 2 after the third); the brief levels just after a
/// push are not modelled (assumed). Any other handle is unmeasured and answers the placeholder.
pub(crate) fn context_get_queue_level(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (context, queued, available) = (args[0], args[1], args[2]);
    if is_refused_context(context) || (queued == 0 && available == 0) {
        return INVALID_ARGUMENT;
    }
    if !is_made_context(context) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    for (slot, level) in [(queued, 0_u32), (available, 2)] {
        // SAFETY: the guest's out-parameter, a u32, by the call's contract.
        if slot != 0 && !unsafe { guest::write_u32(slot, level) } {
            return u64::from(GuestError::Unimplemented.as_raw());
        }
    }
    0
}

/// `sceAudioOut2ContextPush(context, flag)`: 0 on the context made for flag 0 (not blocking) and
/// flag 1 (blocking), returning within microseconds either way as measured - no grain is paced -
/// and `0x80268001` for flag 2 and for contexts 1 and -1 (REQ-24b2, `report-1791836505.txt`
/// 98-164). Other flags and handles are unmeasured and answer the placeholder.
pub(crate) fn context_push(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (context, flag) = (args[0], args[1] as u32);
    if is_refused_context(context) || (is_made_context(context) && flag == 2) {
        return INVALID_ARGUMENT;
    }
    if is_made_context(context) && flag <= 1 {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The next port `sceAudioOut2PortCreate` gives: a fresh process's ports were numbered from 0 in the
/// order they were made (REQ-ao05, `report-1791713884.txt` 54-99).
static NEXT_PORT: AtomicU64 = AtomicU64::new(0);

/// The port parameters whose creation is measured, as u32s from +0x00: 0, the channels shifted
/// left by 8, the rate and the port type ([`PORT_TYPES`]), at 48000 Hz, with an 8-byte user handle
/// at +0x10 (REQ-ao05, REQ-ao07). PPSA04263 rounds a request for 6 channels up to 8 before it calls.
const PORT_RATE: u32 = 48_000;

/// What `sceAudioOut2PortCreate` answered for each channel count and port type (the word at +0x0c)
/// measured, in a fresh process after a context and a user (REQ-ao07, `report-1791758178.txt`
/// 54-82): `0` where a port was made, and the refusal otherwise.
const PORT_TYPES: [((u32, u32), u64); 7] = [
    ((2, 2), 0),
    ((8, 2), 0),
    ((8, 3), 0),
    ((8, 1), 0),
    ((8, 4), 0x8026_8001),
    ((8, 0), 0x8026_8002),
    ((2, 3), 0x8026_8002),
];

/// What `sceAudioOut2PortCreate` answered on a context [`TITLE_NEED`]'s set made, by channel count
/// and port type (REQ-ac02, `report-1791833977.txt` 56-61): eight channels of type 0 and of type 2
/// each made a port.
const TITLE_PORT_TYPES: [((u32, u32), u64); 2] = [((8, 0), 0), ((8, 2), 0)];

/// The parameter words PPSA04263 makes its pool of mono ports with: `0x100`, one channel, 48000 Hz
/// and port type 0 (REQ-ap01, `report-1791818993.txt` 440-573).
const MONO_PORT: [u32; 4] = [0x100, 1 << 8, PORT_RATE, 0];

/// The next mono port: the console numbered thirty-six of them `0x80000000` to `0x80000023`, in the
/// order they were made, apart from the ports [`NEXT_PORT`] numbers (REQ-ap01).
static NEXT_MONO_PORT: AtomicU64 = AtomicU64::new(0x8000_0000);

/// The ports made and not yet destroyed, and those destroyed: `sceAudioOut2PortDestroy` answers
/// each differently.
static PORTS: Mutex<(BTreeSet<u64>, BTreeSet<u64>)> =
    Mutex::new((BTreeSet::new(), BTreeSet::new()));

/// What a port's parameters ask for: one of the measured kinds, or `None` where unmeasured.
enum PortParams {
    /// Numbered from zero, answering [`PORT_TYPES`]' answer.
    Numbered(u64),
    /// A [`MONO_PORT`], numbered from `0x80000000`.
    Mono,
}

/// What `param`'s port parameters are, for a user `sceAudioOut2UserCreate` gave, or `None` where
/// the parameters or the user are unmeasured.
fn measured_port(param: u64, set: u8) -> Option<PortParams> {
    let mut words = [0_u32; 4];
    for (i, word) in words.iter_mut().enumerate() {
        // SAFETY: the guest's parameter block, by the call's contract.
        *word = unsafe { guest::read_u32(param.wrapping_add(i as u64 * 4)) }?;
    }
    // SAFETY: the same block's user handle, at +0x10.
    let user = unsafe { guest::read_u64(param.wrapping_add(0x10)) }?;
    let known_user = USERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&user);
    if !known_user {
        return None;
    }
    if words == MONO_PORT {
        return Some(PortParams::Mono);
    }
    let [first, layout, rate, fourth] = words;
    if first != 0 || rate != PORT_RATE || layout & 0xff != 0 {
        return None;
    }
    let channels = layout >> 8;
    let table: &[((u32, u32), u64)] = if set == TITLE_SET {
        &TITLE_PORT_TYPES
    } else {
        &PORT_TYPES
    };
    table
        .iter()
        .find(|((measured, kind), _)| *measured == channels && *kind == fourth)
        .map(|&(_, answer)| PortParams::Numbered(answer))
}

/// `sceAudioOut2PortCreate(context, param, port)`: on the context `sceAudioOut2ContextCreate` made,
/// with measured parameters, writes the next port number to the 8-byte `port` and answers 0
/// (REQ-ao05, `report-1791713884.txt` 54-99: 2 channels then 8 gave ports 0 and 1); a mono port
/// takes the next of its own numbers, from `0x80000000` (REQ-ap01). Other parameters, contexts and
/// users are unmeasured and answer the placeholder.
pub(crate) fn port_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (context, param, port) = (args[0], args[1], args[2]);
    let set = CONTEXT_SET.load(Ordering::Acquire);
    let made = set != NO_CONTEXT;
    // Eight channels of type 0 or 2 on a handle no context was made as are refused, writing
    // nothing (REQ-ac01, `report-1791826929.txt` 207-212).
    if (!made || context != 0) && port != 0 && eight_channels_of_type(param, &[0, 2]) {
        return INVALID_ARGUMENT;
    }
    if !made || context != 0 || param == 0 || port == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if set == TITLE_SET
        && eight_channels_of_type(param, &[0, 2])
        && let Some(answer) = indexed_port(param, port)
    {
        return answer;
    }
    let number = match measured_port(param, set) {
        None => return u64::from(GuestError::Unimplemented.as_raw()),
        Some(PortParams::Numbered(0)) => NEXT_PORT.fetch_add(1, Ordering::Relaxed),
        Some(PortParams::Numbered(refused)) => return refused,
        Some(PortParams::Mono) => NEXT_MONO_PORT.fetch_add(1, Ordering::Relaxed),
    };
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(port, number) } {
        PORTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
            .insert(number);
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// What PPSA21564's context answered for an eight-channel port by its parameters' first word
/// (REQ-ac03, `report-1791839056.txt` 76-123): 0 and 1 make a port, its handle the word in the
/// upper half over the process's count of ports made; 2 and 3 answer `0x8026800e`, 7 `0x80268011`
/// and 0x100 `0x80268001`, the port written -1. [`None`] for any other word, which is unmeasured.
fn indexed_port(param: u64, port: u64) -> Option<u64> {
    // SAFETY: the guest's port parameter block, by the call's contract.
    let index = unsafe { guest::read_u32(param) }?;
    let refused = match index {
        0 | 1 => {
            let handle = (u64::from(index) << 16) | NEXT_PORT.fetch_add(1, Ordering::Relaxed);
            // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
            if !unsafe { guest::write_u64(port, handle) } {
                return Some(u64::from(GuestError::Unimplemented.as_raw()));
            }
            PORTS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0
                .insert(handle);
            return Some(0);
        }
        2 | 3 => PORT_INDEX_REFUSED,
        7 => PORT_INDEX_OUT_OF_RANGE,
        0x100 => INVALID_ARGUMENT,
        _ => return None,
    };
    // SAFETY: as above.
    if unsafe { guest::write_u64(port, u64::MAX) } {
        Some(refused)
    } else {
        Some(u64::from(GuestError::Unimplemented.as_raw()))
    }
}

/// What a first word of 2 or 3 was answered on PPSA21564's context (REQ-ac03).
const PORT_INDEX_REFUSED: u64 = 0x8026_800e;
/// What a first word of 7 was answered there (REQ-ac03).
const PORT_INDEX_OUT_OF_RANGE: u64 = 0x8026_8011;

/// Whether the port parameters at `param` are eight channels at 48000 Hz of one of `types`.
fn eight_channels_of_type(param: u64, types: &[u32]) -> bool {
    if param == 0 {
        return false;
    }
    let word = |at: u64| {
        // SAFETY: the guest's port parameter block, by the call's contract.
        unsafe { guest::read_u32(param.wrapping_add(at)) }
    };
    word(4) == Some(8 << 8)
        && word(8) == Some(PORT_RATE)
        && word(12).is_some_and(|port_type| types.contains(&port_type))
}

/// `SCE_AUDIO_OUT2_ERROR_INVALID_ARGUMENT`: what a port of `-1`, the measured attribute records and
/// a port on no context were answered.
const INVALID_ARGUMENT: u64 = 0x8026_8001;

/// What a port destroyed already was answered (REQ-ap01).
const INVALID_HANDLE: u64 = 0x8026_8009;

/// `sceAudioOut2PortDestroy(port)`: 0 for a port the process made, `0x80268009` for one it has
/// destroyed and `0x80268001` for `-1` (REQ-ap01, `report-1791818993.txt` 440-573). Any other
/// number is unmeasured and answers the placeholder.
pub(crate) fn port_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let port = args[0];
    if port == u64::MAX {
        return INVALID_ARGUMENT;
    }
    let mut ports = PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ports.0.remove(&port) {
        ports.1.insert(port);
        0
    } else if ports.1.contains(&port) {
        INVALID_HANDLE
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The attribute records `sceAudioOut2PortSetAttributes` was measured with, `(id, size)` each:
/// PPSA04263's two shapes, read off its calls (REQ-ap01).
const MEASURED_ATTRIBUTES: [&[(u32, u64)]; 2] = [&[(0, 8)], &[(0, 8), (8, 4), (4, 4)]];

/// `sceAudioOut2PortSetAttributes(port, attributes, count)`: for the measured record shapes,
/// `{ u32 id; u32 pad; u64 value; u64 size }` each, `0x80268001`, with nothing of the values
/// written, on a port the process made and on `-1` alike (REQ-ap01, `report-1791818993.txt`
/// 440-573: values of zero). Any other shape is unmeasured and answers the placeholder.
pub(crate) fn port_set_attributes(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (records, count) = (args[1], args[2]);
    let shape: Option<Vec<(u32, u64)>> = (0..count.min(8))
        .map(|i| {
            let at = records.wrapping_add(i * 0x18);
            // SAFETY: the guest's attribute records, by the call's contract.
            let id = unsafe { guest::read_u32(at) }?;
            // SAFETY: the same record's size, at +0x10.
            let size = unsafe { guest::read_u64(at.wrapping_add(0x10)) }?;
            Some((id, size))
        })
        .collect();
    match shape {
        Some(shape) if MEASURED_ATTRIBUTES.contains(&shape.as_slice()) => INVALID_ARGUMENT,
        _ => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// The bytes `sceAudioOut2PortGetState` writes (D799): the port state a console reported for its
/// port in a fresh process (REQ-ao07, `report-1791758178.txt` 77-86), then zero to 0x40.
const PORT_STATE: [u8; 0x40] = {
    let mut state = [0_u8; 0x40];
    let measured = [0x81, 0, 2, 0, 0xff, 0xff, 5, 0];
    let mut i = 0;
    while i < measured.len() {
        state[i] = measured[i];
        i += 1;
    }
    state
};

/// The 16 bytes `sceAudioOut2GetSpeakerInfo` wrote first: byte 0 zero and the word at +4 three,
/// the rest zero, for flag 0 and 1, with and without a port made (REQ-so01,
/// `report-1791801495.txt` 54-61, 99-104).
const SPEAKER_INFO: [u8; 16] = [0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// `sceAudioOut2GetSpeakerInfo(info, flag)`: 0, with [`SPEAKER_INFO`] written. The console changed
/// 0x40 bytes of the probe's block, of which only the first 16 were read back; the rest are
/// unmeasured, and only the 16 are written - PPSA04263's block is 0x10 bytes, the title's own
/// fields after it. A null block is unmeasured and answers the placeholder.
pub(crate) fn get_speaker_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let info = args[0];
    if info == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's info block, by the call's contract.
    if unsafe { guest::write_bytes(info, &SPEAKER_INFO) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceAudioOut2PortGetState(port, state)`: for a port the process made, writes [`PORT_STATE`] and
/// answers 0 (D799). A port it did not make, or a null record, is unmeasured and answers the
/// placeholder with nothing written.
pub(crate) fn port_get_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (port, state) = (args[0], args[1]);
    if state == 0 || port >= NEXT_PORT.load(Ordering::Relaxed) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's state record, by the call's contract.
    if unsafe { guest::write_bytes(state, &PORT_STATE) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The bytes `sceAudioOut2GetSpeakerArrayMemorySize` answered for each argument triple measured,
/// the same twice each (REQ-ao06, `report-1791727748.txt` 92-124, link-bound in a fresh process
/// after the context, user and an 8-channel port were made). PPSA04263 asks for `(8, 0, 1)`.
const SPEAKER_ARRAY_SIZES: [([u32; 3], u64); 6] = [
    ([8, 0, 1], 0xae00),
    ([2, 0, 1], 0xad40),
    ([6, 0, 1], 0xada0),
    ([8, 1, 1], 0xb380),
    ([8, 0, 0], 0x2e0),
    ([8, 0, 2], 0xae00),
];

/// `sceAudioOut2GetSpeakerArrayMemorySize(a, b, c)`: the bytes a speaker array of that shape needs,
/// answered in `rax` (not a status). Answered for the measured triples only; any other is
/// unmeasured and answers the placeholder.
pub(crate) fn get_speaker_array_memory_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let asked = [args[0] as u32, args[1] as u32, args[2] as u32];
    SPEAKER_ARRAY_SIZES
        .iter()
        .find(|(triple, _)| *triple == asked)
        .map_or(
            u64::from(GuestError::Unimplemented.as_raw()),
            |(_, size)| *size,
        )
}
