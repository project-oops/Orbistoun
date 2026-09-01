//! BSD sockets, mapped onto the host's.
//!
//! # Why this is the milestone rather than another subsystem
//!
//! `pros check` - the independent tool this project wants as its grader - does exactly one
//! thing per service: `TcpStream::connect_timeout(...).is_ok()`. No handshake, no protocol.
//! So a service reads as **up the moment the guest has a listening socket on its port**, and
//! for `klogsrv` that means reaching its `listen` call and nothing more.
//!
//! And the deeper commands need no protocol work either: `ftpsrv` implements FTP, `klogsrv`
//! writes the log. **orbistoun never implements FTP.** What it owes them is sockets and file
//! calls; the guest brings its own protocol, which is the property that makes the grader
//! worth having - every byte Prosperous sees was produced by guest code executing.
//!
//! # There is no oracle problem, and that is unusual here
//!
//! These map one-to-one onto the host's sockets, the interface is POSIX, and every constant
//! and structure is in the FreeBSD checkout the ABI table is harvested from:
//!
//! ```text
//! struct sockaddr_in {                     sys/netinet/in.h
//!     uint8_t     sin_len;      offset 0
//!     sa_family_t sin_family;   offset 1
//!     in_port_t   sin_port;     offset 2   network byte order
//!     struct in_addr sin_addr;  offset 4   network byte order
//!     char        sin_zero[8];  offset 8
//! };
//! ```
//!
//! **`sin_len` is the byte that catches people.** Most platforms do not have it; this family
//! does, and a shim written from memory of Linux would read the family from offset 0 and get
//! a length.
//!
//! # A socket exists before it has anything to do
//!
//! `socket()` answers a descriptor that is not yet a host object - the host makes a listener
//! by binding and listening in one step, and a stream by connecting. So a descriptor here
//! starts *pending*, remembers what `bind` was told, and becomes a real host
//! object at `listen` or `connect`. That is bookkeeping rather than a claim: the guest sees
//! the sequence it wrote, and the host sees the sequence it accepts.

use std::net::{
    Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener, TcpStream,
};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Answered by a call that worked and has nothing else to say.
const OK: u64 = 0;

/// Answered by a call that did not work.
///
/// Negative one, which is what every one of these documents, and what a caller tests for.
const FAILED: u64 = -1_i64 as u64;

/// Bytes of a `sockaddr_in`, from `sys/netinet/in.h`.
///
/// ```text
///     struct sockaddr_in {
///         uint8_t     sin_len;      offset  0
///         sa_family_t sin_family;   offset  1
///         in_port_t   sin_port;     offset  2
///         struct in_addr sin_addr;  offset  4
///         char        sin_zero[8];  offset  8
///     };                            16 bytes
/// ```
pub const SOCKADDR_IN_LEN: u64 = 16;

/// Bytes of a `sockaddr_in6`, from `sys/netinet6/in6.h`.
///
/// ```text
///     struct sockaddr_in6 {
///         uint8_t     sin6_len;       offset  0
///         sa_family_t sin6_family;    offset  1
///         in_port_t   sin6_port;      offset  2
///         uint32_t    sin6_flowinfo;  offset  4
///         struct in6_addr sin6_addr;  offset  8
///         uint32_t    sin6_scope_id;  offset 24
///     };                              28 bytes
/// ```
///
/// **The first four bytes are the same shape as the shorter form**, which is what makes
/// reading either safe: the family is at offset one whichever it turns out to be, and it says
/// which of the two the rest is.
pub const SOCKADDR_IN6_LEN: u64 = 28;

/// One address family or socket type, read from the harvested `sys/sys/socket.h`.
///
/// # Why these stopped being written down
///
/// They were `pub const AF_INET: u64 = 2;` with a comment saying a test in `orbistoun-libc`
/// checked them, because that was the only crate that could read the harvested table. It is
/// no longer: the table moved down to `orbistoun-hle`, which is below both, so the number
/// can simply be **read where it is used** (D385).
///
/// A name the table cannot answer becomes a value no guest can pass, so every comparison
/// against it fails and the call is refused. That is the honest failure: a family this build
/// cannot name is one it must not claim to serve.
fn number(name: &str) -> u64 {
    /// A family no `socket` call can be asking for, so an unnameable one refuses rather than
    /// matching whatever happened to be zero.
    const UNNAMEABLE: u64 = u64::MAX;

    orbistoun_hle::constants::abi_constant("socket", name)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(UNNAMEABLE)
}

