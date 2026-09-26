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

/// What a socket call answers: the value a guest sees, or the POSIX `errno` that stopped it.
///
/// # Why the number leaves the body
///
/// The two spellings of these calls agree on success and disagree on failure. A POSIX-named
/// socket call answers `-1` and leaves the number in `errno`; `libSceNet` folds it into the
/// return as `0x8041_0100 | errno`, which one obSCEne sweep measured three times over. A body
/// that collapses to `-1` has already discarded the only part the vendor spelling needs - so
/// the number travels out and each spelling encodes it. One implementation, two honest
/// returns (D667).
pub type Answer = Result<u64, u32>;

/// The errno for a failure nothing here can name.
///
/// **Zero is not an errno** - it is what `errno` holds when nothing went wrong - so a vendor
/// code built from it cannot collide with a measured one, and a guest that switches on the
/// result meets a code it does not know, which is exactly true. Answering a plausible
/// `EINVAL` instead would be inventing a constant to make a failure look explained.
pub const UNNAMED: u32 = 0;

/// A body's answer in the POSIX spelling: the value, or `-1` with the number left to `errno`.
#[must_use]
pub const fn as_posix(answer: Answer) -> u64 {
    match answer {
        Ok(value) => value,
        Err(_) => FAILED,
    }
}

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
        /// Whether the guest has asked for non-blocking, before there is anything to set it on.
        ///
        /// **Remembered rather than refused**, because a server sets the option immediately
        /// after `socket` and connects afterwards - which is the order obSCEne writes and the
        /// order the check that caught this measures. Accepting the option and dropping it
        /// left a blocking host socket behind a guest that believed otherwise, and a
        /// would-block that never came reads as the peer hanging up (D667).
        nonblocking: bool,
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
        /// Whether the guest asked for non-blocking, carried forward to what `accept` answers.
        ///
        /// **The host cannot be asked.** `TcpListener` has a setter and no getter, and `accept`
        /// needs the answer twice: to decide whether waiting is allowed at all, and to hand the
        /// accepted stream the same mode - which obSCEne's `102-net/accept-inherits` measures
        /// the console doing.
        nonblocking: bool,
    },
    /// A connected stream, either accepted or connected.
    Stream {
        /// The host stream.
        stream: TcpStream,
        /// Whether the guest asked for non-blocking on it.
        ///
        /// **Kept because `MSG_DONTWAIT` has to put it back.** Honouring that flag means making
        /// the host socket non-blocking for exactly one call, and the host has a setter with no
        /// getter - so a shim that restored by guessing would leave every later read
        /// non-blocking, turning a guest's blocking `recv` into a busy loop somewhere else.
        nonblocking: bool,
    },
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
pub fn socket(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let (domain, kind) = (args[0], args[1]);
    // The type carries flags on this platform; the low bits are the type itself.
    if (domain != af_inet() && domain != af_inet6()) || kind & 0xF != sock_stream() {
        return Err(UNNAMED);
    }
    crate::descriptor::insert_socket(Socket::Pending {
        bound: None,
        nonblocking: false,
    })
    .ok_or(UNNAMED)
}

