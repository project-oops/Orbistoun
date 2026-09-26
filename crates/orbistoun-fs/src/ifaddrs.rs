//! Which addresses a guest can be reached on, and how it prints one.
//!
//! Servers call `getifaddrs` before serving and print what comes back, via `__inet_ntop`.
//!
//! The structure, from `include/ifaddrs.h`:
//!
//! ```text
//! include/ifaddrs.h
//!     struct ifaddrs {
//!         struct ifaddrs  *ifa_next;      offset  0
//!         char            *ifa_name;      offset  8
//!         unsigned int     ifa_flags;     offset 16
//!         struct sockaddr *ifa_addr;      offset 24
//!         struct sockaddr *ifa_netmask;   offset 32
//!         struct sockaddr *ifa_dstaddr;   offset 40
//!         void            *ifa_data;      offset 48
//!     };
//! ```
//!
//! `ifa_flags` is four bytes followed by four of padding, since the next pointer is
//! eight-aligned. The flags are harvested from `sys/net/if.h`; a server filters on
//! `IFF_LOOPBACK`, looking for an address that is not the loopback. The loopback is
//! `127.0.0.1`. The other entry is the host's outward address, found by opening a UDP socket
//! towards a documentation address and reading which local address it would use; a UDP
//! `connect` sends nothing. The interface name `net0` is the one invention, since nothing
//! here knows what the platform calls its interfaces; `lo0` is every BSD's loopback name.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Bytes of a `struct ifaddrs`, from `include/ifaddrs.h`.
const IFADDRS_LEN: usize = 56;

/// `IFF_UP`, from `sys/net/if.h`.
pub const IFF_UP: u32 = 0x1;

/// `IFF_LOOPBACK`, from `sys/net/if.h`.
///
/// The flag a server filters on, so it is named, and checked against the harvested table by
/// a test in `orbistoun-libc` (D370).
pub const IFF_LOOPBACK: u32 = 0x8;

/// `IFF_RUNNING`, from `sys/net/if.h` by way of `IFF_DRV_RUNNING`.
pub const IFF_RUNNING: u32 = 0x40;

/// `AF_INET`, as [`crate::socket`] reads it out of the harvested table.
fn af_inet() -> u64 {
    crate::socket::af_inet()
}

/// `AF_INET6`, the same way.
fn af_inet6() -> u64 {
    crate::socket::af_inet6()
}

/// Bytes of a `sockaddr_in`.
const SOCKADDR_IN_LEN: usize = 16;

/// One interface this reports.
struct Interface {
    /// What the guest sees it called.
    name: &'static str,
    /// Its address.
    address: Ipv4Addr,
    /// Its netmask.
    netmask: Ipv4Addr,
    /// Its flags.
    flags: u32,
}

/// The host's address for reaching anything outside itself.
///
/// A UDP socket pointed at RFC 5737 `TEST-NET-1`, reserved and routed nowhere, picks a route
/// and a source address without sending anything; the source address is the answer. [`None`]
/// on a host with no route out, which is a real state.
fn outward_address() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()? {
        std::net::SocketAddr::V4(v4) => Some(*v4.ip()),
        std::net::SocketAddr::V6(_) => None,
    }
}

/// What this reports, in the order a guest walks it.
///
/// The loopback last, so a server taking the first non-loopback entry finds the useful one
/// at once.
fn interfaces() -> Vec<Interface> {
    let mut out = Vec::new();
    if let Some(address) = outward_address() {
        out.push(Interface {
            name: "net0",
            address,
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            flags: IFF_UP | IFF_RUNNING,
        });
    }
    out.push(Interface {
        name: "lo0",
        address: Ipv4Addr::LOCALHOST,
        netmask: Ipv4Addr::new(255, 0, 0, 0),
        flags: IFF_UP | IFF_RUNNING | IFF_LOOPBACK,
    });
    out
}

