//! `libSceVoice` - voice chat ports.
//!
//! The names come from real import tables (D504). `sceVoiceInit`, its thread parameters and the
//! ports are implemented as measured in the shapes PPSA04263 calls them, on a console with no
//! headset (REQ-gt02, gt10, gt13).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;

guest_module! {
    "libSceVoice" {
        // (param, version)
        "sceVoiceInit" => 2,
        // (params)
        "sceVoiceSetThreadsParams" => 1,
        // (port id out, param)
        "sceVoiceCreatePort" => 2,
        // (input port, output port)
        "sceVoiceConnectIPortToOPort" => 2,
        // (port, info)
        "sceVoiceGetPortInfo" => 2,
        "sceVoiceStart" => 0,
        "sceVoiceStop" => 0,
        // (port)
        "sceVoiceDeletePort" => 1,
    }
}

/// What a second `sceVoiceInit` answered (REQ-gt02, `20261016-000300-eboot.log` 4915).
const ALREADY_INITIALISED: u64 = 0x804e_0802;

/// Whether `sceVoiceInit` has answered 0 in this process.
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// `sceVoiceInit(param, version)`: 0 the first time and `0x804e0802` after, the parameter block left
/// as it was, for PPSA04263's shape - a 0x20-byte block holding `0x10000000` at +0, version 100
/// (REQ-gt02, `000-boot/gt02-voice`, lines 4913-4916). A zeroed block was refused with `0x804e0805`
/// (REQ-gt01); any shape but the measured one, and a null block, answer the placeholder.
pub(crate) fn voice_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let mut param = [0_u8; 0x20];
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    if args[0] == 0 || !unsafe { orbistoun_mem::guest::read_into(args[0], &mut param) } {
        return placeholder;
    }
    let measured_shape = args[1] == 100
        && param[..4] == 0x1000_0000_u32.to_le_bytes()
        && param[4..].iter().all(|&b| b == 0);
    if !measured_shape {
        return placeholder;
    }
    if INITIALISED.swap(true, Ordering::Relaxed) {
        ALREADY_INITIALISED
    } else {
        0
    }
}