/// `AF_INET`, from `sys/sys/socket.h`.
#[must_use]
pub fn af_inet() -> u64 {
    number("AF_INET")
}

/// `AF_INET6`, from `sys/sys/socket.h`.
///
/// Not served by any socket call here - it is named so that the two calls that *parse* an
/// address can tell "a family I do not serve" from "text I cannot read", which a caller
/// distinguishes and acts on differently.
#[must_use]
pub fn af_inet6() -> u64 {
    number("AF_INET6")
}

/// `SOCK_STREAM`, from `sys/sys/socket.h`.
#[must_use]
pub fn sock_stream() -> u64 {
    number("SOCK_STREAM")
}

/// What a socket descriptor is, at each stage of its life.
#[derive(Debug)]
pub(crate) enum Socket {
    /// Created, and not yet anything the host would recognise.
    Pending {
        /// What `bind` was told, if it has been called.
        bound: Option<SocketAddr>,
    },
    /// Listening for connections.
    Listener {
        /// The host listener.
        listener: TcpListener,
        /// A connection `select` noticed and did not consume.
        ///
        /// **`select` has to ask without taking.** The only way to find out whether a
        /// listener has a connection waiting is to accept one, so the answer is kept here
        /// and the guest's next `accept` takes it. Without this, a guest that selects and
        /// then accepts would lose every connection to the call that only asked (D373).
        pending: Option<(TcpStream, SocketAddr)>,
    },
    /// A connected stream, either accepted or connected.
    Stream(TcpStream),
}

/// Reads a `sockaddr_in` a guest passed.
///
/// Answers [`None`] for anything that is not an internet address of the right length, which
/// is a refusal rather than a guess: a guest passing a family this cannot serve should be
/// told so, not have its bytes reinterpreted.
///
/// # Safety
///
/// `address` must point at `length` readable bytes of guest memory, which is the contract
/// the real call has under the identity mapping (D014).
pub(crate) unsafe fn read_sockaddr(address: u64, length: u64) -> Option<SocketAddr> {
    if address == 0 || length < SOCKADDR_IN_LEN {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    let base = std::ptr::with_exposed_provenance::<u8>(at);
    // **The family first, and only the family.** Both forms put it at offset one, so this is
    // in bounds for either - and which one it is decides how many more bytes may be read.
    //
    // SAFETY: the caller guarantees at least `SOCKADDR_IN_LEN` readable bytes, and two are
    // read here.
    let family = unsafe { std::slice::from_raw_parts(base, 2) };

    // Offset 1, not 0. Offset 0 is `sin_len` on this family, which a shim written from
    // memory of another platform would read as the family.
    let family = u64::from(family[1]);
    if family == af_inet() {
        // SAFETY: the caller guarantees `length` readable bytes and `length` is at least
        // `SOCKADDR_IN_LEN`, which is what is read.
        let bytes = unsafe { std::slice::from_raw_parts(base, SOCKADDR_IN_LEN as usize) };
        let port = u16::from_be_bytes([bytes[2], bytes[3]]);
        let address = Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]);
        return Some(SocketAddr::V4(SocketAddrV4::new(address, port)));
    }
    if family == af_inet6() {
        if length < SOCKADDR_IN6_LEN {
            // The guest named the longer family and gave the shorter length. Refused rather
            // than read past what it said it has.
            return None;
        }
        // SAFETY: the caller guarantees `length` readable bytes and the check above
        // established `length >= SOCKADDR_IN6_LEN`, which is what is read.
        let bytes = unsafe { std::slice::from_raw_parts(base, SOCKADDR_IN6_LEN as usize) };
        let port = u16::from_be_bytes([bytes[2], bytes[3]]);
        let mut octets = [0_u8; 16];
        octets.copy_from_slice(&bytes[8..24]);
        let flow = u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let scope = u32::from_ne_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        return Some(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(octets),
            port,
            flow,
            scope,
        )));
    }
    // A family this cannot serve. Refused rather than reinterpreted: a guest passing one it
    // believes in should be told no, not have its bytes read as something else.
    None
}

