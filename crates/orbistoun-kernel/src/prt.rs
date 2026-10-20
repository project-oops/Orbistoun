//! The PRT aperture: the slots `sceKernelGetPrtAperture` reads back.
//!
//! The console's answers are obSCEne's `020-memory/memory-pool-prt-aperture` rows
//! (REQ-20261012T1822Z-7389, report `report-20261014-batch1.log` lines 3495-3529, FW 12.40):
//!
//! - Slots 0, 1 and 2 answer 0 and read base 1, length 0, both before and after the Set calls.
//! - Slot 3 answers `EINVAL` and writes nothing.
//! - With both outs null the call faults.
//! - Every `sceKernelSetPrtAperture` the probe made was refused. It tried slots 0-3 and -1, at a
//!   4 GiB base aligned and unaligned, with a 64 KiB length.
//!
//! What Set accepts is unmeasured, so Set is not implemented and refuses by name.
//! REQ-20261020T1310Z-km01 section 2 asks for its window. Until a slot can be set, every slot
//! reads as the console's unset one.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, errno};
use orbistoun_mem::guest;

use super::OK;

/// How many slots there are: 0, 1 and 2 answer, and 3 is refused.
pub const SLOTS: i32 = 3;

/// The base an unset slot reads as.
pub const UNSET_BASE: u64 = 1;

/// The length an unset slot reads as.
pub const UNSET_LEN: u64 = 0;

/// `sceKernelGetPrtAperture(slot, base_out, len_out)`: the base and length of one aperture slot.
pub fn get(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // An `int` in the first register.
    let slot = args[0] as u32 as i32;
    let (base_out, len_out) = (args[1], args[2]);
    // Slot 3 is the measured refusal; a negative slot is outside the same range.
    if !(0..SLOTS).contains(&slot) {
        return u64::from(GuestError::vendor(errno::INVALID).as_raw());
    }
    // The console faults writing through a null out. A handler cannot fault the guest, so it
    // answers what FreeBSD's `copyout` answers for an address it cannot write. km01 2c asks which
    // of the two outs faults alone.
    if base_out == 0 || len_out == 0 {
        return u64::from(GuestError::vendor(errno::FAULT).as_raw());
    }
    // SAFETY: a non-null address the guest passed for this call, valid by its contract.
    let base_written = unsafe { guest::write_u64(base_out, UNSET_BASE) };
    // SAFETY: as above.
    let len_written = unsafe { guest::write_u64(len_out, UNSET_LEN) };
    if base_written && len_written {
        OK
    } else {
        u64::from(GuestError::vendor(errno::FAULT).as_raw())
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn args(slot: u64, base: u64, len: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut out = [0; GUEST_ARG_REGISTERS];
        out[..3].copy_from_slice(&[slot, base, len]);
        out
    }

    /// Slots 0-2 read base 1 and length 0; slot 3 is `EINVAL` with both outs untouched.
    #[test]
    fn every_slot_reads_as_the_console_s_unset_one() {
        for slot in 0..3 {
            let (mut base, mut len) = (0xcccc_cccc_cccc_cccc_u64, 0xcccc_cccc_cccc_cccc_u64);
            let at = |p: *mut u64| p as usize as u64;
            assert_eq!(
                super::get(&args(slot, at(&raw mut base), at(&raw mut len))),
                0
            );
            assert_eq!((base, len), (1, 0), "slot {slot}");
        }
        let (mut base, mut len) = (0xcccc_cccc_cccc_cccc_u64, 0xcccc_cccc_cccc_cccc_u64);
        let at = |p: *mut u64| p as usize as u64;
        assert_eq!(
            super::get(&args(3, at(&raw mut base), at(&raw mut len))),
            0x8002_0016
        );
        assert_eq!((base, len), (0xcccc_cccc_cccc_cccc, 0xcccc_cccc_cccc_cccc));
    }

    /// Null outs, where the console faults, are answered `EFAULT`.
    #[test]
    fn null_outs_are_a_fault() {
        assert_eq!(super::get(&args(0, 0, 0)), 0x8002_000e);
    }
}
