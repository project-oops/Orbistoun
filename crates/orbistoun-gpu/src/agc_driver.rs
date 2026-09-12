//! `libSceAgcDriver` - the submission side of the current generation's graphics API.
//!
//! Separate from [`super::agc`] because the platform separates them: a guest builds command
//! buffers with `libSceAgc` and hands them over with `libSceAgcDriver`, and the two are
//! distinct libraries in an import table. Declaring them as one would make every trace
//! entry name the wrong library.
//!
//! Names, provenance and the arity caveat are as [`super::agc`] states them - read out of
//! real import tables, with arities deliberately unestablished.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgcDriver" {
        "sceAgcDriverAddEqEvent" => 6,
        "sceAgcDriverCreateQueue" => 3,
        "sceAgcDriverGetDefaultOwner" => 6,
        "sceAgcDriverGetResourceRegistrationMaxNameLength" => 6,
        "sceAgcDriverInitResourceRegistration" => 6,
        "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements" => 6,
        "sceAgcDriverRegisterDefaultOwner" => 6,
        "sceAgcDriverSetHsOffchipParam" => 6,
        "sceAgcDriverSetTFRing" => 6,
        "sceAgcDriverSubmitAcb" => 6,
        "sceAgcDriverSubmitDcb" => 6,
    }
}

/// `sceAgcDriverCreateQueue(type, out_queue, flags)`.
///
/// Accepts the queue type and returns `0` - the measured success code for both the compute queue
/// (`type` 3) and the graphics Universal Graphics Queue (`type` 0): obSCEne's
/// `166-agc/driver-create-queue` and `166-agc/primitive-draw` both record `rc-create 0x0` (sweeps
/// `20260911-*` and `20260912-003916`, the latter answering 9a41).
///
/// **What it deliberately does not do:** write the queue object into `*out_queue`. obSCEne measured
/// the object's header (`38 00 00 00 03 00 00 00 00 00 02 00 ...`) but not *where* the call places
/// it relative to the arguments - the `3c5e`-style pointer-distance measurement create-shader has,
/// this call does not yet. Fabricating a pointer into `*out_queue` is the plausible-output failure
/// principle 3 forbids, so the return is honest and the out-parameter waits on that measurement. No
/// guest reaches this call today (the corpus stalls earlier, at `sceAgcDriverRegisterOwner`), so the
/// accept-and-return is exercised by tests and by obSCEne's rc check, not by a guest dereference yet.
fn create_queue(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// Implementations this crate provides for `libSceAgcDriver`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceAgcDriverCreateQueue", create_queue)]
}
