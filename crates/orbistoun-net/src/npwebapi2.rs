//! `libSceNpWebApi2` - the account service's web API.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. The library and user contexts are implemented by
//! D151 - handles the guest only compares and passes back are small integers from one - and are
//! assumed, not measured. Requests are not implemented.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

use crate::contexts::Contexts;

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

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpWebApi2Initialize", initialize),
        ("sceNpWebApi2Terminate", terminate),
        ("sceNpWebApi2CreateUserContext", create_user_context),
        ("sceNpWebApi2DeleteUserContext", delete_user_context),
    ]
}

static LIBRARIES: Contexts = Contexts::new();
static USERS: Contexts = Contexts::new();

fn placeholder() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceNpWebApi2Initialize(httpCtxId, poolSize)`: a library context, a positive id from one
/// (D151). PPSA28061 passes the HTTP/2 context it was just given.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    LIBRARIES.issue().map_or(placeholder(), u64::from)
}

/// `sceNpWebApi2Terminate(libCtxId)`: retires it and answers 0; one not live answers the
/// placeholder, the library's code being unmeasured.
fn terminate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if LIBRARIES.retire(args[0]) {
        0
    } else {
        placeholder()
    }
}

/// `sceNpWebApi2CreateUserContext(libCtxId, userId)`: a user context under a live library context,
/// a positive id from one (D151); under one not live, the placeholder.
fn create_user_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !LIBRARIES.live(args[0]) {
        return placeholder();
    }
    USERS.issue().map_or(placeholder(), u64::from)
}

/// `sceNpWebApi2DeleteUserContext(userCtxId)`: retires it and answers 0; one not live answers the
/// placeholder.
fn delete_user_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if USERS.retire(args[0]) {
        0
    } else {
        placeholder()
    }
}
