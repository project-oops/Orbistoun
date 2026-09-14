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
        "sceAgcDriverRegisterOwner" => 6,
        "sceAgcDriverRegisterResource" => 6,
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

/// `SCE_AGC_ERROR_RESOURCE_REGISTRATION_NOT_SUPPORTED`.
///
/// **The whole resource-registration subsystem is a stub on retail.** obSCEne disassembled
/// `libSceAgcDriver.sprx` (REQ-...0925Z-7b3c) and found `sceAgcDriverRegisterOwner`,
/// `RegisterResource`, `InitResourceRegistration` are each `mov $0x8a6c9018, %eax; ret` - pure
/// stubs - and confirmed it on hardware: they return `0x8a6c9018` and mutate zero bytes of their
/// caller buffers. So this is the measured value, and orbistoun returns it for fidelity (principle 1): the
/// guest gets exactly the "not supported" the console gives it.
///
/// It is **not** what gates PPSA28061's startup abort, and returning it does not clear that abort.
/// With this in effect alongside `sceAgcCreateShader -> 0x0` and
/// `sceKernelMapperGetParam -> 0x80020006`, PPSA28061 still aborts at the same point (worklog 515).
/// The abort is gated on the *mapper's* return, not this constant (D643); that the console ships with
/// these measured codes and the guest here does not survive them is an open divergence, not a reason
/// to invent a success.
const RESOURCE_REGISTRATION_NOT_SUPPORTED: u64 = 0x8a6c_9018;

/// `sceAgcDriverRegisterOwner(owner_buf)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_owner(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverRegisterResource(res, ...)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_resource(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverInitResourceRegistration(...)` - stub, returns `0x8a6c9018`.
fn init_resource_registration(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverQueryResourceRegistrationUserMemoryRequirements(...)` - stub, returns `0x8a6c9018`.
///
/// Hardware left the caller's size sentinel at `0`; being a "not supported" stub it does not write a
/// meaningful requirement, and a guest that gets the error does not read the size, so nothing is
/// written back rather than an invented figure.
fn query_resource_registration_user_memory_requirements(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// Implementations this crate provides for `libSceAgcDriver`.
///
/// `sceAgcDriverCreateQueue` accepts the queue (9a41); the resource-registration family are the
/// retail stubs 7b3c disassembled, each returning the measured `0x8a6c9018`. This is fidelity, not an
/// unblock: PPSA28061 still aborts at its startup wall with these in effect (the abort is gated on the
/// mapper, D643/worklog 515).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAgcDriverCreateQueue", create_queue),
        ("sceAgcDriverRegisterOwner", register_owner),
        ("sceAgcDriverRegisterResource", register_resource),
        (
            "sceAgcDriverInitResourceRegistration",
            init_resource_registration,
        ),
        (
            "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements",
            query_resource_registration_user_memory_requirements,
        ),
    ]
}
