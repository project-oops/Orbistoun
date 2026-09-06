//! `libSceSsl` - TLS.
//!
//! **10 names, declared and not implemented.** They come from PPSA02664's own import table (10).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceSsl" {
        "sceSslFreeSslCertName" => 6,
        "sceSslGetCaCerts" => 6,
        "sceSslGetIssuerName" => 6,
        "sceSslGetMemoryPoolStats" => 6,
        "sceSslGetNameEntryCount" => 6,
        "sceSslGetNameEntryInfo" => 6,
        "sceSslGetSerialNumber" => 6,
        "sceSslGetSubjectName" => 6,
        "sceSslInit" => 6,
        "sceSslTerm" => 6,
    }
}
