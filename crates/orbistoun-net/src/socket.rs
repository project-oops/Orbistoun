//! `libSceNet` - sockets.
//!
//! **20 names, declared and not implemented.** They come from PPSA02664's own import table (20).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceNet" {
        "sceNetAccept" => 6,
        "sceNetBind" => 6,
        "sceNetConnect" => 6,
        "sceNetErrnoLoc" => 6,
        "sceNetGetsockname" => 6,
        "sceNetHtons" => 6,
        "sceNetInetPton" => 6,
        "sceNetListen" => 6,
        "sceNetPoolCreate" => 6,
        "sceNetPoolDestroy" => 6,
        "sceNetRecv" => 6,
        "sceNetRecvfrom" => 6,
        "sceNetResolverCreate" => 6,
        "sceNetResolverDestroy" => 6,
        "sceNetResolverStartNtoa" => 6,
        "sceNetSend" => 6,
        "sceNetSendto" => 6,
        "sceNetSetsockopt" => 6,
        "sceNetSocket" => 6,
        "sceNetSocketClose" => 6,
    }
}