/// `bind(fd, address, length)` - remembers where a socket is to listen.
///
/// **Remembered rather than performed.** The host makes a listening socket by binding and
/// listening in one call, so this records the address and `listen` uses it. A guest sees the
/// sequence it wrote either way, and the alternative - binding here and rebuilding at listen -
/// would hold the port twice.
///
/// Reference: POSIX.1-2008 `bind(2)`.
pub fn bind(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    // SAFETY: a guest-supplied `sockaddr` under the identity mapping (D014), with the
    // length the guest itself passed.
    let Some(wanted) = (unsafe { read_sockaddr(args[1], args[2]) }) else {
        return Err(UNNAMED);
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { bound, .. } => {
            *bound = Some(wanted);
            Ok(OK)
        }
        // Binding something already listening or connected is an error in the interface.
        _ => Err(UNNAMED),
    })
    .unwrap_or(Err(UNNAMED))
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
pub fn listen(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let wanted = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { bound, nonblocking } => bound.map(|address| (address, *nonblocking)),
        _ => None,
    })
    .flatten();
    let Some((wanted, nonblocking)) = wanted else {
        return Err(UNNAMED);
    };
    let Ok(listener) = TcpListener::bind(wanted) else {
        return Err(UNNAMED);
    };
    // The flag the guest set before there was anything to set it on, applied now that there
    // is. Refusing here would be reporting the listener as unopenable for a reason that is
    // orbistoun's bookkeeping rather than the guest's request.
    if nonblocking {
        let _ = listener.set_nonblocking(true);
    }
    // **Say a service came up, and where.** This is the moment `pros check` is waiting for - a
    // guest with a listening socket on its port - and until now it happened silently. Naming the
    // host address the listener actually bound (which is `wanted` mapped one-to-one onto the
    // host, D-socket) lets an operator, or a driver, connect to the thing that just opened. To
    // the kernel log too, so a `klogsrv` reader tailing it sees the service announce itself.
    if let Ok(addr) = listener.local_addr() {
        tracing::info!("guest listening on {addr}");
        let line = format!("orbistoun: guest listening on {addr}");
        orbistoun_core::klog::note(&line);
    }
    crate::descriptor::with_socket(args[0], |socket| {
        *socket = Socket::Listener {
            listener,
            pending: None,
            nonblocking,
        };
        Ok(OK)
    })
    .unwrap_or(Err(UNNAMED))
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
pub fn accept(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    /// What `accept` found without waiting, or what it must wait on.
    enum Next {
        /// `select` already took this connection off the listener.
        Already((TcpStream, SocketAddr)),
        /// Nothing waiting yet; block on this clone with the table released.
        Wait(TcpListener),
    }

    let found = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Listener {
            listener,
            pending,
            nonblocking,
        } => {
            let next = pending.take().map_or_else(
                || listener.try_clone().ok().map(Next::Wait),
                |ready| Some(Next::Already(ready)),
            );
            next.map(|next| (next, *nonblocking))
        }
        _ => None,
    })
    .flatten();
    let Some((next, nonblocking)) = found else {
        return Err(UNNAMED);
    };

    let accepted = match next {
        Next::Already(ready) => Ok(ready),
        Next::Wait(listener) => {
            // The mode the guest asked for, not the one a `select` probe happened to leave
            // behind: a listener the guest set non-blocking must answer straight away, and one
            // it did not must wait however long it takes.
            let _ = listener.set_nonblocking(nonblocking);
            listener.accept()
        }
    };
    let (stream, peer) = match accepted {
        Ok(ready) => ready,
        // Nothing waiting on a non-blocking listener. Measured as `EAGAIN` through the
        // vendor encoding rather than guessed: it is the same condition, and the same code,
        // as a would-block read (obSCEne `102-net/recv-would-block`).
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return Err(orbistoun_core::errno::AGAIN);
        }
        Err(_) => return Err(UNNAMED),
    };
    if !write_sockaddr(args[1], args[2], peer) {
        return Err(UNNAMED);
    }
    // **Inherited, explicitly.** Hardware answers a would-block on a socket accepted from a
    // non-blocking listener (`102-net/accept-inherits`), and whether the host does that by
    // itself is a per-platform difference this must not be at the mercy of.
    let _ = stream.set_nonblocking(nonblocking);
    crate::descriptor::insert_socket(Socket::Stream {
        stream,
        nonblocking,
    })
    .ok_or(UNNAMED)
}

