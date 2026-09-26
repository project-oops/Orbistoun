//! `libSceAmpr` - command-buffer construction for asynchronous processing, placed beside the
//! graphics submission path by name association.
//!
//! The names are declared and none is implemented, so the module is listed in `SERVES_NOTHING`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAmpr" {
        "sceAmprAprCommandBufferConstructor" => 6,
        "sceAmprAprCommandBufferReadFile" => 6,
        "sceAmprCommandBufferConstructor" => 6,
        "sceAmprCommandBufferReset" => 6,
        "sceAmprCommandBufferSetBuffer" => 6,
    }
}
