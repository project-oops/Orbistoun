//! `libSceHttp2` - the second-generation HTTP client.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. The library context is implemented as obSCEne
//! measured it (REQ-ht01, `reports/hardware/20261007-102530-eboot.obs.log` 4410-4423). The
//! transfers are not implemented.

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

/// The contexts issued and not yet retired, numbered from `0x30000001` (`arm2-init`).
static CONTEXTS: Contexts = Contexts::from(0x3000_0001);

/// An SSL context id of 0 (`arm3-zero-ids`, `rc-ssl0`).
const INVALID_SSL_CONTEXT: u64 = 0x8095_f006;

/// `sceHttp2Term` on a context already retired (`arm4-term`, `rc-second`).
const ALREADY_TERMINATED: u64 = 0x817b_1100;

/// `sceHttp2Init(netPoolId, sslCtxId, poolSize, maxConcurrentRequests)`: a library context, the
/// next from `0x30000001`. A pool id of 0 is accepted - the library's own pool - and an SSL id of
/// 0 refused (REQ-ht01, `130-layout/http2-init`). The pools are the platform's memory accounting;
/// nothing here draws on them.
fn init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[1] == 0 {
        return INVALID_SSL_CONTEXT;
    }
    CONTEXTS
        .issue()
        .map_or(u64::from(GuestError::HostFailed.as_raw()), u64::from)
}

/// `sceHttp2Term(context)`: retires a context and answers 0; one already retired answers
/// `0x817b1100`, as measured. One never issued answers the same, the nearest measured case.
fn term(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if CONTEXTS.retire(args[0]) {
        0
    } else {
        ALREADY_TERMINATED
    }
}