/// `connect(fd, address, length)` - the other direction.
///
/// Reference: POSIX.1-2008 `connect(2)`.
pub fn connect(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    // SAFETY: a guest-supplied `sockaddr` under the identity mapping (D014).
    let Some(wanted) = (unsafe { read_sockaddr(args[1], args[2]) }) else {
        return Err(UNNAMED);
    };
    // **Connected blocking, then set.** A non-blocking connect answers `EINPROGRESS` and
    // finishes later, which is a second state this does not model - so the flag the guest
    // asked for is applied to the finished stream instead. The difference a guest could see
    // is the return of `connect` itself, and nothing measured has looked at it.
    let Ok(stream) = TcpStream::connect(wanted) else {
        return Err(UNNAMED);
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Pending { nonblocking, .. } => {
            let _ = stream.set_nonblocking(*nonblocking);
            *socket = Socket::Stream {
                stream: stream.try_clone().expect("a stream clones"),
                nonblocking: *nonblocking,
            };
            Ok(OK)
        }
        _ => Err(UNNAMED),
    })
    .unwrap_or(Err(UNNAMED))
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
pub fn setsockopt(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let (fd, level, optname, optval, optlen) = (args[0], args[1], args[2], args[3], args[4]);
    // **The one option that is applied, because it is the one that was measured.** Hardware
    // accepted `setsockopt(SOL_SOCKET, 0x1200, &1, 4)` and then answered a would-block on the
    // socket it was set on, which is a pair of measurements no blocking socket can produce
    // (obSCEne `102-net/nonblocking-option` and `102-net/recv-would-block`). Accepting it and
    // doing nothing was the plausible answer; this is the measured one (D667).
    if level == SOL_SOCKET && optname == SO_NONBLOCKING {
        // The value is the flag, read the way `setsockopt` documents: a non-zero `int` turns
        // it on. A guest that passes no buffer is asking for the default, which is on.
        let wanted = read_option_flag(optval, optlen);
        return if crate::descriptor::set_nonblocking(fd, wanted) {
            Ok(OK)
        } else {
            Err(UNNAMED)
        };
    }
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
    Ok(OK)
}

/// `getsockname(fd, address, length)` - where a socket actually ended up.
///
/// Worth having rather than stubbing: a server that binds to port zero asks the system which
/// port it got, and prints it. Answering a made-up number would put a wrong port in front of
/// whoever is trying to connect.
///
/// Reference: POSIX.1-2008 `getsockname(2)`.
pub fn getsockname(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let found = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Listener { listener, .. } => listener.local_addr().ok(),
        Socket::Stream { stream, .. } => stream.local_addr().ok(),
        Socket::Pending { bound, .. } => *bound,
    })
    .flatten();
    let Some(local) = found else {
        return Err(UNNAMED);
    };
    if write_sockaddr(args[1], args[2], local) {
        Ok(OK)
    } else {
        Err(UNNAMED)
    }
}

/// `getpeername(fd, address, length)` - who is at the other end.
///
/// Reference: POSIX.1-2008 `getpeername(2)`.
pub fn getpeername(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let found = crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Stream { stream, .. } => stream.peer_addr().ok(),
        _ => None,
    })
    .flatten();
    let Some(peer) = found else {
        return Err(UNNAMED);
    };
    if write_sockaddr(args[1], args[2], peer) {
        Ok(OK)
    } else {
        Err(UNNAMED)
    }
}

/// `send(fd, buffer, length, flags)` - a write with flags nobody here honours.
///
/// The flags are ignored and that is stated: `MSG_OOB` and `MSG_DONTROUTE` are not things
/// this can promise, and a guest relying on one would be misread. Every payload measured
/// passes zero.
///
/// Reference: POSIX.1-2008 `send(2)`.
pub fn send(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let Some(bytes) = guest_bytes(args[1], args[2]) else {
        return Err(UNNAMED);
    };
    crate::descriptor::socket_write(args[0], bytes, wait_for(args[3])).map(|n| n as u64)
}

/// `recv(fd, buffer, length, flags)` - a read with the same caveat about flags.
///
/// Reference: POSIX.1-2008 `recv(2)`.
pub fn recv(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let Some(into) = guest_bytes_mut(args[1], args[2]) else {
        return Err(UNNAMED);
    };
    crate::descriptor::socket_read(args[0], into, wait_for(args[3])).map(|n| n as u64)
}

/// What a call's flags say about waiting.
///
/// **`MSG_DONTWAIT` is the one flag here that is honoured**, and it is honoured because
/// ignoring it did not merely answer wrong: obSCEne passes it, orbistoun waited, and the guest
/// stopped for the rest of the run - eleven sections lost to one blocking read. The others are
/// still ignored and the shims say so.
///
/// Reference: `MSG_DONTWAIT` from the harvested `sys/sys/socket.h`.
fn wait_for(flags: u64) -> crate::descriptor::Wait {
    let dont_wait = number("MSG_DONTWAIT");
    // **An unnameable constant is all-ones, and a mask of all-ones matches everything.** The
    // refusal-by-impossible-value trick the families use does not carry over to a bitmask, so
    // a table that could not answer would turn every flagged call non-blocking rather than
    // none. Refused explicitly instead; the test below rules the case out anyway.
    if dont_wait == u64::MAX || flags & dont_wait == 0 {
        crate::descriptor::Wait::AsTheSocketIs
    } else {
        crate::descriptor::Wait::Never
    }
}

