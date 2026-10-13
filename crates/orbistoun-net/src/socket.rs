//! `libSceNet` - sockets.
//!
//! The bodies live in `orbistoun-fs`, which owns the descriptor table files and sockets
//! share. The two spellings differ only on failure: a POSIX `bind` answers `-1` and sets
//! `errno`, `sceNetBind` answers [`NET_ERROR_BASE`]` | errno`. So the bodies carry the errno
//! out as `orbistoun_fs::socket::Answer` and each crate encodes it its own way (D525).
//!
//! Arities other than `6` are the types obSCEne's `src/probe/sections/net.c` calls through:
//! they establish the first N arguments, not that there is no further one. The resolvers,
//! datagram calls are declared and unserved. A failure also writes its errno to the slot
//! `sceNetErrnoLoc` answers.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_fs::socket::{self as bodies, Answer};
use orbistoun_hle::guest_module;

use super::NET_ERROR_BASE;

/// A body's answer in this library's spelling: the value, or `0x8041_0100 | errno`.
///
/// The counterpart to `orbistoun_fs::socket::as_posix`. Zero-extended to sixty-four bits,
/// because the guest compares the code as a thirty-two-bit `int`.
#[must_use]
fn as_vendor(answer: Answer) -> u64 {
    match answer {
        Ok(value) => value,
        Err(errno) => refuse(errno),
    }
}

thread_local! {
    /// This library's errno, the slot `sceNetErrnoLoc` answers. A failing call writes it and a
    /// succeeding one leaves it (measured, `-7af9`). Per thread is assumed: only two calls on one
    /// thread were compared, and they answered the same address.
    static NET_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// A failure in this library's spelling, `0x8041_0100 | errno`, with the errno written to the
/// slot `sceNetErrnoLoc` answers.
fn refuse(errno: u32) -> u64 {
    NET_ERRNO.with(|slot| slot.set(i32::try_from(errno).unwrap_or(i32::MAX)));
    u64::from(GuestError::vendor_in(NET_ERROR_BASE, errno).as_raw())
}

/// `sceNetErrnoLoc()`: the address of this thread's library errno - the same address on every call
/// from a thread (measured: `ptrs-equal 1`). A failing `sceNetSocketClose(9999)` wrote 9 there and
/// a `sceNetInetPton` that then succeeded left it 9.
fn errno_location(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NET_ERRNO.with(|slot| slot.as_ptr() as usize as u64)
}

/// One `GuestFn` per errno-bearing body, in this library's spelling.
///
/// `orbistoun-fs` has the matching macro for the POSIX spelling; each crate knows only its own
/// encoding.
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
/// The leading name shifts every argument by one register, so this cannot share the POSIX
/// body unshifted. The name labels the socket for the platform's accounting and is not kept:
/// hardware answers the same descriptor for a name and for `NULL`.
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
    vendor_sendto => sendto,
    vendor_recvfrom => recvfrom,
    vendor_close => close,
}

/// The address families `sceNetInetPton` reads: IPv4, and this platform's IPv6 (28).
const PTON_INET: u64 = 2;
const PTON_INET6: u64 = 28;

/// The longest text `sceNetInetPton` reads; a valid address is far shorter.
const PTON_TEXT_MAX: usize = 64;

/// The address bytes `text` spells in `family`, or `None` for text that is not one - as a console
/// read the arms obSCEne measured (`REQ-20261012T1816Z-7af9`, `000-boot/net-inet-pton`,
/// `report-1791836505.txt` lines 837-907). IPv4 is four decimal parts of at most 255 with no
/// leading zero (`01.2.3.4` is refused), and trailing whitespace is tolerated (`1.2.3.4 ` reads)
/// while leading whitespace is not. IPv6 is RFC 4291 text with at most one `::` and an optional
/// dotted tail, which Rust's parser reads the same on every measured arm.
fn pton(family: u64, text: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(text).ok()?;
    match family {
        PTON_INET => text
            .trim_end_matches(|c: char| c.is_ascii_whitespace())
            .parse::<std::net::Ipv4Addr>()
            .ok()
            .map(|address| address.octets().to_vec()),
        _ => text
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|address| address.octets().to_vec()),
    }
}

