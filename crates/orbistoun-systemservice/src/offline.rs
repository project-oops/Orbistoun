//! The online services a title starts at boot, as a console signed in to no network account
//! answers them: each starts and reports nothing.
//!
//! orbistoun has no network account and implements no online service (D832 scope), so a title's
//! online stack meets an offline console. Every initialiser here answers 0, as an offline console's
//! does: the library loads and starts, and only the calls that would reach the network fail later,
//! when a title makes them. A title that branches on these answers - PPSA04263 takes its online
//! start-up's failure path on a non-zero one (`js`/`jne` at its call sites), shutting down the
//! thread that delivers its user's sign-in state - goes on as it does offline.
//!
//! Assumed, not measured: obSCEne's probes could not load these modules unprivileged (census
//! 20261007, `module-load` 0x80020002). The reference emulators answer each 0 for an offline
//! console; the hardware census filed as REQ-gt05/gt09 replaces these if it disagrees.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// `libSceNpEntitlementAccess`: the store entitlements a title checks.
pub mod entitlement_access {
    orbistoun_hle::guest_module! {
        "libSceNpEntitlementAccess" {
            "sceNpEntitlementAccessInitialize" => 6,
        }
    }
}

/// `libSceGameLiveStreaming`: broadcasting the title's play.
pub mod live_streaming {
    orbistoun_hle::guest_module! {
        "libSceGameLiveStreaming" {
            "sceGameLiveStreamingInitialize" => 6,
        }
    }
}

/// `libSceSharePlay`: sharing play with a remote player.
pub mod share_play {
    orbistoun_hle::guest_module! {
        "libSceSharePlay" {
            "sceSharePlayInitialize" => 6,
        }
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpEntitlementAccessInitialize", started),
        ("sceGameLiveStreamingInitialize", started),
        ("sceSharePlayInitialize", started),
    ]
}

/// An online service's initialiser on an offline console: it starts, and answers 0.
pub(crate) fn started(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    /// Each online service a title starts answers 0, as an offline console's does.
    #[test]
    fn each_online_service_starts_offline() {
        let args = [0x4000, 0, 0, 0, 0, 0];
        for (name, function) in super::implementations() {
            assert_eq!(function(&args), 0, "{name}");
        }
    }
}
