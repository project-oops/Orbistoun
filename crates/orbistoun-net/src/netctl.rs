//! `libSceNetCtl` - network configuration and link status.
//!
//! `sceNetCtlInit` and `sceNetCtlTerm` bracket the library's use and hold nothing the host
//! network needs. `sceNetCtlGetInfo` reports the host's link ([`crate::host`]) as a wired console
//! reports its own, to the codes, buffer and values obSCEne measured (`-5c1e`, `-9e41`, check
//! `102-net/netctl-info`, sweep 20260927-153242); oops-sdk's `src/net/netctl.c` names the codes.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceNetCtl" {
        "sceNetCtlGetInfo" => 2,
        "sceNetCtlInit" => 0,
        "sceNetCtlTerm" => 0,
        // (fn, arg, &cid)
        "sceNetCtlRegisterCallback" => 3,
        // (cid)
        "sceNetCtlUnregisterCallback" => 1,
        "sceNetCtlCheckCallback" => 0,
        // (&state)
        "sceNetCtlGetState" => 1,
        // (event, &result)
        "sceNetCtlGetResult" => 2,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNetCtlGetInfo", get_info),
        ("sceNetCtlInit", init),
        ("sceNetCtlTerm", term),
        ("sceNetCtlRegisterCallback", register_callback),
        ("sceNetCtlUnregisterCallback", unregister_callback),
        ("sceNetCtlCheckCallback", check_callback),
        ("sceNetCtlGetState", get_state),
        ("sceNetCtlGetResult", get_result),
    ]
}