/// Lists this process has handed out, so `freeifaddrs` can give them back.
///
/// Kept rather than leaked: a server calls `getifaddrs` every time it reports its address,
/// and runs for days.
fn handed_out() -> &'static Mutex<BTreeMap<u64, Vec<u8>>> {
    static LISTS: OnceLock<Mutex<BTreeMap<u64, Vec<u8>>>> = OnceLock::new();
    LISTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Writes a `sockaddr_in` into `block` at `at`.
fn put_sockaddr(block: &mut [u8], at: usize, address: Ipv4Addr) {
    block[at] = SOCKADDR_IN_LEN as u8;
    block[at + 1] = af_inet() as u8;
    // Port zero: an interface address has no port; the field exists because the structure is
    // shared.
    block[at + 2] = 0;
    block[at + 3] = 0;
    block[at + 4..at + 8].copy_from_slice(&address.octets());
}

/// Writes a pointer into `block` at `at`.
fn put_pointer(block: &mut [u8], at: usize, value: u64) {
    block[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// `getifaddrs(&list)`: builds the linked list a guest walks.
///
/// One allocation holds every structure, name and address, so the list is freed by dropping
/// one thing and a guest walking `ifa_next` never leaves it.
///
/// Reference: FreeBSD `getifaddrs(3)`; `struct ifaddrs` from `include/ifaddrs.h`.
fn getifaddrs(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out == 0 {
        return -1_i64 as u64;
    }
    let entries = interfaces();

    // Laid out as every `ifaddrs`, then every name, then every address and netmask, in one
    // block, so the addresses inside it are computed before it is written.
    let first_name = entries.len() * IFADDRS_LEN;
    let names_len: usize = entries.iter().map(|i| i.name.len() + 1).sum();
    let addresses_at = first_name + names_len;
    let mut block = vec![0_u8; addresses_at + entries.len() * SOCKADDR_IN_LEN * 2];
    let base = block.as_ptr() as u64;

    let mut writing_name = first_name;
    for (index, entry) in entries.iter().enumerate() {
        let record = index * IFADDRS_LEN;
        let next = if index + 1 == entries.len() {
            0
        } else {
            base + ((index + 1) * IFADDRS_LEN) as u64
        };
        let address_at = addresses_at + index * SOCKADDR_IN_LEN * 2;
        let netmask_at = address_at + SOCKADDR_IN_LEN;

        put_pointer(&mut block, record, next);
        put_pointer(&mut block, record + 8, base + writing_name as u64);
        block[record + 16..record + 20].copy_from_slice(&entry.flags.to_le_bytes());
        put_pointer(&mut block, record + 24, base + address_at as u64);
        put_pointer(&mut block, record + 32, base + netmask_at as u64);
        // No destination address and no driver data: a broadcast interface has neither.
        put_pointer(&mut block, record + 40, 0);
        put_pointer(&mut block, record + 48, 0);

        let end = writing_name + entry.name.len();
        block[writing_name..end].copy_from_slice(entry.name.as_bytes());
        writing_name = end + 1;

        put_sockaddr(&mut block, address_at, entry.address);
        put_sockaddr(&mut block, netmask_at, entry.netmask);
    }

    let Ok(at) = usize::try_from(out) else {
        return -1_i64 as u64;
    };
    // SAFETY: a guest-supplied `struct ifaddrs **` under the identity mapping, the same
    // contract the real call has.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), base);
    }
    if let Ok(mut lists) = handed_out().lock() {
        lists.insert(base, block);
    }
    0
}

/// `freeifaddrs(list)`: gives back what [`getifaddrs`] handed out.
///
/// A list this did not hand out is ignored, since freeing it would treat an arbitrary guest
/// pointer as an allocation.
///
/// Reference: FreeBSD `getifaddrs(3)`.
fn freeifaddrs(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Ok(mut lists) = handed_out().lock() {
        lists.remove(&args[0]);
    }
    0
}

/// `__inet_ntop(family, source, destination, size)`: an address as text.
///
/// The leading underscores are FreeBSD's: `inet_ntop` resolves to `__inet_ntop` there, and
/// both names are served. `AF_INET` and `AF_INET6`; any other family is refused rather than
/// printed plausibly. The sixteen-byte form is rendered by [`std::net::Ipv6Addr`], whose
/// `Display` is RFC 5952 canonical, as `inet_ntop` produces.
///
/// Reference: POSIX.1-2008 `inet_ntop(3)`. `INET_ADDRSTRLEN` is 16 and `INET6_ADDRSTRLEN` is
/// 46, from `sys/netinet/in.h`.
fn inet_ntop(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (family, source, destination, size) = (args[0], args[1], args[2], args[3]);
    if source == 0 || destination == 0 {
        return 0;
    }
    let (Ok(from), Ok(to)) = (usize::try_from(source), usize::try_from(destination)) else {
        return 0;
    };
    let width = if family == af_inet() {
        4
    } else if family == af_inet6() {
        16
    } else {
        return 0;
    };
    // SAFETY: a guest-supplied `struct in_addr` or `struct in6_addr` under the identity
    // mapping: four or sixteen bytes for the family the guest named, the same contract the
    // real call has.
    let octets =
        unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(from), width) };
    let text = if width == 4 {
        format!("{}.{}.{}.{}\0", octets[0], octets[1], octets[2], octets[3])
    } else {
        let mut sixteen = [0_u8; 16];
        sixteen.copy_from_slice(octets);
        format!("{}\0", std::net::Ipv6Addr::from(sixteen))
    };
    if (text.len() as u64) > size {
        // Refused rather than truncated: half an address is a different address.
        return 0;
    }
    // SAFETY: the destination has at least `text.len()` bytes, just checked against the size
    // the guest passed.
    unsafe {
        std::ptr::copy_nonoverlapping(
            text.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(to),
            text.len(),
        );
    }
    destination
}