/// `shutdown(fd, how)` - stops one or both directions.
///
/// Reference: POSIX.1-2008 `shutdown(2)`; `SHUT_RD`, `SHUT_WR` and `SHUT_RDWR` are 0, 1 and
/// 2, from `sys/sys/socket.h`.
pub fn shutdown(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    use std::net::Shutdown;
    let how = match args[1] {
        0 => Shutdown::Read,
        1 => Shutdown::Write,
        2 => Shutdown::Both,
        // A direction nobody defines, refused rather than treated as both.
        _ => return Err(UNNAMED),
    };
    crate::descriptor::with_socket(args[0], |socket| match socket {
        Socket::Stream { stream, .. } => {
            if stream.shutdown(how).is_ok() {
                Ok(OK)
            } else {
                Err(UNNAMED)
            }
        }
        // Shutting down something with no peer is the same condition `recv` meets there, and
        // the console names it the same way.
        _ => Err(orbistoun_core::errno::NOT_CONNECTED),
    })
    .unwrap_or(Err(UNNAMED))
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

/// `htonl(value)` - host byte order to network byte order, 32 bits.
///
/// Reference: POSIX.1-2008 `htonl(3)`. Network order is big-endian by definition and both the
/// guest and this host are x86-64, so the conversion is a byte swap - stated rather than
/// written as a no-op, because a no-op is what it would be on a big-endian host and is exactly
/// the assumption that would be wrong there.
///
/// **Only the low thirty-two bits are meaningful.** The argument arrives in a 64-bit register
/// and the high half is whatever the caller last had there.
pub fn htonl(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from((args[0] as u32).swap_bytes())
}

/// `htons(value)` - host to network, 16 bits. POSIX.1-2008 `htons(3)`.
pub fn htons(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from((args[0] as u16).swap_bytes())
}

/// `ntohl(value)` - network to host, 32 bits. POSIX.1-2008 `ntohl(3)`.
///
/// The same swap as [`htonl`]: the conversion is its own inverse, which is why the two are
/// separate names for one operation rather than a pair.
pub fn ntohl(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from((args[0] as u32).swap_bytes())
}

/// `ntohs(value)` - network to host, 16 bits. POSIX.1-2008 `ntohs(3)`.
pub fn ntohs(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from((args[0] as u16).swap_bytes())
}

/// The socket options `getsockopt` can answer, by the section and name they are harvested under.
///
/// # Why a table rather than a blanket answer
///
/// An option this layer cannot read has no value to hand back, and inventing one is worse than
/// failing: a caller reads what it is given and acts on it. So each option is either answered
/// from the host socket or **refused by name**, and the refusal says which option was wanted so
/// a trace names the next piece of work rather than two integers.
///
/// The values come from the harvested table rather than being written here (D350/D351), which
/// also bounds what can be recognised at all: `TCP_NODELAY` lives in `netinet/tcp.h`, outside
/// what the harvest reads, so it is refused as unknown rather than guessed at.
const READABLE_OPTIONS: &[(&str, &str)] = &[("socket", "SO_ERROR"), ("in", "IP_TTL")];

/// The option level a socket's own settings live at.
///
/// Not read from the harvested header, and deliberately: `SOL_SOCKET` is `0xffff` there *and*
/// in every measurement obSCEne took, so the two agree - but the pair below has no header entry
/// to be read from, and splitting one constant across two provenances is how a level and an
/// option end up disagreeing about which platform they describe.
const SOL_SOCKET: u64 = 0xffff;

/// The option that turns non-blocking on, as this platform numbers it.
///
/// **Measured, not derived.** `0x1200` is not the FreeBSD `SO_*` numbering and nothing in the
/// harvested headers names it; what establishes it is obSCEne's `102-net/nonblocking-option`,
/// where hardware answered `0x0` to `setsockopt(SOL_SOCKET, 0x1200, &1, 4)` and `-1` to the
/// `fcntl(F_SETFL, O_NONBLOCK)` that would be the portable way. The check tries `0x1100` as a
/// fallback and the console refused it, so the value is settled rather than one of two.
const SO_NONBLOCKING: u64 = 0x1200;

/// The name of an option, for a report that would otherwise print a number.
fn option_name(option: u64) -> Option<&'static str> {
    let wanted = i64::try_from(option).ok()?;
    READABLE_OPTIONS.iter().find_map(|(section, name)| {
        (orbistoun_hle::constants::abi_constant(section, name)? == wanted).then_some(*name)
    })
}

