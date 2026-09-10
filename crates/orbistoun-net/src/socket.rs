//! `libSceNet` - sockets.
//!
//! **20 names declared, ten of them served.** They come from PPSA02664's own import table (20).
//!
//! # Arities, for the eleven that have one
//!
//! Every arity here used to be `6`, the trampoline's full capture, on the rule that a wrong
//! arity only degrades a trace while a wrong name is a shim nothing can reach (D504). Eleven of
//! them are no longer guesses: obSCEne types each import in `src/probe/sections/net.c` and calls
//! it through that type, and hardware answered - a descriptor, a bind, a listen, a would-block
//! code. **What that establishes is that the first N arguments are those**, not that there is no
//! N+1th; a trailing argument nothing passes would be invisible to a call that works. Recorded
//! at that strength.
//!
//! The nine that are still `6` have never been called.
//!
//! # Why the bodies are next door and the spellings are here
//!
//! `orbistoun-fs` owns sockets - it holds the descriptor table a guest closes with `close`
//! whether it opened a file or a socket, so the two cannot live apart. What it does *not* own
//! is how this library reports a failure, and that is the one place the two spellings of these
//! calls disagree: a POSIX-named `bind` answers `-1` and leaves the number in `errno`, and
//! `sceNetBind` answers [`NET_ERROR_BASE`]` | errno`.
//!
//! So the bodies carry the errno out as `orbistoun_fs::socket::Answer` and each crate encodes
//! it its own way. This is the same rule D525 settled for `sceKernelStat`: a name is offered
//! by the crate that declares it, wherever the body happens to live (D667).
//!
//! **Ten names remain declared and unserved** - the resolvers, the pools, the datagram calls,
//! `sceNetErrnoLoc` and `sceNetInetPton`. Nothing has measured what they answer.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_fs::socket::{self as bodies, Answer};
use orbistoun_hle::guest_module;

use super::NET_ERROR_BASE;

/// A body's answer in this library's spelling: the value, or `0x8041_0100 | errno`.
///
/// The counterpart to `orbistoun_fs::socket::as_posix`, and the reason the errno travels at
/// all. Widened to sixty-four bits by zero extension rather than sign extension, because the
/// codes obSCEne read back off the console are what a thirty-two-bit `int` holds and the guest
/// truncates to that width before comparing.
#[must_use]
fn as_vendor(answer: Answer) -> u64 {
    match answer {
        Ok(value) => value,
        Err(errno) => u64::from(GuestError::vendor_in(NET_ERROR_BASE, errno).as_raw()),
    }
}

/// One `GuestFn` per errno-bearing body, in this library's spelling.
///
/// `orbistoun-fs` has the matching macro for the POSIX one. Two macros rather than one shared
/// one because the rules differ and each crate knows only its own.
macro_rules! vendor_spellings {
    ($($wrapper:ident => $body:ident,)*) => {
        $(
            fn $wrapper(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
                as_vendor(bodies::$body(args))
            }
        )*
    };
}

/// `sceNetSocket(name, family, type, protocol)` - the one call whose arguments are not POSIX's.
///
/// **A leading name, which POSIX `socket` does not take.** Every other call in this library sits
/// at the same argument positions as its POSIX twin, so they share a body outright; this one is
/// shifted by one register, and sharing it unshifted would read a `const char *` as the address
/// family and refuse every socket a title asked for.
///
/// The name is what the platform labels the socket with, for its own accounting. Nothing here
/// keeps it and nothing pretends to: obSCEne passed `"obscene"` and then `NULL` and hardware
/// answered the same descriptor (`0x13`) to both, so it changes nothing a guest can observe
/// through this interface.
fn vendor_socket(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let shifted = [args[1], args[2], args[3], args[4], args[5], 0];
    as_vendor(bodies::socket(&shifted))
}

vendor_spellings! {
    vendor_bind => bind,
    vendor_listen => listen,
    vendor_accept => accept,
    vendor_connect => connect,
    vendor_setsockopt => setsockopt,
    vendor_getsockname => getsockname,
    vendor_send => send,
    vendor_recv => recv,
    vendor_close => close,
}

/// Implementations this module provides, by symbol name.
///
/// Exactly the names obSCEne's `102-net/resolve` reads out of a title's import table and calls,
/// plus `sceNetGetsockname` - a server that binds to port zero asks which port it got and
/// prints it, and a made-up number there is a wrong port in front of whoever is connecting.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNetSocket", vendor_socket),
        ("sceNetBind", vendor_bind),
        ("sceNetListen", vendor_listen),
        ("sceNetAccept", vendor_accept),
        ("sceNetConnect", vendor_connect),
        ("sceNetSetsockopt", vendor_setsockopt),
        ("sceNetGetsockname", vendor_getsockname),
        ("sceNetSend", vendor_send),
        ("sceNetRecv", vendor_recv),
        ("sceNetSocketClose", vendor_close),
        // No wrapper: a byte swap cannot fail, so there is no errno to encode and the POSIX
        // body is the whole function.
        ("sceNetHtons", bodies::htons),
    ]
}