/// `__inet_pton(family, source, destination)`: text as an address.
///
/// Answers 1 when parsed, 0 when the text is not a valid address in this family, and -1 with
/// `EAFNOSUPPORT` for a family this does not know; a caller tells them apart. `AF_INET` takes
/// a dotted quad only, since the classful short forms are `inet_aton`'s. `AF_INET6` is parsed
/// by [`std::net::Ipv6Addr`], which follows RFC 4291 and rejects a zone identifier, as
/// `inet_pton` does. Only the underscored spelling is served, since only that one is
/// imported (D367).
fn inet_pton(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// What the call answers for a family it does not serve.
    const UNSUPPORTED_FAMILY: u64 = -1_i64 as u64;
    /// What it answers for text that is not an address in the family asked for.
    const NOT_AN_ADDRESS: u64 = 0;

    let (family, source, destination) = (args[0], args[1], args[2]);
    let sixteen_byte = if family == af_inet() {
        false
    } else if family == af_inet6() {
        true
    } else {
        // A family this cannot parse is a different failure from text it cannot parse. `errno`
        // is left alone: the return value is the answer.
        return UNSUPPORTED_FAMILY;
    };
    if source == 0 || destination == 0 {
        return NOT_AN_ADDRESS;
    }
    // SAFETY: the guest's text argument, a NUL-terminated string by the call's contract.
    let Some(text) = (unsafe { orbistoun_mem::guest::read_path(source) }) else {
        return NOT_AN_ADDRESS;
    };
    let parsed: Vec<u8> = if sixteen_byte {
        let Ok(address) = text.parse::<std::net::Ipv6Addr>() else {
            return NOT_AN_ADDRESS;
        };
        address.octets().to_vec()
    } else {
        let Some(octets) = dotted_quad(&text) else {
            return NOT_AN_ADDRESS;
        };
        octets.to_vec()
    };
    let Ok(to) = usize::try_from(destination) else {
        return NOT_AN_ADDRESS;
    };
    // SAFETY: a guest-supplied `struct in_addr` or `struct in6_addr` under the identity
    // mapping: four or sixteen bytes for the family the guest named and passed to be written.
    unsafe {
        std::ptr::copy_nonoverlapping(
            parsed.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(to),
            parsed.len(),
        );
    }
    1
}

