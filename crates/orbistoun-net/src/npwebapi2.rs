//! `libSceNpWebApi2` - the account service's web API.
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
    "libSceNpWebApi2" {
        "sceNpWebApi2CreateRequest" => 6,
        "sceNpWebApi2CreateUserContext" => 6,
        "sceNpWebApi2DeleteRequest" => 6,
        "sceNpWebApi2DeleteUserContext" => 6,
        "sceNpWebApi2Initialize" => 6,
        "sceNpWebApi2ReadData" => 6,
        "sceNpWebApi2SendRequest" => 6,
        "sceNpWebApi2Terminate" => 6,
    }
}