/// `sceNetCtlInit()`: zero, a second call too (measured).
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNetCtlTerm()`: nothing to release.
fn term(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// Bytes of the information buffer; every measured code wrote all of them (`bytes-written 0x100`).
const INFO_BYTES: usize = 256;

/// What a wired console answers for the wireless codes 5-10 (measured).
const NOT_WIRELESS: u32 = 0x8041_2109;

/// The information codes, as oops-sdk names them where it does; 3 is named by the value it holds.
const DEVICE: u32 = 1;
const ETHER_ADDR: u32 = 2;
const MTU: u32 = 3;
const LINK: u32 = 4;
const IP_ADDRESS: u32 = 14;
const NETMASK: u32 = 15;
const DEFAULT_ROUTE: u32 = 16;
const PRIMARY_DNS: u32 = 17;
const SECONDARY_DNS: u32 = 18;

/// The device a console on a cable reports: `0` (measured, code 1).
const DEVICE_WIRED: u32 = 0;
/// The link state a connected console reports: `1` (measured, code 4).
const LINK_CONNECTED: u32 = 1;

/// The buffer `code` fills from `link`, or the code it is refused with. `None` for a code nothing
/// measured, which leaves the buffer alone.
fn answer(code: u32, link: Option<&crate::host::Link>) -> Option<([u8; INFO_BYTES], u32)> {
    let mut info = [0_u8; INFO_BYTES];
    let refused = u32::from_ne_bytes(GuestError::HostFailed.as_raw().to_ne_bytes());
    let text = |info: &mut [u8; INFO_BYTES], value: String| {
        info[..value.len()].copy_from_slice(value.as_bytes());
    };
    let rc = match (code, link) {
        (DEVICE, _) => {
            info[..4].copy_from_slice(&DEVICE_WIRED.to_le_bytes());
            0
        }
        (5..=10, _) => NOT_WIRELESS,
        // Measured empty on a wired console with its address from DHCP and no proxy.
        (11..=13 | 19 | 20, _) => 0,
        (ETHER_ADDR, Some(link)) => {
            info[..6].copy_from_slice(&link.hardware);
            0
        }
        (MTU, Some(link)) => {
            info[..4].copy_from_slice(&link.mtu.to_le_bytes());
            0
        }
        (LINK, Some(_)) => {
            info[..4].copy_from_slice(&LINK_CONNECTED.to_le_bytes());
            0
        }
        (IP_ADDRESS, Some(link)) => {
            text(&mut info, link.address.to_string());
            0
        }
        (NETMASK, Some(link)) => {
            text(&mut info, link.netmask.to_string());
            0
        }
        (
            DEFAULT_ROUTE,
            Some(crate::host::Link {
                gateway: Some(gateway),
                ..
            }),
        ) => {
            text(&mut info, gateway.to_string());
            0
        }
        (PRIMARY_DNS | SECONDARY_DNS, Some(link)) => {
            match link.dns.get((code - PRIMARY_DNS) as usize) {
                Some(server) => {
                    text(&mut info, server.to_string());
                    0
                }
                None => refused,
            }
        }
        // A measured code whose value the host cannot supply: no link out, or no gateway. The DNS
        // codes too: with no link they fell through to the unmeasured arm and left the buffer
        // alone, which only a host with no link out (the macOS CI runner) showed.
        (2..=4 | 14..=18, _) => refused,
        _ => return None,
    };
    Some((info, rc))
}

/// `sceNetCtlGetInfo(code, info)`: rewrites the 256-byte buffer and answers `code`'s value from the
/// host's link.
fn get_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(code) = u32::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let link = crate::host::link();
    let Some((info, rc)) = answer(code, link.as_ref()) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: the guest's buffer, which the call's contract makes 256 writable bytes.
    if unsafe { guest::write_bytes(args[1], &info) } {
        u64::from(rc)
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// A null callback, id slot, state slot or result slot (measured, `-074b`).
const INVALID_ADDR: u64 = 0x8041_2107;
/// Every callback slot taken (measured: the eighth registration after init).
const CALLBACK_MAX: u64 = 0x8041_2103;
/// An id in range whose slot is free (measured: an id unregistered once already).
const ID_NOT_FOUND: u64 = 0x8041_2104;
/// An id outside the slots (measured: 8 and -1).
const INVALID_ID: u64 = 0x8041_2105;
/// A result for an event that has not happened (measured: events 2 and 3).
const NOT_AVAILABLE: u64 = 0x8041_210f;

/// How many callbacks a process can hold: ids 1 to 7 (measured).
const CALLBACK_SLOTS: usize = 7;

/// The state a console with an address reports: 3, IP obtained (measured).
const STATE_IP_OBTAINED: u32 = 3;

/// The registered callbacks, by id less one: the function and its argument. Nothing calls them:
/// `sceNetCtlCheckCallback` delivered no event on a console whose state did not change.
fn callbacks() -> &'static std::sync::Mutex<[Option<(u64, u64)>; CALLBACK_SLOTS]> {
    static CALLBACKS: std::sync::Mutex<[Option<(u64, u64)>; CALLBACK_SLOTS]> =
        std::sync::Mutex::new([None; CALLBACK_SLOTS]);
    &CALLBACKS
}

/// `sceNetCtlRegisterCallback(fn, arg, &cid)`: the lowest free id from 1 into `cid` and 0;
/// `0x80412103` with `cid` untouched once seven are held; `0x80412107` for a null `fn` or `cid`
/// (obSCEne `REQ-20261012T1822Z-074b`, `000-boot/netctl-callbacks`, `report-1791836505.txt`
/// lines 915-989). A function already registered is taken again: the duplicate arm was measured
/// with every slot already full, so its refusal says nothing about duplicates.
fn register_callback(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [function, argument, cid, ..] = *args;
    if function == 0 || cid == 0 {
        return INVALID_ADDR;
    }
    let mut slots = callbacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(free) = slots.iter().position(Option::is_none) else {
        return CALLBACK_MAX;
    };
    let id = u32::try_from(free + 1).unwrap_or(u32::MAX);
    // SAFETY: the guest's id slot, by the call's contract; written through the checked accessor.
    if !unsafe { guest::write_u32(cid, id) } {
        return INVALID_ADDR;
    }
    slots[free] = Some((function, argument));
    0
}

/// `sceNetCtlUnregisterCallback(cid)`: 0 for a held id, `0x80412104` for a free one, `0x80412105`
/// outside 1 to 7 (measured).
fn unregister_callback(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = args[0] as u32;
    let Some(index) = (id as usize)
        .checked_sub(1)
        .filter(|&index| index < CALLBACK_SLOTS)
    else {
        return INVALID_ID;
    };
    let mut slots = callbacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slots[index].take().is_some() {
        0
    } else {
        ID_NOT_FOUND
    }
}

/// `sceNetCtlCheckCallback()`: 0, delivering nothing - three calls on the registering thread
/// invoked no callback (measured). The state never changes here, so there is nothing to deliver.
fn check_callback(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNetCtlGetState(&state)`: 3, IP obtained, when the host has a link out, as a connected console
/// answered (measured); `0x80412107` for a null slot. With no link the state is unmeasured and
/// answers the placeholder.
fn get_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return INVALID_ADDR;
    }
    if crate::host::link().is_none() {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's state slot, by the call's contract; written through the checked accessor.
    if unsafe { guest::write_u32(args[0], STATE_IP_OBTAINED) } {
        0
    } else {
        INVALID_ADDR
    }
}

/// `sceNetCtlGetResult(event, &result)`: event 1 writes 0 and answers 0; events 2 and 3 answer
/// `0x8041210f` with the slot untouched; a null slot `0x80412107` (measured). Other events are
/// unmeasured.
fn get_result(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [event, result, ..] = *args;
    if result == 0 {
        return INVALID_ADDR;
    }
    match event as u32 {
        1 => {
            // SAFETY: the guest's result slot, by the call's contract.
            if unsafe { guest::write_u32(result, 0) } {
                0
            } else {
                INVALID_ADDR
            }
        }
        2 | 3 => NOT_AVAILABLE,
        _ => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

#[cfg(test)]
mod tests {
    /// With no link out, every measured code whose value comes from the link is refused with a
    /// zeroed buffer - the DNS codes included - rather than left unanswered.
    #[test]
    fn with_no_link_the_link_codes_are_refused_with_a_zeroed_buffer() {
        for code in [2, 3, 4, 14, 15, 16, 17, 18] {
            let (info, rc) = super::answer(code, None).expect("a measured code is answered");
            assert_ne!(rc, 0, "code {code} refused");
            assert!(info.iter().all(|&b| b == 0), "code {code} zeroed");
        }
    }
}
