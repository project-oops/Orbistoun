//! `libSceAgcDriver` - the submission side of the current generation's graphics API.
//!
//! Separate from [`super::agc`] because the platform separates them: a guest builds command
//! buffers with `libSceAgc` and hands them over with `libSceAgcDriver`, and the two are
//! distinct libraries in an import table. Declaring them as one would make every trace
//! entry name the wrong library.
//!
//! Names, provenance and the arity caveat are as [`super::agc`] states them - read out of
//! real import tables, with arities deliberately unestablished.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgcDriver" {
        "sceAgcDriverAddEqEvent" => 6,
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
