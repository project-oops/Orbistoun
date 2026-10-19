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
            // (sku flag *)
            "sceNpEntitlementAccessGetSkuFlag" => 1,
            // (service label, list *, room, hits *)
            "sceNpEntitlementAccessGetAddcontEntitlementInfoList" => 4,
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
        ("sceNpEntitlementAccessGetSkuFlag", sku_flag),
        (
            "sceNpEntitlementAccessGetAddcontEntitlementInfoList",
            addcont_entitlements,
        ),
        ("sceGameLiveStreamingInitialize", started),
        ("sceSharePlayInitialize", started),
    ]
}

/// An online service's initialiser on an offline console: it starts, and answers 0.
pub(crate) fn started(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// The SKU flag of a full game, as `sceNpEntitlementAccessGetSkuFlag` reports it; a trial is 1.
const SKU_FULL: u32 = 3;

/// The answer to an entitlement query with a null destination or a bad parameter:
/// `SCE_NP_ENTITLEMENT_ACCESS_ERROR_INVALID_ARGUMENT` (REQ-gt20, measured on hardware).
const ENTITLEMENT_PARAMETER: u64 = 0x817d_0002;

/// `sceNpEntitlementAccessGetSkuFlag(flag *)`: the installed application's SKU, a full game, written
/// to `*flag`. It is the console's local licence, not a store query, so an offline console
/// answers it. PPSA04263 seeds its flag with 1 (trial), keeps it on a non-zero answer, and
/// treats anything but 1 as the full game.
fn sku_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's flag, one 32-bit word by the call's contract; written through the checked
    // accessor.
    if args[0] != 0 && unsafe { orbistoun_mem::guest::write_u32(args[0], SKU_FULL) } {
        0
    } else {
        ENTITLEMENT_PARAMETER
    }
}

/// `sceNpEntitlementAccessGetAddcontEntitlementInfoList(label, list *, room, hits *)`: the
/// add-ons installed for the title - none - so `*hits` is 0 and the list is left as it was.
/// PPSA04263 asks first with no list for the count, and marks its entitlement check done on 0
/// hits.
fn addcont_entitlements(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's hit count, one 32-bit word by the call's contract; written through the
    // checked accessor.
    if args[3] != 0 && unsafe { orbistoun_mem::guest::write_u32(args[3], 0) } {
        0
    } else {
        ENTITLEMENT_PARAMETER
    }
}

#[cfg(test)]
mod tests {
    /// Each online service a title starts answers 0, as an offline console's does.
    #[test]
    fn each_online_service_starts_offline() {
        let args = [0x4000, 0, 0, 0, 0, 0];
        // The initialisers only: the queries write through their arguments.
        let initialisers = super::implementations()
            .iter()
            .filter(|(name, _)| name.ends_with("Initialize"));
        for (name, function) in initialisers {
            assert_eq!(function(&args), 0, "{name}");
        }
    }

    /// The SKU is a full game and no add-on is installed; a null destination is refused.
    #[test]
    fn the_licence_is_a_full_game_with_no_add_ons() {
        let mut word = 1_u32;
        let at = std::ptr::from_mut(&mut word) as u64;
        assert_eq!(super::sku_flag(&[at, 0, 0, 0, 0, 0]), 0);
        assert_eq!(word, super::SKU_FULL);
        word = 7;
        assert_eq!(super::addcont_entitlements(&[0, 0, 0, at, 0, 0]), 0);
        assert_eq!(word, 0);
        assert_eq!(super::sku_flag(&[0; 6]), super::ENTITLEMENT_PARAMETER);
        assert_eq!(
            super::addcont_entitlements(&[0; 6]),
            super::ENTITLEMENT_PARAMETER
        );
    }
}
