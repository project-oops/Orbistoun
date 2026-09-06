//! `libSceHttp2` - the second-generation HTTP client.
//!
//! **23 names, declared and not implemented.** They come from PPSA02664's own import table (23).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

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