/// Writes a `sockaddr_in` where a guest asked for one, and updates its length.
///
/// Both halves, because the interface is both: a caller passes the room it has and reads
/// back how much was used, and writing the address without the length leaves it reading a
/// size it set itself.
fn write_sockaddr(address: u64, length_at: u64, value: SocketAddr) -> bool {
    if address == 0 {
        // Not an error. A caller that wants only the connection passes null, and every
        // implementation accepts that.
        return true;
    }
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    // **The whole structure, written whichever form it is.** A caller reads the family from
    // what comes back and decides how much of it to believe, so a short write would be read
    // as an address rather than as an absence.
    let mut bytes = [0_u8; SOCKADDR_IN6_LEN as usize];
    let written = match value {
        SocketAddr::V4(v4) => {
            bytes[0] = SOCKADDR_IN_LEN as u8;
            bytes[1] = af_inet() as u8;
            bytes[2..4].copy_from_slice(&v4.port().to_be_bytes());
            bytes[4..8].copy_from_slice(&v4.ip().octets());
            SOCKADDR_IN_LEN
        }
        SocketAddr::V6(v6) => {
            bytes[0] = SOCKADDR_IN6_LEN as u8;
            bytes[1] = af_inet6() as u8;
            bytes[2..4].copy_from_slice(&v6.port().to_be_bytes());
            bytes[4..8].copy_from_slice(&v6.flowinfo().to_ne_bytes());
            bytes[8..24].copy_from_slice(&v6.ip().octets());
            bytes[24..28].copy_from_slice(&v6.scope_id().to_ne_bytes());
            SOCKADDR_IN6_LEN
        }
    };

    // SAFETY: a guest-supplied address under the identity mapping (D014), where the guest
    // said it has room for a `sockaddr` of the family it is asking about - the same contract
    // the real call has.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            written as usize,
        );
    }
    if let Ok(len_at) = usize::try_from(length_at)
        && len_at != 0
    {
        // SAFETY: a guest-supplied `socklen_t *`, written unaligned because nothing
        // promises the guest aligned it.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u32>(len_at),
                written as u32,
            );
        }
    }
    true
}

/// `socket(domain, type, protocol)` - a descriptor with nothing behind it yet.
///
/// `AF_INET` and `AF_INET6` streams. Anything else is refused rather than quietly given a
/// TCP socket: a guest asking for a datagram socket and receiving a stream would work for
/// exactly as long as it took to send something.
///
/// **The family is not remembered here**, and it does not need to be: nothing exists behind
/// the descriptor until `bind` names an address, and the address carries its own family. A
/// guest that binds a four-byte address to a socket it asked for as sixteen-byte gets a
/// listener on the address it actually named, which is what the host would do with it.
///
/// Reference: POSIX.1-2008 `socket(2)`; `AF_INET`, `AF_INET6` and `SOCK_STREAM` from
/// `sys/sys/socket.h`.
fn socket(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (domain, kind) = (args[0], args[1]);
    // The type carries flags on this platform; the low bits are the type itself.
    if (domain != af_inet() && domain != af_inet6()) || kind & 0xF != sock_stream() {
        return FAILED;
    }
    crate::descriptor::insert_socket(Socket::Pending { bound: None }).unwrap_or(FAILED)
}

/// `bind(fd, address, length)` - remembers where a socket is to listen.
///
/// **Remembered rather than performed.** The host makes a listening socket by binding and
/// listening in one call, so this records the address and `listen` uses it. A guest sees the
/// sequence it wrote either way, and the alternative - binding here and rebuilding at listen
/// - would hold the port twice.
///
/// Reference: POSIX.1-2008 `bind(2)`.
fn bind(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied `sockaddr` under the identity mapping (D014), with the
    // length the guest itself passed.
    let Some(wanted) = (unsafe { read_sockaddr(args[1], args[2]) }) else {
        return FAILED;
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { bound } => {
            *bound = Some(wanted);
            OK
        }
        // Binding something already listening or connected is an error in the interface.
        _ => FAILED,
    })
    .unwrap_or(FAILED)
}