guest_module! {
    "libSceNet" {
        "sceNetAccept" => 3,
        "sceNetBind" => 3,
        "sceNetConnect" => 3,
        "sceNetErrnoLoc" => 6,
        "sceNetGetsockname" => 3,
        "sceNetHtons" => 1,
        "sceNetInetPton" => 6,
        "sceNetListen" => 2,
        "sceNetPoolCreate" => 6,
        "sceNetPoolDestroy" => 6,
        "sceNetRecv" => 4,
        "sceNetRecvfrom" => 6,
        "sceNetResolverCreate" => 6,
        "sceNetResolverDestroy" => 6,
        "sceNetResolverStartNtoa" => 6,
        "sceNetSend" => 4,
        "sceNetSendto" => 6,
        "sceNetSetsockopt" => 5,
        "sceNetSocket" => 4,
        "sceNetSocketClose" => 1,
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
    use orbistoun_fs::socket::{SOCKADDR_IN_LEN, af_inet, sock_stream};

    use super::super::NET_ERROR_BASE;

    /// A socket, opened the way a title opens one: `sceNetSocket(name, family, type, protocol)`.
    ///
    /// **The name is the first argument**, which is where this interface differs from POSIX and
    /// where a test that called it POSIX-style would pass while the guest failed. `NULL` for the
    /// name, which hardware accepted alongside a real one.
    fn open_socket() -> u64 {
        call("sceNetSocket", [0, af_inet(), sock_stream(), 0, 0, 0])
    }

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("served");
        function(&args)
    }

    /// The bytes a guest passes for an address, laid out as `sys/netinet/in.h` says.
    fn sockaddr(port: u16, sin_len: u8) -> [u8; SOCKADDR_IN_LEN as usize] {
        let mut bytes = [0_u8; SOCKADDR_IN_LEN as usize];
        bytes[0] = sin_len;
        bytes[1] = af_inet() as u8;
        bytes[2..4].copy_from_slice(&port.to_be_bytes());
        bytes[4..8].copy_from_slice(&[127, 0, 0, 1]);
        bytes
    }

    /// **The first argument is a name, not the address family.**
    ///
    /// The one place this library's arguments are not POSIX's, and the one that would have
    /// shipped: `sceNetBind`, `sceNetConnect`, `sceNetRecv` and the rest sit at the same
    /// registers as their POSIX twins, so sharing a body outright is right for them and wrong
    /// for this one. Unshifted, a `const char *` arrives where `AF_INET` belongs and every
    /// socket a title asks for is refused.
    ///
    /// Asserted from both sides, because only the pair says which way round it is: called the
    /// way a title calls it, a descriptor; called POSIX-style, a refusal.
    ///
    /// Reference: obSCEne `src/probe/sections/net.c`, which types the import as
    /// `int (*)(const char *, int, int, int)` and measured hardware answering `0x13` to both a
    /// name and `NULL`.
    #[test]
    fn the_socket_call_takes_a_name_before_the_family() {
        let named = c"orbistoun";
        let fd = call(
            "sceNetSocket",
            [named.as_ptr() as u64, af_inet(), sock_stream(), 0, 0, 0],
        );
        assert_ne!(
            fd,
            as_refusal(orbistoun_fs::socket::UNNAMED),
            "a named socket, which is how a title opens one"
        );
        assert_eq!(call("sceNetSocketClose", [fd, 0, 0, 0, 0, 0]), 0);

        assert_eq!(
            call("sceNetSocket", [af_inet(), sock_stream(), 0, 0, 0, 0]),
            as_refusal(orbistoun_fs::socket::UNNAMED),
            "POSIX-style, the family lands in the name register and there is no socket to open"
        );
    }

    /// The code this library answers for a given errno, as a guest sees it.
    fn as_refusal(errno: u32) -> u64 {
        u64::from(GuestError::vendor_in(NET_ERROR_BASE, errno).as_raw())
    }
    /// **Every name obSCEne resolves in the title leg is served here.**
    ///
    /// The failure this pins: the bodies existed under their POSIX spellings and the vendor
    /// twins reached a stub, so a title calling `sceNetBind` got `0x7fff0001` and three checks
    /// that pass on hardware failed here (D667). Named individually rather than counted,
    /// because a count says something is missing and leaves finding it to a reader.
    #[test]
    fn the_names_a_title_resolves_are_all_served() {
        let missing: Vec<&str> = [
            "sceNetSocket",
            "sceNetBind",
            "sceNetListen",
            "sceNetAccept",
            "sceNetConnect",
            "sceNetSetsockopt",
            "sceNetSend",
            "sceNetRecv",
            "sceNetSocketClose",
        ]
        .into_iter()
        .filter(|wanted| !super::implementations().iter().any(|(n, _)| n == wanted))
        .collect();
        assert!(
            missing.is_empty(),
            "obSCEne's `102-net/resolve` imports these and nothing answers them: {missing:?}"
        );
    }

    /// **Nothing is served that is not declared**, or the guest can never reach it.
    ///
    /// The other half, and the one that fails silently: a served name absent from the module's
    /// import list is a function nothing links to, which reads in a report as implemented and
    /// behaves as a stub.
    #[test]
    fn every_served_name_is_declared() {
        for (name, _) in super::implementations() {
            assert!(
                super::MODULE.imports.iter().any(|i| i.name == *name),
                "{name} is served but not declared"
            );
        }
    }

    /// **The vendor spelling binds, and takes either `sin_len`.**
    ///
    /// Measured, `102-net/sockaddr-bind`: hardware answers `0x0` for `sin_len` of both **0 and
    /// 16**, so the platform does not validate that byte. Refusing the zero would be orbistoun
    /// inventing a check the console does not do.
    #[test]
    fn the_vendor_bind_takes_either_sockaddr_length() {
        let fd = open_socket();
        let full = sockaddr(0, SOCKADDR_IN_LEN as u8);
        assert_eq!(
            call(
                "sceNetBind",
                [fd, full.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0,
            "sin_len 16"
        );

        let second = open_socket();
        let zeroed = sockaddr(0, 0);
        assert_eq!(
            call(
                "sceNetBind",
                [second, zeroed.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0,
            "sin_len 0, which the console also binds"
        );
        assert_eq!(call("sceNetSocketClose", [fd, 0, 0, 0, 0, 0]), 0);
        assert_eq!(call("sceNetSocketClose", [second, 0, 0, 0, 0, 0]), 0);
    }

    /// **A failure comes back in this library's numbering, not POSIX's `-1`.**
    ///
    /// The whole reason the vendor spelling is served from here. `sceNetRecv` on a listening
    /// socket answered `0x8041_0139` on hardware; a guest that tests `rc == -1` sees neither,
    /// and a guest that switches on the code sees only one of them.
    #[test]
    fn a_failure_is_encoded_the_way_this_library_encodes_one() {
        let fd = open_socket();
        let wanted = sockaddr(0, SOCKADDR_IN_LEN as u8);
        assert_eq!(
            call(
                "sceNetBind",
                [fd, wanted.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0
        );
        assert_eq!(call("sceNetListen", [fd, 8, 0, 0, 0, 0]), 0);

        let mut into = [0_u8; 16];
        assert_eq!(
            call(
                "sceNetRecv",
                [fd, into.as_mut_ptr() as u64, into.len() as u64, 0, 0, 0]
            ),
            u64::from(
                GuestError::vendor_in(NET_ERROR_BASE, orbistoun_core::errno::NOT_CONNECTED)
                    .as_raw()
            ),
            "the code the console answered for exactly this call"
        );
        assert_eq!(call("sceNetSocketClose", [fd, 0, 0, 0, 0, 0]), 0);
    }

    /// **A refusal is never the placeholder**, which is the shape the bug had.
    ///
    /// The negative test. `0x7fff0001` says "nothing implements this"; a served call that
    /// failed has to say something else, or the report cannot tell a missing function from a
    /// working one that was asked for the impossible.
    #[test]
    fn a_served_call_never_answers_the_unimplemented_placeholder() {
        let mut into = [0_u8; 16];
        // Descriptor 4096 is above the table's ceiling, so it names nothing.
        let answered = call(
            "sceNetRecv",
            [4096, into.as_mut_ptr() as u64, into.len() as u64, 0, 0, 0],
        );
        assert_ne!(
            answered,
            u64::from(GuestError::Unimplemented.as_raw()),
            "a call that ran and failed is not a call nobody wrote"
        );
        assert_eq!(
            answered & u64::from(0xFFFF_FF00_u32),
            u64::from(NET_ERROR_BASE),
            "and it is still in this library's numbering"
        );
    }
}
