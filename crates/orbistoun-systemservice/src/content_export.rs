//! `libSceContentExport` - handing a title's content (a picture, a clip) to the system.
//!
//! `sceContentExportInit2` is implemented, guest-observed (D812); nothing else is.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceContentExport" {
        // (param): a block whose first two words are the title's allocation and release functions.
        "sceContentExportInit2" => 1,
    }
}

/// `sceContentExportInit2(param)`: 0, with nothing written and neither callback called (D812).
/// The call binds in no probe leg (REQ-ce01), so this is what the title's own code requires:
/// PPSA21564 asserts the answer is 0 and traps if not, and the retail title runs on the console. A
/// null block is not that call and answers the placeholder.
fn init2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

/// Implementations this module provides for `libSceContentExport`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceContentExportInit2", init2)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Initialising with PPSA21564's block answers 0 and leaves the block as it was (D812).
    #[test]
    fn initialising_answers_zero_and_writes_nothing() {
        let block = [0x1111_u64, 0x2222, 0, 0, 0];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = block.as_ptr() as u64;
        assert_eq!(super::init2(&args), 0);
        assert_eq!(block, [0x1111, 0x2222, 0, 0, 0]);
    }
}