/// `sceNetInetPton(family, src, dst)`: 1 with the 4 or 16 address bytes written, 0 for text that
/// is no address with `dst` untouched, `0x8041012f` for a family other than 2 or 28 (99 and 1
/// measured), `0x80410116` for a null `src`. A null `dst` with good text answers 1 and writes
/// nothing (measured). Each refusal writes its errno (`0x2f`, `0x16`, measured); text that is no
/// address leaves it.
fn inet_pton(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [family, src, dst, ..] = *args;
    if family != PTON_INET && family != PTON_INET6 {
        return refuse(0x2f);
    }
    if src == 0 {
        return refuse(0x16);
    }
    // SAFETY: the guest's text, read through the checked accessor up to its terminator.
    let Some(text) = (unsafe { orbistoun_mem::guest::read_cstr(src, PTON_TEXT_MAX) }) else {
        return 0;
    };
    let Some(address) = pton(family, &text) else {
        return 0;
    };
    // SAFETY: the guest's destination, as many bytes as the family's address.
    if dst != 0 && !unsafe { orbistoun_mem::guest::write_bytes(dst, &address) } {
        return refuse(0x16);
    }
    1
}

/// Implementations this module provides, by symbol name.
///
/// The names obSCEne's `102-net/resolve` calls, plus `sceNetGetsockname`, which a server
/// bound to port zero uses to learn its port.
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
        ("sceNetSendto", vendor_sendto),
        ("sceNetRecvfrom", vendor_recvfrom),
        ("sceNetSocketClose", vendor_close),
        // A byte swap cannot fail, so there is no errno to encode and the POSIX body is the whole
        // function.
        ("sceNetHtons", bodies::htons),
        ("sceNetInetPton", inet_pton),
        ("sceNetErrnoLoc", errno_location),
    ]
}

guest_module! {
    "libSceNet" {
        "sceNetAccept" => 3,
        "sceNetBind" => 3,
        "sceNetConnect" => 3,
        "sceNetErrnoLoc" => 0,
        "sceNetGetsockname" => 3,
        "sceNetHtons" => 1,
        // (family, src, dst)
        "sceNetInetPton" => 3,
        "sceNetInit" => 1,
        "sceNetListen" => 2,
        // (name, size, flags) and (pool): served by `pool.rs`.
        "sceNetPoolCreate" => 3,
        "sceNetPoolDestroy" => 1,
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
        "sceNetTerm" => 0,
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
    use orbistoun_fs::socket::{SOCKADDR_IN_LEN, af_inet, sock_stream};

    use super::super::NET_ERROR_BASE;

    /// A socket, opened the way a title opens one: `sceNetSocket(name, family, type, protocol)`,
    /// with a `NULL` name.
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

    /// The first argument of `sceNetSocket` is a name, not the address family.
    ///
    /// Asserted from both sides: called as a title calls it, a descriptor; called POSIX-style, a
    /// refusal. The signature is obSCEne's `src/probe/sections/net.c`.
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
    /// Every name obSCEne resolves in the title leg is served here, named individually (D667).
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

    /// Nothing is served that is not declared, since an undeclared name is unreachable from a
    /// guest.
    #[test]
    fn every_served_name_is_declared() {
        for (name, _) in super::implementations() {
            assert!(
                super::MODULE.imports.iter().any(|i| i.name == *name),
                "{name} is served but not declared"
            );
        }
    }

    /// The vendor spelling binds with a `sin_len` of 0 or 16 (`102-net/sockaddr-bind`): the
    /// platform does not validate that byte.
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

    /// A failure comes back in this library's numbering, not POSIX's `-1`.
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

    /// A refusal is never the placeholder, so a report can tell a missing function from a failed
    /// call.
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
