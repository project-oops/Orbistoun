//! `libSceJson2` - the platform's JSON parser, reached by mangled C++ names.
//!
//! **3 names, declared and not implemented.** They come from other modules in the recorded corpus (3).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceJson2" {
        "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE" => 6,
        "_ZN3sce4Json11InitializerC1Ev" => 6,
        "_ZN3sce4Json12MemAllocatorC2Ev" => 6,
    }
}
