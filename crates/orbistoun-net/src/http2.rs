//! `libSceHttp2` - the second-generation HTTP client.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. The library context is implemented by D151 - a
//! handle the guest only compares and passes back is a small integer from one - and is assumed,
//! not measured (REQ-ht01 asks). The transfers are not implemented.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

use crate::contexts::Contexts;

guest_module! {
    "libSceHttp2" {
        "sceHttp2AbortRequest" => 6,
        "sceHttp2AddRequestHeader" => 6,
        "sceHttp2CookieFlush" => 6,
        "sceHttp2CreateRequestWithURL" => 6,
        "sceHttp2CreateTemplate" => 6,
        "sceHttp2DeleteRequest" => 6,
        "sceHttp2DeleteTemplate" => 6,
        "sceHttp2GetAllResponseHeaders" => 6,
        "sceHttp2GetResponseContentLength" => 6,
        "sceHttp2GetStatusCode" => 6,
        "sceHttp2Init" => 6,
        "sceHttp2ReadData" => 6,
        "sceHttp2SendRequest" => 6,
        "sceHttp2SetAuthEnabled" => 6,
        "sceHttp2SetConnectTimeOut" => 6,
        "sceHttp2SetRecvTimeOut" => 6,
        "sceHttp2SetRedirectCallback" => 6,
        "sceHttp2SetRequestContentLength" => 6,
        "sceHttp2SetSendTimeOut" => 6,
        "sceHttp2SetSslCallback" => 6,
        "sceHttp2SslDisableOption" => 6,
        "sceHttp2SslEnableOption" => 6,
        "sceHttp2Term" => 6,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceHttp2Init", init), ("sceHttp2Term", term)]
}

/// The contexts issued and not yet retired.
static CONTEXTS: Contexts = Contexts::new();

/// `sceHttp2Init(netPoolId, sslCtxId, poolSize, maxConcurrentRequests)`: a library context, a
/// positive id from one (D151). PPSA28061 passes the pool and SSL ids it was just given and treats
/// anything negative as failure. The pools are the platform's memory accounting; nothing here
/// draws on them.
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    CONTEXTS
        .issue()
        .map_or(u64::from(GuestError::HostFailed.as_raw()), u64::from)
}

/// `sceHttp2Term(context)`: retires a context and answers 0. One not issued, or already retired,
/// answers the placeholder: the library's code for it is unmeasured.
fn term(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if CONTEXTS.retire(args[0]) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}