/// Four decimal octets separated by three dots, and nothing else.
///
/// Stricter than `inet_aton`: no leading zeros (octal to some readers, decimal to others), no
/// whitespace, no trailing characters, no short forms.
fn dotted_quad(text: &str) -> Option<[u8; 4]> {
    let mut octets = [0_u8; 4];
    let mut parts = text.split('.');
    for slot in &mut octets {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            // A leading zero is octal to some readers and decimal to others.
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(octets)
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("getifaddrs", getifaddrs),
        ("freeifaddrs", freeifaddrs),
        ("__inet_ntop", inet_ntop),
        ("inet_ntop", inet_ntop),
        ("__inet_pton", inet_pton),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .expect("declared");
        function(&args)
    }

    /// Reads a pointer out of guest memory, as the guest would.
    fn pointer_at(address: u64, offset: usize) -> u64 {
        let base = std::ptr::with_exposed_provenance::<u8>(address as usize);
        // SAFETY: `address` came from `getifaddrs`, which keeps the block alive, and every
        // offset asked for here is inside one of its records.
        let slot = unsafe { base.add(offset) };
        // SAFETY: `slot` is in bounds by the line above.
        unsafe { std::ptr::read_unaligned(slot.cast::<u64>()) }
    }

    /// Reads a 32-bit field out of guest memory, as the guest would.
    fn word_at(address: u64, offset: usize) -> u32 {
        let base = std::ptr::with_exposed_provenance::<u8>(address as usize);
        // SAFETY: as in `pointer_at`.
        let slot = unsafe { base.add(offset) };
        // SAFETY: `slot` is in bounds by the line above.
        unsafe { std::ptr::read_unaligned(slot.cast::<u32>()) }
    }

    /// Reads one byte out of guest memory, as the guest would.
    fn byte_at(address: u64, offset: usize) -> u8 {
        let base = std::ptr::with_exposed_provenance::<u8>(address as usize);
        // SAFETY: as in `pointer_at`.
        let slot = unsafe { base.add(offset) };
        // SAFETY: `slot` is in bounds by the line above.
        unsafe { *slot }
    }

    /// The list is laid out at the header's offsets and ends in a null.
    #[test]
    fn the_list_is_laid_out_where_the_header_says_and_ends_in_a_null() {
        let mut head = 0_u64;
        assert_eq!(
            call(
                "getifaddrs",
                [std::ptr::addr_of_mut!(head) as u64, 0, 0, 0, 0, 0]
            ),
            0
        );
        assert_ne!(head, 0);

        let mut at = head;
        let mut seen = 0;
        let mut loopbacks = 0;
        while at != 0 {
            let flags = word_at(at, 16);
            assert_ne!(flags & super::IFF_UP, 0, "every interface reported is up");
            if flags & super::IFF_LOOPBACK != 0 {
                loopbacks += 1;
            }

            let name = pointer_at(at, 8);
            assert_ne!(name, 0, "every entry is named");

            let address = pointer_at(at, 24);
            assert_ne!(address, 0, "and has an address");
            assert_eq!(
                byte_at(address, 1),
                super::af_inet() as u8,
                "the family is at offset one"
            );

            seen += 1;
            at = pointer_at(at, 0);
            assert!(seen < 8, "the list terminates");
        }
        assert!(seen >= 1);
        assert_eq!(
            loopbacks, 1,
            "exactly one loopback, which a server filters out"
        );

        assert_eq!(call("freeifaddrs", [head, 0, 0, 0, 0, 0]), 0);
    }

    /// A list this did not hand out is ignored rather than freed.
    #[test]
    fn freeing_something_this_never_handed_out_does_nothing() {
        let mut junk = [0_u64; 4];
        assert_eq!(
            call(
                "freeifaddrs",
                [std::ptr::addr_of_mut!(junk) as u64, 0, 0, 0, 0, 0]
            ),
            0
        );
    }

    /// An address becomes the text a person reads.
    #[test]
    fn an_address_becomes_the_text_a_person_reads() {
        let address = [192_u8, 168, 1, 55];
        let mut text = [0_u8; 16];
        let answered = call(
            "__inet_ntop",
            [
                super::af_inet(),
                address.as_ptr() as u64,
                text.as_mut_ptr() as u64,
                text.len() as u64,
                0,
                0,
            ],
        );
        assert_eq!(
            answered,
            text.as_ptr() as u64,
            "the destination, as the interface says"
        );
        let end = text.iter().position(|b| *b == 0).expect("terminated");
        assert_eq!(&text[..end], b"192.168.1.55");
    }

    /// A buffer too small is refused, because half an address is a different address.
    #[test]
    fn a_destination_too_small_is_refused_rather_than_truncated() {
        let address = [192_u8, 168, 1, 55];
        let mut text = [0xAA_u8; 8];
        assert_eq!(
            call(
                "__inet_ntop",
                [
                    super::af_inet(),
                    address.as_ptr() as u64,
                    text.as_mut_ptr() as u64,
                    text.len() as u64,
                    0,
                    0
                ]
            ),
            0
        );
        assert_eq!(text, [0xAA; 8], "and nothing is written");
    }

    /// A family this cannot render is refused rather than printed as something plausible.
    ///
    /// `AF_UNIX`, a family that is not rendered.
    #[test]
    fn an_address_family_this_cannot_render_is_refused() {
        let unix = orbistoun_hle::constants::abi_constant("socket", "AF_UNIX")
            .and_then(|value| u64::try_from(value).ok())
            .expect("the header names it");
        assert_ne!(unix, super::af_inet());
        assert_ne!(unix, super::af_inet6());

        let address = [0_u8; 16];
        let mut text = [0xAA_u8; 64];
        assert_eq!(
            call(
                "__inet_ntop",
                [
                    unix,
                    address.as_ptr() as u64,
                    text.as_mut_ptr() as u64,
                    64,
                    0,
                    0
                ]
            ),
            0
        );
        assert_eq!(text, [0xAA; 64], "and nothing is written");
    }

    /// A sixteen-byte address parses and renders back.
    ///
    /// Round-tripped rather than compared against a literal, since the text form has
    /// compression rules.
    #[test]
    fn a_sixteen_byte_address_parses_and_renders_back() {
        for spelling in ["::", "::1", "2001:db8::1", "fe80::1"] {
            let mut source = spelling.as_bytes().to_vec();
            source.push(0);
            let mut octets = [0_u8; 16];
            assert_eq!(
                call(
                    "__inet_pton",
                    [
                        super::af_inet6(),
                        source.as_ptr() as u64,
                        octets.as_mut_ptr() as u64,
                        0,
                        0,
                        0
                    ]
                ),
                1,
                "{spelling} is an address"
            );

            let mut text = [0_u8; 64];
            assert_ne!(
                call(
                    "__inet_ntop",
                    [
                        super::af_inet6(),
                        octets.as_ptr() as u64,
                        text.as_mut_ptr() as u64,
                        64,
                        0,
                        0
                    ]
                ),
                0,
                "{spelling} renders"
            );
            let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
            let back = std::str::from_utf8(&text[..end]).expect("text");
            assert_eq!(back, spelling, "and comes back the way it went in");
        }
    }

    /// Text that is not an address is a different failure from a family that is not served.
    ///
    /// Zero and `-1` mean different things to a caller.
    #[test]
    fn unparseable_text_is_not_the_same_answer_as_an_unserved_family() {
        let mut source = b"not an address ".to_vec();
        source.push(0);
        let mut octets = [0_u8; 16];
        assert_eq!(
            call(
                "__inet_pton",
                [
                    super::af_inet(),
                    source.as_ptr() as u64,
                    octets.as_mut_ptr() as u64,
                    0,
                    0,
                    0
                ]
            ),
            0,
            "not an address in this family"
        );
        let unix = orbistoun_hle::constants::abi_constant("socket", "AF_UNIX")
            .and_then(|value| u64::try_from(value).ok())
            .expect("the header names it");
        assert_eq!(
            call(
                "__inet_pton",
                [
                    unix,
                    source.as_ptr() as u64,
                    octets.as_mut_ptr() as u64,
                    0,
                    0,
                    0
                ]
            ),
            u64::MAX,
            "a family this does not serve"
        );
    }
}