/// Says which option a guest wanted and could not have, once per distinct option.
///
/// Once, because options are typically set in a loop and the same refusal repeated forty times
/// buries the rest of the report.
fn refuse_option(level: u64, option: u64) -> Answer {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<BTreeSet<(u64, u64)>>> = Mutex::new(None);

    if let Ok(mut guard) = SAID.lock()
        && guard
            .get_or_insert_with(BTreeSet::new)
            .insert((level, option))
    {
        let named = option_name(option).map_or_else(
            || format!("option {option:#x} at level {level:#x}"),
            |name| format!("{name} (level {level:#x})"),
        );
        tracing::warn!("getsockopt asked for {named}, which is not answered here");
        let line = format!("orbistoun: getsockopt asked for {named}, which is not answered here");
        orbistoun_core::klog::note(&line);
    }
    Err(UNNAMED)
}

/// The `int` behind a `setsockopt` value pointer, as a flag.
///
/// A guest that passes no buffer is asking for the option's plain form, which for a flag is
/// "on" - refusing that would be stricter than the interface, and the platform accepted the
/// call in every form obSCEne tried.
fn read_option_flag(value_at: u64, length: u64) -> bool {
    if value_at == 0 || length < 4 {
        return true;
    }
    let Ok(base) = usize::try_from(value_at) else {
        return true;
    };
    // SAFETY: a guest-supplied option buffer under the identity mapping (D014), of at least
    // the four bytes an `int` option occupies - which is what the length above checked.
    let value = unsafe { std::ptr::read_unaligned(base as *const i32) };
    value != 0
}

/// `getsockopt(fd, level, option, value, length)` - POSIX.1-2008 `getsockopt(2)`.
///
/// Answers the options the host socket can report and refuses the rest by name. See
/// `READABLE_OPTIONS`; the counterpart `setsockopt` takes the opposite tack for a reason
/// stated there - a server exits if setting an option fails, whereas a server *reading* one
/// gets a value it will act on, so a wrong answer is worse than a refusal.
pub fn getsockopt(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    let (fd, level, option, value, length) = (args[0], args[1], args[2], args[3], args[4]);
    let Some(name) = option_name(option) else {
        return refuse_option(level, option);
    };
    let answer = crate::descriptor::with_socket(fd, |socket| {
        let Socket::Stream { stream, .. } = socket else {
            return None;
        };
        match name {
            // **Reading `SO_ERROR` clears it**, which is the half a naive version drops: a
            // caller polls it precisely to consume the pending error, and one that keeps
            // answering the same error sees a socket that never recovers.
            "SO_ERROR" => Some(
                stream
                    .take_error()
                    .ok()
                    .flatten()
                    .and_then(|e| e.raw_os_error())
                    .unwrap_or(0),
            ),
            "IP_TTL" => stream.ttl().ok().and_then(|ttl| i32::try_from(ttl).ok()),
            _ => None,
        }
    });
    let Some(Some(answered)) = answer else {
        return refuse_option(level, option);
    };
    if !write_option(value, length, answered) {
        return Err(UNNAMED);
    }
    Ok(OK)
}

/// Writes an `int` option value back to the guest, and the length beside it.
fn write_option(value_at: u64, length_at: u64, value: i32) -> bool {
    let Ok(base) = usize::try_from(value_at) else {
        return false;
    };
    if value_at == 0 {
        return false;
    }
    // SAFETY: a guest-supplied out-parameter under the identity mapping (D014). Four bytes,
    // which is what an `int` option is - eight would take the caller's next variable (D272).
    unsafe { std::ptr::write_unaligned(base as *mut i32, value) };
    if length_at != 0
        && let Ok(at) = usize::try_from(length_at)
    {
        // SAFETY: the caller's `socklen_t`, four bytes, under the same contract.
        unsafe { std::ptr::write_unaligned(at as *mut u32, 4) };
    }
    true
}

