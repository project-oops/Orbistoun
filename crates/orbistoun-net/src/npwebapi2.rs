//! `libSceNpWebApi2` - the account service's web API.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
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