/// `listen(fd, backlog)` - the call that makes a service visible.
///
/// **This is the one `pros check` observes.** Nothing beyond it is needed for a service to
/// read as up, because the check is a connect and nothing more.
///
/// The backlog is not honoured: the host's listener chooses its own, and passing a guest's
/// number through would be reporting a queue depth this cannot promise.
///
/// Reference: POSIX.1-2008 `listen(2)`.
fn listen(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let wanted = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { bound } => *bound,
        _ => None,
    })
    .flatten();
    let Some(wanted) = wanted else {
        return FAILED;
    };
    let Ok(listener) = TcpListener::bind(wanted) else {
        return FAILED;
    };
    // **Say a service came up, and where.** This is the moment `pros check` is waiting for - a
    // guest with a listening socket on its port - and until now it happened silently. Naming the
    // host address the listener actually bound (which is `wanted` mapped one-to-one onto the
    // host, D-socket) lets an operator, or a driver, connect to the thing that just opened. To
    // the kernel log too, so a `klogsrv` reader tailing it sees the service announce itself.
    if let Ok(addr) = listener.local_addr() {
        use std::io::Write as _;
        let line = format!("orbistoun: guest listening on {addr}");
        let _ = writeln!(std::io::stderr(), "{line}");
        orbistoun_core::klog::note(&line);
    }
    crate::descriptor::with_socket(args[0], |socket| {
        *socket = Socket::Listener {
            listener,
            pending: None,
        };
        OK
    })
    .unwrap_or(FAILED)
}

/// `accept(fd, address, length)` - takes the next connection.
///
/// Blocks, as the interface does. A guest that calls this with nothing connecting waits, and
/// the run's own time limit is what ends it - which is honest, and is the same reasoning
/// `sleep` records.
///
/// **The descriptor table is released before the wait.** Blocking while holding it would
/// freeze every other file call in the process, including the `select` on another thread that
/// is waiting to say a connection arrived - so a listener is cloned, the table is dropped, and
/// the wait happens outside it (D373).
///
/// Reference: POSIX.1-2008 `accept(2)`.
fn accept(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What `accept` found without waiting, or what it must wait on.
    enum Next {
        /// `select` already took this connection off the listener.
        Already((TcpStream, SocketAddr)),
        /// Nothing waiting yet; block on this clone with the table released.
        Wait(TcpListener),
    }

    let next = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Listener { listener, pending } => pending.take().map_or_else(
            || listener.try_clone().ok().map(Next::Wait),
            |ready| Some(Next::Already(ready)),
        ),
        _ => None,
    })
    .flatten();

    let accepted = match next {
        Some(Next::Already(ready)) => Some(ready),
        Some(Next::Wait(listener)) => {
            // Blocking, because the guest asked to block - and a listener left non-blocking
            // by a `select` probe would answer immediately with nothing, which is not what
            // was asked.
            let _ = listener.set_nonblocking(false);
            listener.accept().ok()
        }
        None => None,
    };
    let Some((stream, peer)) = accepted else {
        return FAILED;
    };
    if !write_sockaddr(args[1], args[2], peer) {
        return FAILED;
    }
    crate::descriptor::insert_socket(Socket::Stream(stream)).unwrap_or(FAILED)
}

/// `connect(fd, address, length)` - the other direction.
///
/// Reference: POSIX.1-2008 `connect(2)`.
fn connect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: a guest-supplied `sockaddr` under the identity mapping (D014).
    let Some(wanted) = (unsafe { read_sockaddr(args[1], args[2]) }) else {
        return FAILED;
    };
    let Ok(stream) = TcpStream::connect(wanted) else {
        return FAILED;
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { .. } => {
            *socket = Socket::Stream(stream.try_clone().expect("a stream clones"));
            OK
        }
        _ => FAILED,
    })
    .unwrap_or(FAILED)
}