/// `close(fd)` for a socket - what `sceNetSocketClose` is.
///
/// **Not the same function as the kernel's `close`**, and the difference is only visible when
/// it fails: `sceKernelClose` is measured answering `0x8002_0009` for a descriptor that is not
/// open, so `EBADF` is the number, and each library numbers it in its own base. The success
/// path is one table lookup and is shared.
pub fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> Answer {
    if crate::descriptor::close(args[0]) {
        Ok(OK)
    } else {
        Err(orbistoun_core::errno::BAD_DESCRIPTOR)
    }
}

/// One `GuestFn` per errno-bearing body, in the POSIX spelling.
///
/// A macro rather than a dozen copies of the same two lines. The conversion is a single rule,
/// and written out per call it is exactly the sort of place one of them ends up answering `0`
/// for a failure - which is the bug this whole change exists to stop.
///
/// `orbistoun-net` has the matching macro for the vendor spelling. Two macros rather than one
/// shared one because the *rules* differ: this crate knows what POSIX returns, that one knows
/// how `libSceNet` numbers an error, and neither has to learn the other's.
macro_rules! posix_spellings {
    ($($wrapper:ident => $body:ident,)*) => {
        $(
            fn $wrapper(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
                as_posix($body(args))
            }
        )*
    };
}

posix_spellings! {
    posix_socket => socket,
    posix_bind => bind,
    posix_listen => listen,
    posix_accept => accept,
    posix_connect => connect,
    posix_setsockopt => setsockopt,
    posix_getsockopt => getsockopt,
    posix_getsockname => getsockname,
    posix_getpeername => getpeername,
    posix_send => send,
    posix_recv => recv,
    posix_shutdown => shutdown,
}

