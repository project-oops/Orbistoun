//! `libSceAppContent` - downloadable content and additional-data mounts.
//!
//! **8 names, declared and not implemented.** They come from PPSA02664's own import table (8).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAppContent" {
        "sceAppContentAddcontMount" => 6,
        "sceAppContentAddcontUnmount" => 6,
        "sceAppContentAppParamGetInt" => 6,
        "sceAppContentDownloadDataGetAvailableSpaceKb" => 6,
        "sceAppContentInitialize" => 6,
        "sceAppContentTemporaryDataFormat" => 6,
        "sceAppContentTemporaryDataGetAvailableSpaceKb" => 6,
        "sceAppContentTemporaryDataMount2" => 6,
    }
}