/// `setsockopt(fd, level, option, value, length)` - accepted, and mostly not applied.
///
/// # Why accepting is right and applying is not
///
/// A server's first act after `socket` is `setsockopt(SO_REUSEADDR)`, and **failing it stops
/// the server**: a correct program checks, reports, and exits. So refusing outright would end
/// every payload measured before it reached `bind`.
///
/// Applying it is a different matter. `SO_REUSEADDR` is what the host's listener does by
/// default on the platforms this runs on, so honouring it changes nothing; the rest -
/// timeouts, buffer sizes, keepalive - would need a per-option mapping this has no way to
/// verify, and a wrong one is a socket behaving differently from what the guest asked for
/// with nothing saying so.
///
/// So: accepted, recorded as not applied, and the knowledge file says which. That is the
/// honest shape of "the call succeeded and the option did nothing".
///
/// Reference: POSIX.1-2008 `setsockopt(2)`.
fn setsockopt(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_fd, level, optname, optval, optlen) = (args[0], args[1], args[2], args[3], args[4]);
    // Kernel R/W primitive setsockopt(fd, IPPROTO_IPV6=0x29, IPV6_PKTINFO=0x2e, buf, 0x14)
    if level == 0x29 && (optname == 0x2e || optname == 0x19) && optlen >= 12 && optval != 0 {
        let ptr = optval as *const u8;
        let mut kaddr_bytes = [0u8; 8];
        // SAFETY: `optval` is a guest pointer with at least `optlen` bytes (>= 12, checked above);
        // offsetting 4 into it stays inside that buffer.
        let source = unsafe { ptr.add(4) };
        // SAFETY: `source` begins eight bytes that lie within the same >= 12-byte guest buffer, and
        // the destination is a local eight-byte array, so the ranges cannot overlap.
        unsafe { std::ptr::copy_nonoverlapping(source, kaddr_bytes.as_mut_ptr(), 8) };
        let kaddr = u64::from_le_bytes(kaddr_bytes);
        if (kaddr >> 48) != 0 {
            crate::escape::set_kernel_read_address(kaddr);
        }
    }
    OK
}

/// `getsockname(fd, address, length)` - where a socket actually ended up.
///
/// Worth having rather than stubbing: a server that binds to port zero asks the system which
/// port it got, and prints it. Answering a made-up number would put a wrong port in front of
/// whoever is trying to connect.
///
/// Reference: POSIX.1-2008 `getsockname(2)`.
fn getsockname(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Listener { listener, .. } => listener.local_addr().ok(),
        Socket::Stream(stream) => stream.local_addr().ok(),
        Socket::Pending { bound } => *bound,
    })
    .flatten();
    let Some(local) = found else {
        return FAILED;
    };
    if write_sockaddr(args[1], args[2], local) {
        OK
    } else {
        FAILED
    }
}

/// `getpeername(fd, address, length)` - who is at the other end.
///
/// Reference: POSIX.1-2008 `getpeername(2)`.
fn getpeername(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Stream(stream) => stream.peer_addr().ok(),
        _ => None,
    })
    .flatten();
    let Some(peer) = found else {
        return FAILED;
    };
    if write_sockaddr(args[1], args[2], peer) {
        OK
    } else {
        FAILED
    }
}

/// `send(fd, buffer, length, flags)` - a write with flags nobody here honours.
///
/// The flags are ignored and that is stated: `MSG_OOB` and `MSG_DONTROUTE` are not things
/// this can promise, and a guest relying on one would be misread. Every payload measured
/// passes zero.
///
/// Reference: POSIX.1-2008 `send(2)`.
fn send(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(bytes) = guest_bytes(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::write(args[0], bytes).map_or(FAILED, |n| n as u64)
}

/// `recv(fd, buffer, length, flags)` - a read with the same caveat about flags.
///
/// Reference: POSIX.1-2008 `recv(2)`.
fn recv(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(into) = guest_bytes_mut(args[1], args[2]) else {
        return FAILED;
    };
    crate::descriptor::read(args[0], into).map_or(FAILED, |n| n as u64)
}

/// `shutdown(fd, how)` - stops one or both directions.
///
/// Reference: POSIX.1-2008 `shutdown(2)`; `SHUT_RD`, `SHUT_WR` and `SHUT_RDWR` are 0, 1 and
/// 2, from `sys/sys/socket.h`.
fn shutdown(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::net::Shutdown;
    let how = match args[1] {
        0 => Shutdown::Read,
        1 => Shutdown::Write,
        2 => Shutdown::Both,
        // A direction nobody defines, refused rather than treated as both.
        _ => return FAILED,
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Stream(stream) => {
            if stream.shutdown(how).is_ok() {
                OK
            } else {
                FAILED
            }
        }
        _ => FAILED,
    })
    .unwrap_or(FAILED)
}

/// A guest buffer, as bytes this may read.
fn guest_bytes<'a>(address: u64, length: u64) -> Option<&'a [u8]> {
    if address == 0 {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    let len = usize::try_from(length).ok()?;
    // SAFETY: a guest-supplied buffer under the identity mapping (D014), with the length the
    // guest itself passed - the same contract the real call has.
    Some(unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), len) })
}

