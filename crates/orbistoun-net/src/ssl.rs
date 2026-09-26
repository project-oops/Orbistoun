//! `libSceSsl` - TLS.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
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