/// The thread parameters PPSA04263 passes `sceVoiceSetThreadsParams`: three priority-and-mask
/// pairs, 0x38 bytes (REQ-gt10, as measured).
const PPSA04263_THREADS_PARAMS: [u8; 0x38] = [
    0x01, 0x00, 0x00, 0x00, 0xbc, 0x02, 0x00, 0x00, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xbc, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xbc, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `sceVoiceSetThreadsParams(params)`: 0, the block left as it was, for PPSA04263's 0x38-byte block
/// and for a zeroed one, after `sceVoiceInit` (REQ-gt10, `000-boot/gt10-socket-voice`,
/// `report-20261018-gt-census.log` rows 2803-2808; gt07's rows 2763-2765 agree). Before
/// initialisation, and any other block, is unmeasured and answers the placeholder.
pub(crate) fn voice_set_threads_params(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let mut params = [0_u8; 0x38];
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    if args[0] == 0 || !unsafe { orbistoun_mem::guest::read_into(args[0], &mut params) } {
        return placeholder;
    }
    let measured = params == PPSA04263_THREADS_PARAMS || params.iter().all(|&b| b == 0);
    if measured && INITIALISED.load(Ordering::Relaxed) {
        0
    } else {
        placeholder
    }
}

/// Where a port's parameter block holds the signed-in user, in the shapes that carry one.
const PORT_PARAM_USER: std::ops::Range<usize> = 0xc..0x10;

/// The port parameter blocks PPSA04263 builds, as measured (REQ-gt13), each with whether it
/// carries the signed-in user at [`PORT_PARAM_USER`] - the bytes there in the table are the
/// rig's user, 0x1ea2f4d9 - and the port's kind, the first word.
const MEASURED_PORT_PARAMS: [([u8; 0x18], bool); 4] = [
    (
        [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0xd9, 0xf4,
            0xa2, 0x1e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        true,
    ),
    (
        [
            0x04, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x84, 0x1c,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        false,
    ),
    (
        [
            0x03, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x08,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3e,
        ],
        false,
    ),
    (
        [
            0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0xd9, 0xf4,
            0xa2, 0x1e, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        true,
    ),
];

/// The kinds of port connected as input then output, as measured (REQ-gt13): kind 0 to kinds 4
/// and 3.
const MEASURED_CONNECTIONS: [(u32, u32); 2] = [(0, 4), (0, 3)];

/// What `sceVoiceGetPortInfo` answered for every measured port (REQ-gt13), with the first 32
/// bytes of the block zeroed.
const PORT_INFO_REFUSED: u64 = 0x804e_080a;

/// Bytes of the info block `sceVoiceGetPortInfo` zeroed (REQ-gt13); the rest were left.
const PORT_INFO_ZEROED: usize = 0x20;

/// What `sceVoiceStart` answered with no headset connected (REQ-gt13).
const START_REFUSED: u64 = 0x804e_0805;

/// What `sceVoiceDeletePort` answered for a port already deleted (REQ-gt13).
const PORT_DELETED: u64 = 0x804e_0807;

/// The process's voice ports: each created port's kind by id, `None` once deleted. Ids count up
/// from 0 in creation order, as measured.
static PORTS: Mutex<Vec<Option<u32>>> = Mutex::new(Vec::new());

/// The kind of a port parameter block if it is one of the measured shapes, the user field
/// matching the signed-in user where the shape carries one.
fn measured_port_kind(param: &[u8; 0x18]) -> Option<u32> {
    let user = orbistoun_systemservice::signed_in_user().to_le_bytes();
    MEASURED_PORT_PARAMS
        .iter()
        .find(|(shape, carries_user)| {
            let mut expected = *shape;
            if *carries_user {
                expected[PORT_PARAM_USER].copy_from_slice(&user);
            }
            expected == *param
        })
        .map(|(shape, _)| u32::from_le_bytes([shape[0], shape[1], shape[2], shape[3]]))
}

/// The ports, held even if a holder panicked: the list stays consistent between calls.
fn ports() -> std::sync::MutexGuard<'static, Vec<Option<u32>>> {
    PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The kind of live port `port`, if there is one.
fn live_port(port: u64) -> Option<u32> {
    let index = usize::try_from(port).ok()?;
    ports().get(index).copied().flatten()
}

/// `sceVoiceCreatePort(id, param)`: after `sceVoiceInit`, for one of PPSA04263's four parameter
/// blocks, writes the next port id - 0 for the first - as a 32-bit word, leaves the block as it
/// was and answers 0 (REQ-gt13, `000-boot/gt13-voice-ports`, `report-20261018-gt13-census.log`
/// rows 2876-2921). Any other block, or a call before initialisation, answers the placeholder.
pub(crate) fn voice_create_port(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let (out, at) = (args[0], args[1]);
    let mut param = [0_u8; 0x18];
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    if out == 0 || at == 0 || !unsafe { orbistoun_mem::guest::read_into(at, &mut param) } {
        return placeholder;
    }
    let Some(kind) = measured_port_kind(&param) else {
        return placeholder;
    };
    if !INITIALISED.load(Ordering::Relaxed) {
        return placeholder;
    }
    let mut ports = ports();
    let Ok(id) = u32::try_from(ports.len()) else {
        return placeholder;
    };
    // SAFETY: the guest's port id word, by the call's contract; written through the checked
    // accessor.
    if !unsafe { orbistoun_mem::guest::write_u32(out, id) } {
        return placeholder;
    }
    ports.push(Some(kind));
    0
}

/// `sceVoiceConnectIPortToOPort(input, output)`: 0 for two live ports of the kinds measured
/// connected - kind 0 to kind 4 or 3 (REQ-gt13). Any other pair answers the placeholder.
pub(crate) fn voice_connect_port(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let pair = live_port(args[0]).zip(live_port(args[1]));
    if pair.is_some_and(|pair| MEASURED_CONNECTIONS.contains(&pair)) {
        0
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// `sceVoiceGetPortInfo(port, info)`: for a live port, zeroes the first 32 bytes of `info` and
/// answers `0x804e080a`, as the console did for every port with no headset connected (REQ-gt13).
/// A port that is not live answers the placeholder.
pub(crate) fn voice_get_port_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    if args[1] == 0 || live_port(args[0]).is_none() {
        return placeholder;
    }
    // SAFETY: the guest's info block, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_bytes(args[1], &[0; PORT_INFO_ZEROED]) } {
        PORT_INFO_REFUSED
    } else {
        placeholder
    }
}

/// `sceVoiceStart()`: `0x804e0805` after `sceVoiceInit`, as with no headset connected
/// (REQ-gt13); before it, the placeholder.
pub(crate) fn voice_start(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALISED.load(Ordering::Relaxed) {
        START_REFUSED
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// `sceVoiceStop()`: 0 after `sceVoiceInit` (REQ-gt13); before it, the placeholder.
pub(crate) fn voice_stop(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALISED.load(Ordering::Relaxed) {
        0
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// `sceVoiceDeletePort(port)`: 0 for a live port, which is then gone, and `0x804e0807` for one
/// already deleted (REQ-gt13). An id never created answers the placeholder.
pub(crate) fn voice_delete_port(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut ports = ports();
    let Some(slot) = usize::try_from(args[0]).ok().and_then(|i| ports.get_mut(i)) else {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    };
    if slot.take().is_some() {
        0
    } else {
        PORT_DELETED
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// PPSA04263's `sceVoiceInit(param, 100)` answers 0, then `0x804e0802`, the block left; a
    /// shape not measured answers the placeholder (REQ-gt02).
    #[test]
    fn voice_initialises_once_as_measured() {
        let mut param = [0_u8; 0x20];
        param[..4].copy_from_slice(&0x1000_0000_u32.to_le_bytes());
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = param.as_ptr() as u64;
        args[1] = 100;
        assert_eq!(super::voice_init(&args), 0);
        assert_eq!(super::voice_init(&args), 0x804e_0802);
        assert_eq!(&param[..4], &0x1000_0000_u32.to_le_bytes());
        args[1] = 1;
        assert_ne!(super::voice_init(&args), 0, "an unmeasured version");

        // Once initialised, the thread parameters: PPSA04263's block and a zeroed one answer 0,
        // left as they were; another block answers the placeholder (REQ-gt10).
        let mut params = super::PPSA04263_THREADS_PARAMS;
        let mut set = [0_u64; GUEST_ARG_REGISTERS];
        set[0] = params.as_mut_ptr() as u64;
        assert_eq!(super::voice_set_threads_params(&set), 0);
        assert_eq!(params, super::PPSA04263_THREADS_PARAMS, "left as it was");
        let mut zeroed = [0_u8; 0x38];
        set[0] = zeroed.as_mut_ptr() as u64;
        assert_eq!(super::voice_set_threads_params(&set), 0);
        let mut other = [9_u8; 0x38];
        set[0] = other.as_mut_ptr() as u64;
        assert_ne!(
            super::voice_set_threads_params(&set),
            0,
            "an unmeasured block"
        );

        // The ports, as with no headset connected (REQ-gt13): PPSA04263's four blocks, the
        // signed-in user where they carry one, make ports 0-3, the blocks left as they were.
        let user = orbistoun_systemservice::signed_in_user().to_le_bytes();
        let mut ids = [0xcccc_cccc_u32; 4];
        for (n, (shape, carries_user)) in super::MEASURED_PORT_PARAMS.iter().enumerate() {
            let mut param = *shape;
            if *carries_user {
                param[super::PORT_PARAM_USER].copy_from_slice(&user);
            }
            let before = param;
            let mut create = [0_u64; GUEST_ARG_REGISTERS];
            create[0] = (&raw mut ids[n]) as u64;
            create[1] = param.as_mut_ptr() as u64;
            assert_eq!(super::voice_create_port(&create), 0, "port {n}");
            assert_eq!(param, before, "left as it was");
        }
        assert_eq!(ids, [0, 1, 2, 3]);
        let mut other = super::MEASURED_PORT_PARAMS[1].0;
        other[4] = 9;
        let mut create = [0_u64; GUEST_ARG_REGISTERS];
        create[0] = (&raw mut ids[0]) as u64;
        create[1] = other.as_mut_ptr() as u64;
        assert_ne!(super::voice_create_port(&create), 0, "an unmeasured block");
        assert_eq!(ids[0], 0, "no id written");

        let pair = |input: u64, output: u64| {
            let mut connect = [0_u64; GUEST_ARG_REGISTERS];
            connect[0] = input;
            connect[1] = output;
            super::voice_connect_port(&connect)
        };
        assert_eq!(pair(0, 1), 0);
        assert_eq!(pair(0, 2), 0);
        assert_ne!(pair(1, 0), 0, "an unmeasured pair");
        assert_ne!(pair(0, 9), 0, "no such port");

        let mut info = [0xcc_u8; 0x40];
        let mut get = [0_u64; GUEST_ARG_REGISTERS];
        get[0] = 3;
        get[1] = info.as_mut_ptr() as u64;
        assert_eq!(super::voice_get_port_info(&get), 0x804e_080a);
        assert_eq!(info[..0x20], [0; 0x20], "zeroed");
        assert_eq!(info[0x20..], [0xcc; 0x20], "left as it was");

        let none = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::voice_start(&none), 0x804e_0805);
        assert_eq!(super::voice_stop(&none), 0);

        let mut delete = [0_u64; GUEST_ARG_REGISTERS];
        for port in 0..4 {
            delete[0] = port;
            assert_eq!(super::voice_delete_port(&delete), 0, "port {port}");
        }
        delete[0] = 0;
        assert_eq!(
            super::voice_delete_port(&delete),
            0x804e_0807,
            "deleted already"
        );
        delete[0] = 7;
        assert_ne!(super::voice_delete_port(&delete), 0, "never created");
        assert_ne!(super::voice_get_port_info(&get), 0x804e_080a, "deleted");
    }
}
