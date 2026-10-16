//! `libSceContentDelete` - removing a title's downloaded content.
//!
//! `sceContentDeleteInitialize` is implemented, as measured in the shape PPSA04263 calls it;
//! nothing else is.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceContentDelete" {
        // (param)
        "sceContentDeleteInitialize" => 1,
    }
}

/// What a second `sceContentDeleteInitialize` answered (REQ-gt03, `report-1792154079.txt` 2641).
const ALREADY_INITIALISED: u64 = 0x809d_5003;

/// The block PPSA04263 passes: u64 4, u64 0x4000 and u64 0x1c, then a pointer (REQ-gt03).
const MEASURED_BLOCK: [u64; 3] = [4, 0x4000, 0x1c];

/// Whether `sceContentDeleteInitialize` has answered 0 in this process.
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// `sceContentDeleteInitialize(param)`: 0 the first time and `0x809d5003` after, the block left as
/// it was, for PPSA04263's block - 4, 0x4000, 0x1c and a pointer (REQ-gt03, `000-boot/gt03-
/// contentdelete`, `report-1792154079.txt` 2636-2643, after `sceSysmoduleLoadModule(0x00ee)`). A
/// null or zeroed block answered `0x809d5001` (REQ-gt02); any shape but the measured one, a null
/// block among them, answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(GuestError::Unimplemented.as_raw());
    if args[0] == 0 {
        return placeholder;
    }
    let mut block = [0_u8; 0x20];
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    if !unsafe { orbistoun_mem::guest::read_into(args[0], &mut block) } {
        return placeholder;
    }
    let word =
        |at: usize| u64::from_le_bytes(block[at * 8..at * 8 + 8].try_into().unwrap_or([0; 8]));
    if [word(0), word(1), word(2)] != MEASURED_BLOCK || word(3) == 0 {
        return placeholder;
    }
    if INITIALISED.swap(true, Ordering::Relaxed) {
        ALREADY_INITIALISED
    } else {
        0
    }
}

/// Implementations this module provides for `libSceContentDelete`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceContentDeleteInitialize", initialize)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// PPSA04263's block initialises once - 0, then `0x809d5003` - and is left as it was; a block
    /// of another shape answers the placeholder (REQ-gt03).
    #[test]
    fn initialising_answers_as_measured() {
        let region = [0_u8; 0x40];
        let block = [4_u64, 0x4000, 0x1c, region.as_ptr() as u64];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = block.as_ptr() as u64;
        assert_eq!(super::initialize(&args), 0);
        assert_eq!(super::initialize(&args), 0x809d_5003);
        assert_eq!(block[..3], [4, 0x4000, 0x1c]);
        let zeroed = [0_u64; 4];
        args[0] = zeroed.as_ptr() as u64;
        assert_ne!(super::initialize(&args), 0, "an unmeasured shape");
    }
}
