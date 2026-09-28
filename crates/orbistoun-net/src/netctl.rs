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
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNetCtlGetInfo", get_info),
        ("sceNetCtlInit", init),
        ("sceNetCtlTerm", term),
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
        // A measured code whose value the host cannot supply: no link out, or no gateway.
        (2..=4 | 14..=16, _) => refused,
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