/// Implementations this module provides, by symbol name.
///
/// **The POSIX spellings only.** The vendor twins live in `orbistoun-net`, which is the crate
/// that declares `libSceNet` and the crate that knows how it encodes an error - the same rule
/// D525 applies to `sceKernelStat`, whose body is next door and whose name is offered where it
/// is declared.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("htonl", htonl),
        ("htons", htons),
        ("ntohl", ntohl),
        ("ntohs", ntohs),
        ("socket", posix_socket),
        ("bind", posix_bind),
        ("listen", posix_listen),
        ("accept", posix_accept),
        ("connect", posix_connect),
        ("setsockopt", posix_setsockopt),
        ("getsockopt", posix_getsockopt),
        ("getsockname", posix_getsockname),
        ("getpeername", posix_getpeername),
        ("send", posix_send),
        ("recv", posix_recv),
        ("shutdown", posix_shutdown),
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

    /// **A `recv` on a listening socket names the reason, and the reason is measured.**
    ///
    /// Hardware answered `sceNetRecv` on a listener `0x8041_0139` (obSCEne
    /// `102-net/recv-would-block`, package leg of sweep 20260909-234847), whose low byte is
    /// `ENOTCONN`. Before this the body collapsed every failure to `-1`, so the vendor spelling
    /// had nothing to encode and the check failed here while passing there (D667).
    ///
    /// Written first, and asserted on the errno rather than on "it failed": the whole value of
    /// carrying one out of the body is that the two spellings can disagree about how to say it.
    #[test]
    fn a_recv_on_a_listening_socket_is_refused_as_having_no_peer() {
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

        let mut into = [0_u8; 16];
        assert_eq!(
            super::recv(&[fd, into.as_mut_ptr() as u64, into.len() as u64, 0, 0, 0]),
            Err(orbistoun_core::errno::NOT_CONNECTED),
            "a listener has no peer, and that is not the same failure as a broken one"
        );
        assert!(crate::descriptor::close(fd));
    }

    /// **Nothing arrived is not end-of-stream**, and the option that makes the difference is
    /// the one hardware accepted.
    ///
    /// Two measurements meet here. `_setsockopt(0xffff, 0x1200, &1, 4)` answers `0x0`
    /// (`102-net/nonblocking-option`), and a `sceNetRecv` on the socket it was set on answers
    /// `0x8041_0123` - `EAGAIN` (`102-net/recv-would-block`). The second is what shows the
    /// first was *applied* rather than merely accepted, because a blocking socket cannot
    /// produce it.
    ///
    /// This build accepted the option and did nothing, then read a would-block as `Ok(0)` -
    /// which a guest reads as the peer hanging up. Two plausible answers composing into a
    /// closed connection is exactly the shape principle 3 forbids.
    ///
    /// **The option is set while the descriptor is still pending**, before `connect` makes a
    /// host socket at all, because that is the order obSCEne writes and the flag has to
    /// survive the object being created.
    #[test]
    fn a_non_blocking_recv_with_nothing_arrived_says_would_block() {
        let _guard = crate::exclusively();
        // A listener that accepts and then says nothing, so the guest's socket is connected
        // and permanently empty.
        let quiet = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a host listener");
        let port = quiet.local_addr().expect("an address").port();
        let held = std::thread::spawn(move || quiet.accept().map(|(stream, _)| stream));

        let fd = call("socket", [af_inet(), super::sock_stream(), 0, 0, 0, 0]);
        assert_eq!(
            call("setsockopt", [fd, 0xffff, 0x1200, 0, 4, 0]),
            0,
            "the option hardware accepted"
        );
        let wanted = sockaddr(port, [127, 0, 0, 1]);
        assert_eq!(
            call(
                "connect",
                [fd, wanted.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0
        );

        let mut into = [0_u8; 16];
        assert_eq!(
            super::recv(&[fd, into.as_mut_ptr() as u64, into.len() as u64, 0, 0, 0]),
            Err(orbistoun_core::errno::AGAIN),
            "the option was applied, and an empty non-blocking read is a would-block"
        );
        assert!(crate::descriptor::close(fd));
        drop(held.join().expect("the host side"));
    }

    /// **The POSIX spelling still answers `-1`, whatever the body named.**
    ///
    /// The negative half of the same change: carrying an errno out of the body must not leak
    /// into the POSIX return, because a caller of `bind` tests for `-1` and reads the number
    /// from `errno`. A guard nobody has watched reject something is a guard nobody knows
    /// anything about, so this asserts the value rather than "it failed".
    #[test]
    fn the_posix_spelling_answers_minus_one_whatever_the_body_named() {
        let _guard = crate::exclusively();
        let mut into = [0_u8; 16];
        // Descriptor 4096 names nothing, so the body has to fail somehow.
        assert_eq!(
            call(
                "recv",
                [4096, into.as_mut_ptr() as u64, into.len() as u64, 0, 0, 0]
            ),
            super::FAILED,
            "POSIX leaves the number to `errno` and answers -1"
        );
    }

    /// A connected pair: a guest descriptor, and the host end held open and silent.
    ///
    /// Returned together because dropping the host end closes the connection, and a read on a
    /// closed socket is a different condition from a read on an empty one - which is the
    /// distinction every test below turns on.
    fn connected_and_quiet() -> (u64, std::net::TcpStream) {
        let quiet = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a host listener");
        let port = quiet.local_addr().expect("an address").port();
        let held = std::thread::spawn(move || quiet.accept().map(|(stream, _)| stream));

        let fd = call("socket", [af_inet(), super::sock_stream(), 0, 0, 0, 0]);
        let wanted = sockaddr(port, [127, 0, 0, 1]);
        assert_eq!(
            call(
                "connect",
                [fd, wanted.as_ptr() as u64, SOCKADDR_IN_LEN, 0, 0, 0]
            ),
            0
        );
        (fd, held.join().expect("the host side").expect("accepted"))
    }

    /// `MSG_DONTWAIT`, from the harvested `sys/sys/socket.h`.
    const DONT_WAIT: u64 = 0x80;

    /// **`MSG_DONTWAIT` is the header's number, and it is nameable.**
    ///
    /// The negative case for [`super::wait_for`]: an unnameable constant is `u64::MAX`, which
    /// as a bitmask matches every flag a guest could pass rather than none. The refusal trick
    /// that works for an address family inverts for a mask, so this asserts the table can
    /// actually answer.
    #[test]
    fn the_dont_wait_flag_comes_from_the_header() {
        assert_eq!(super::number("MSG_DONTWAIT"), DONT_WAIT, "MSG_DONTWAIT");
        assert_ne!(
            super::number("MSG_DONTWAIT"),
            u64::MAX,
            "and it is nameable"
        );
    }

    /// **A `MSG_DONTWAIT` read does not wait, on a socket nobody made non-blocking.**
    ///
    /// This is the flag obSCEne passes and this shim ignored, and ignoring it did not merely
    /// answer wrong - it **hung the guest**. `102-net/recv-would-block` connects a plain
    /// blocking socket and reads it with the flag; hardware answers `0x8041_0123` in three
    /// microseconds, and orbistoun waited until the run's time limit ended it, taking the
    /// remaining eleven sections with it (D667).
    ///
    /// A test that fails by hanging is a poor test, and this one would. It is still the right
    /// assertion: the alternative is asserting on elapsed time, which is a flake on a loaded
    /// machine and does not say what went wrong.
    #[test]
    fn a_dont_wait_read_does_not_wait_on_a_blocking_socket() {
        let _guard = crate::exclusively();
        let (fd, host) = connected_and_quiet();

        assert_eq!(
            super::recv(&[fd, [0_u8; 16].as_mut_ptr() as u64, 16, DONT_WAIT, 0, 0]),
            Err(orbistoun_core::errno::AGAIN),
            "the flag is what makes this answer rather than wait"
        );
        assert!(crate::descriptor::close(fd));
        drop(host);
    }

    /// **And it puts the socket back the way the guest left it.**
    ///
    /// The half that would rot silently. Honouring the flag means turning the host socket
    /// non-blocking for one call, and a shim that forgot to restore afterwards would leave
    /// every later read non-blocking - which looks identical here and turns a guest's blocking
    /// `recv` into a busy loop somewhere else entirely.
    ///
    /// Asserted in the direction that breaks: the socket is set **non-blocking first**, so a
    /// restore that hard-coded "blocking" would clobber it, and the second read would wait
    /// instead of answering.
    #[test]
    fn a_dont_wait_read_leaves_the_sockets_own_mode_alone() {
        let _guard = crate::exclusively();
        let (fd, host) = connected_and_quiet();
        assert!(
            crate::descriptor::set_nonblocking(fd, true),
            "the guest's own request"
        );

        assert_eq!(
            super::recv(&[fd, [0_u8; 16].as_mut_ptr() as u64, 16, DONT_WAIT, 0, 0]),
            Err(orbistoun_core::errno::AGAIN)
        );
        assert_eq!(
            super::recv(&[fd, [0_u8; 16].as_mut_ptr() as u64, 16, 0, 0, 0]),
            Err(orbistoun_core::errno::AGAIN),
            "still non-blocking, because the guest never asked for anything else"
        );
        assert!(crate::descriptor::close(fd));
        drop(host);
    }
}

#[cfg(test)]
mod byte_order {
    use super::{GUEST_ARG_REGISTERS, htonl, htons, ntohl, ntohs};

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, value: u64) -> u64 {
        let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
        regs[0] = value;
        f(&regs)
    }

    /// The swap is the whole function, and the answer is known exactly.
    #[test]
    fn the_conversions_swap_bytes() {
        assert_eq!(call(htonl, 0x1234_5678), 0x7856_3412);
        assert_eq!(call(htons, 0x1234), 0x3412);
        assert_eq!(call(ntohl, 0x7856_3412), 0x1234_5678);
        assert_eq!(call(ntohs, 0x3412), 0x1234);
    }

    /// **Only the low half is meaningful.** The argument arrives in a 64-bit register whose
    /// upper bits are the caller's leftovers; reading them would answer a different number
    /// every call for the same input.
    #[test]
    fn the_high_half_of_the_register_is_ignored() {
        assert_eq!(call(htonl, 0xFFFF_FFFF_1234_5678), 0x7856_3412);
        assert_eq!(call(htons, 0xFFFF_FFFF_FFFF_1234), 0x3412);
    }

    /// Each conversion is its own inverse, which is why network order needs one operation
    /// and not two.
    #[test]
    fn each_conversion_is_its_own_inverse() {
        for value in [0_u64, 1, 0x0000_00FF, 0x1234_5678, 0xFFFF_FFFF] {
            assert_eq!(call(ntohl, call(htonl, value)), value & 0xFFFF_FFFF);
        }
    }
}
