//! `libSceJson2` - the platform's JSON parser, reached by mangled C++ names.
//!
//! The names are declared and none is implemented, so the module is listed in `SERVES_NOTHING`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceJson2" {
        "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE" => 6,
        "_ZN3sce4Json11InitializerC1Ev" => 6,
        "_ZN3sce4Json12MemAllocatorC2Ev" => 6,
    }
}