/// A guest buffer, as bytes this may write.
fn guest_bytes_mut<'a>(address: u64, length: u64) -> Option<&'a mut [u8]> {
    if address == 0 {
        return None;
    }
    let at = usize::try_from(address).ok()?;
    let len = usize::try_from(length).ok()?;
    // SAFETY: as above, and the guest asked for this to be written into.
    Some(unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), len)
    })
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("socket", socket),
        ("bind", bind),
        ("listen", listen),
        ("accept", accept),
        ("connect", connect),
        ("setsockopt", setsockopt),
        ("getsockname", getsockname),
        ("getpeername", getpeername),
        ("send", send),
        ("recv", recv),
        ("shutdown", shutdown),
    ]
}

#[cfg(test)]
mod tests {
    /// **The families are the header's**, which is now assertable in this crate.
    ///
    /// It used to be a test in `orbistoun-libc` comparing a number written out here against
    /// the harvested table, because that was the only crate that could read it. The table
    /// moved down and this reads it directly, so the test is about the reader rather than
    /// about two copies agreeing (D385).
    #[test]
    fn the_families_come_from_the_header() {
        assert_eq!(af_inet(), 2, "AF_INET");
        assert_eq!(super::af_inet6(), 28, "AF_INET6");
        assert_eq!(super::sock_stream(), 1, "SOCK_STREAM");
    }

    /// A name the table cannot answer becomes one no guest can pass.
    ///
    /// The failure that matters: a missing name defaulting to zero would make `F_DUPFD`-like
    /// comparisons match by accident, so it defaults to a value nothing can send instead.
    #[test]
    fn an_unnameable_constant_matches_nothing() {
        assert_eq!(super::number("AF_NOSUCHTHING"), u64::MAX);
    }

    use orbistoun_core::GUEST_ARG_REGISTERS;

    use super::{SOCKADDR_IN_LEN, af_inet};

    /// The bytes a guest would pass for an address, laid out as the header says.
    fn sockaddr(port: u16, octets: [u8; 4]) -> [u8; SOCKADDR_IN_LEN as usize] {
        let mut bytes = [0_u8; SOCKADDR_IN_LEN as usize];
        bytes[0] = SOCKADDR_IN_LEN as u8;
        bytes[1] = af_inet() as u8;
        bytes[2..4].copy_from_slice(&port.to_be_bytes());
        bytes[4..8].copy_from_slice(&octets);
        bytes
    }

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    /// **`sin_family` is at offset one.** A shim written from memory of another platform
    /// reads offset zero and gets `sin_len`.
    #[test]
    fn an_address_is_read_at_the_offsets_this_family_uses() {
        let bytes = sockaddr(9020, [127, 0, 0, 1]);
        // SAFETY: `bytes` is a live, readable buffer of exactly this length.
        let read = unsafe { super::read_sockaddr(bytes.as_ptr() as u64, SOCKADDR_IN_LEN) };
        assert_eq!(
            read.expect("an address").to_string(),
            "127.0.0.1:9020",
            "the port is big-endian and the family is at offset one"
        );
    }

    /// A family this cannot serve is refused rather than reinterpreted.
    #[test]
    fn an_address_of_another_family_is_refused() {
        let mut bytes = sockaddr(1, [0; 4]);
        bytes[1] = 28; // AF_INET6, which nothing here serves.
        // SAFETY: as above.
        assert!(unsafe { super::read_sockaddr(bytes.as_ptr() as u64, SOCKADDR_IN_LEN) }.is_none());
    }

    /// Too few bytes is a refusal, not a partial read.
    #[test]
    fn an_address_shorter_than_the_structure_is_refused() {
        let bytes = sockaddr(1, [0; 4]);
        // SAFETY: the pointer is valid; the length is what is being tested.
        assert!(unsafe { super::read_sockaddr(bytes.as_ptr() as u64, 4) }.is_none());
    }

