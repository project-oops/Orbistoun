//! `libSceNetCtl` - network configuration and link status.
//!
//! `sceNetCtlInit` and `sceNetCtlTerm` bracket the library's use and hold nothing the host
//! network needs. `sceNetCtlGetInfo` answers from the platform's own numbering of its
//! information codes and values, which is unmeasured, so it is declared and unserved.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNetCtl" {
        "sceNetCtlGetInfo" => 6,
        "sceNetCtlInit" => 0,
        "sceNetCtlTerm" => 0,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceNetCtlInit", init), ("sceNetCtlTerm", term)]
}

/// `sceNetCtlInit()`: zero.
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNetCtlTerm()`: nothing to release.
fn term(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}