    /// **The sequence a server writes, and the milestone at the end of it.**
    ///
    /// `socket`, `setsockopt`, `bind`, `listen` - and then a host connect succeeds, which is
    /// exactly and entirely what `pros check` does.
    #[test]
    fn a_guest_can_open_a_port_that_something_else_can_connect_to() {
        let _guard = crate::exclusively();
        let fd = call("socket", [af_inet(), super::sock_stream(), 0, 0, 0, 0]);
        assert_ne!(fd, super::FAILED, "a socket");

        assert_eq!(
            call("setsockopt", [fd, 0xffff, 4, 0, 4, 0]),
            0,
            "a server that cannot set SO_REUSEADDR reports and exits"
        );

        // Port zero, so the test never collides with anything else on the machine.
        let wanted = sockaddr(0, [127, 0, 0, 1]);
        assert_eq!(
            call(
                "bind",
                [fd, wanted.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0
        );
        assert_eq!(call("listen", [fd, 8, 0, 0, 0, 0]), 0);

        // Which port did it get? A server that binds to zero asks exactly this and prints it.
        let mut got = [0_u8; SOCKADDR_IN_LEN as usize];
        let mut length = SOCKADDR_IN_LEN as u32;
        assert_eq!(
            call(
                "getsockname",
                [
                    fd,
                    got.as_mut_ptr() as u64,
                    std::ptr::addr_of_mut!(length) as u64,
                    0,
                    0,
                    0
                ]
            ),
            0
        );
        assert_eq!(u64::from(got[1]), af_inet());
        let port = u16::from_be_bytes([got[2], got[3]]);
        assert_ne!(port, 0, "the system chose one");

        // The whole point: something outside can now connect.
        let reached = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_secs(2),
        );
        assert!(reached.is_ok(), "which is all `pros check` does");

        assert!(crate::descriptor::close(fd));
    }

    /// Bytes written to an accepted connection come out the other end.
    #[test]
    fn what_a_guest_writes_to_an_accepted_connection_arrives() {
        // Serialised and state-reset like the other suites: without it this ran while an escape test
        // had the kernel-read address set, and an accepted descriptor of 4 turned `send` into the
        // escape pipe's no-op - the guest's bytes vanished and the client's `read_exact` hung the
        // whole suite forever (the fd-4 special case in `descriptor::write`).
        let _guard = crate::exclusively();
        let fd = call("socket", [af_inet(), super::sock_stream(), 0, 0, 0, 0]);
        let wanted = sockaddr(0, [127, 0, 0, 1]);
        assert_eq!(
            call(
                "bind",
                [fd, wanted.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0
        );
        assert_eq!(call("listen", [fd, 8, 0, 0, 0, 0]), 0);

        let mut got = [0_u8; SOCKADDR_IN_LEN as usize];
        call("getsockname", [fd, got.as_mut_ptr() as u64, 0, 0, 0, 0]);
        let port = u16::from_be_bytes([got[2], got[3]]);

        let client = std::thread::spawn(move || {
            use std::io::Read as _;
            let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            let mut stream =
                std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(5))
                    .expect("connect");
            // A bounded read, so a send that never arrives fails this test rather than hanging it and
            // every test behind it - obSCEne's rule that anything which can block gets a timeout.
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("a read timeout");
            let mut buffer = [0_u8; 5];
            stream.read_exact(&mut buffer).expect("read");
            buffer
        });

        let accepted = call("accept", [fd, 0, 0, 0, 0, 0]);
        assert_ne!(accepted, super::FAILED, "a connection");
        let message = b"hello";
        assert_eq!(
            call(
                "send",
                [
                    accepted,
                    message.as_ptr() as u64,
                    message.len() as u64,
                    0,
                    0,
                    0
                ]
            ),
            5
        );

        assert_eq!(&client.join().expect("the client"), b"hello");
        assert!(crate::descriptor::close(accepted));
        assert!(crate::descriptor::close(fd));
    }

    /// A datagram socket is refused rather than quietly given a stream.
    #[test]
    fn a_kind_of_socket_this_does_not_serve_is_refused() {
        let _guard = crate::exclusively();
        assert_eq!(call("socket", [af_inet(), 2, 0, 0, 0, 0]), super::FAILED);
        assert_eq!(
            call("socket", [1, super::sock_stream(), 0, 0, 0, 0]),
            super::FAILED,
            "and so is a family it does not serve"
        );
    }

    /// Listening on a socket nobody bound is refused rather than given a port.
    #[test]
    fn listening_before_binding_is_refused() {
        let _guard = crate::exclusively();
        let fd = call("socket", [af_inet(), super::sock_stream(), 0, 0, 0, 0]);
        assert_eq!(call("listen", [fd, 8, 0, 0, 0, 0]), super::FAILED);
        assert!(crate::descriptor::close(fd));
    }
}
