//! The host's network link, which `sceNetCtlGetInfo` reports as the console's own.
//!
//! The link is the adapter holding the address the host reaches the outside from
//! ([`orbistoun_fs::ifaddrs::outward_address`]): its netmask, default gateway, DNS servers, hardware
//! address and MTU, read from the operating system each time they are asked for, since a host's
//! network changes under a running title as a console's does.

use std::net::Ipv4Addr;

/// The host adapter a guest's network configuration comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The adapter's IPv4 address - the host's outward one.
    pub address: Ipv4Addr,
    /// Its netmask.
    pub netmask: Ipv4Addr,
    /// Its default gateway, if it has one.
    pub gateway: Option<Ipv4Addr>,
    /// Its IPv4 DNS servers, in the order the host uses them.
    pub dns: Vec<Ipv4Addr>,
    /// Its six-byte hardware address.
    pub hardware: [u8; 6],
    /// Its MTU in bytes.
    pub mtu: u32,
}

/// The host's link out, or `None` when the host has no route out or its adapter cannot be read.
#[must_use]
pub fn link() -> Option<Link> {
    adapter(orbistoun_fs::ifaddrs::outward_address()?)
}

/// The netmask of an on-link prefix `length` bits long.
#[must_use]
pub fn netmask_of(length: u8) -> Ipv4Addr {
    let bits = u32::MAX
        .checked_shl(32_u32.saturating_sub(u32::from(length)))
        .unwrap_or(0);
    Ipv4Addr::from(bits)
}

#[cfg(windows)]
fn adapter(address: Ipv4Addr) -> Option<Link> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST,
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    const ERROR_SUCCESS: u32 = 0;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut size: u32 = 16 * 1024;
    // Eight-byte words, so the list's structures are aligned; retried at the size asked for when
    // an adapter appears between the two calls.
    let mut buffer: Vec<u64>;
    loop {
        buffer = vec![0; (size as usize).div_ceil(8)];
        // SAFETY: `buffer` is at least `size` bytes of writable memory aligned for the structures
        // the call writes, and `size` is a live `u32` the call updates.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                &raw mut size,
            )
        };
        match rc {
            ERROR_SUCCESS => break,
            ERROR_BUFFER_OVERFLOW => {}
            _ => return None,
        }
    }
    let mut next = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !next.is_null() {
        // SAFETY: a node of the list the call wrote into `buffer`, which outlives this loop.
        let node = unsafe { &*next };
        next = node.Next;
        let mut unicast = node.FirstUnicastAddress;
        let mut prefix = None;
        while !unicast.is_null() {
            // SAFETY: a node of the adapter's unicast list, inside `buffer`.
            let entry = unsafe { &*unicast };
            unicast = entry.Next;
            if ipv4(entry.Address) == Some(address) {
                prefix = Some(entry.OnLinkPrefixLength);
            }
        }
        let Some(prefix) = prefix else {
            continue;
        };
        let mut gateway = None;
        let mut route = node.FirstGatewayAddress;
        while !route.is_null() && gateway.is_none() {
            // SAFETY: a node of the adapter's gateway list, inside `buffer`.
            let entry = unsafe { &*route };
            route = entry.Next;
            gateway = ipv4(entry.Address);
        }
        let mut dns = Vec::new();
        let mut server = node.FirstDnsServerAddress;
        while !server.is_null() {
            // SAFETY: a node of the adapter's DNS server list, inside `buffer`.
            let entry = unsafe { &*server };
            server = entry.Next;
            dns.extend(ipv4(entry.Address));
        }
        let mut hardware = [0; 6];
        if node.PhysicalAddressLength == 6 {
            hardware.copy_from_slice(&node.PhysicalAddress[..6]);
        }
        return Some(Link {
            address,
            netmask: netmask_of(prefix),
            gateway,
            dns,
            hardware,
            mtu: node.Mtu,
        });
    }
    None
}

/// The IPv4 address a `SOCKET_ADDRESS` holds, if it holds one: a `sockaddr_in` is the family,
/// the port, then the four address bytes.
#[cfg(windows)]
fn ipv4(socket: windows_sys::Win32::Networking::WinSock::SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    if socket.lpSockaddr.is_null() || socket.iSockaddrLength < 8 {
        return None;
    }
    // SAFETY: the address the list points at, at least `iSockaddrLength` (checked: 8 or more)
    // bytes long.
    let bytes = unsafe { std::slice::from_raw_parts(socket.lpSockaddr.cast::<u8>(), 8) };
    (u16::from_le_bytes([bytes[0], bytes[1]]) == AF_INET)
        .then(|| Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]))
}

#[cfg(not(windows))]
fn adapter(address: Ipv4Addr) -> Option<Link> {
    let routes = std::fs::read_to_string("/proc/net/route").ok()?;
    let (interface, netmask, gateway) = route_of(&routes, address)?;
    let class = format!("/sys/class/net/{interface}");
    let hardware = std::fs::read_to_string(format!("{class}/address"))
        .ok()
        .and_then(|text| hardware_address(&text))
        .unwrap_or([0; 6]);
    let mtu = std::fs::read_to_string(format!("{class}/mtu"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let dns = std::fs::read_to_string("/etc/resolv.conf")
        .map(|text| name_servers(&text))
        .unwrap_or_default();
    Some(Link {
        address,
        netmask,
        gateway,
        dns,
        hardware,
        mtu,
    })
}

/// A `/proc/net/route` address field: the `in_addr` printed as a host-order hexadecimal word.
fn route_address(field: &str) -> Option<Ipv4Addr> {
    u32::from_str_radix(field, 16)
        .ok()
        .map(|word| Ipv4Addr::from(word.to_le_bytes()))
}

/// From `/proc/net/route`'s text: the interface whose on-link route holds `address` (the longest
/// such prefix), that route's netmask, and the interface's default gateway.
#[must_use]
pub fn route_of(routes: &str, address: Ipv4Addr) -> Option<(String, Ipv4Addr, Option<Ipv4Addr>)> {
    let rows: Vec<Vec<&str>> = routes
        .lines()
        .skip(1)
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|fields| fields.len() >= 8)
        .collect();
    let (interface, netmask) = rows
        .iter()
        .filter_map(|fields| {
            let (destination, mask) = (route_address(fields[1])?, route_address(fields[7])?);
            let (bits, host) = (u32::from(mask), u32::from(address));
            (bits != 0 && host & bits == u32::from(destination)).then_some((fields[0], mask))
        })
        .max_by_key(|&(_, mask)| u32::from(mask))?;
    let gateway = rows
        .iter()
        .filter(|fields| {
            fields[0] == interface && fields[1] == "00000000" && fields[7] == "00000000"
        })
        .find_map(|fields| route_address(fields[2]));
    Some((interface.to_owned(), netmask, gateway))
}

/// A hardware address as `/sys/class/net/<interface>/address` prints it: six colon-separated
/// hexadecimal bytes.
#[must_use]
pub fn hardware_address(text: &str) -> Option<[u8; 6]> {
    let mut bytes = [0; 6];
    let mut parts = text.trim().split(':');
    for byte in &mut bytes {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(bytes)
}

/// The IPv4 `nameserver` lines of a `resolv.conf`, in order.
#[must_use]
pub fn name_servers(text: &str) -> Vec<Ipv4Addr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|rest| rest.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{hardware_address, name_servers, netmask_of, route_of};
    use std::net::Ipv4Addr;

    #[test]
    fn prefix_lengths_become_netmasks() {
        assert_eq!(netmask_of(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(netmask_of(0), Ipv4Addr::UNSPECIFIED);
        assert_eq!(netmask_of(32), Ipv4Addr::BROADCAST);
    }

    /// A Linux routing table with a default route and the subnet route of the same interface.
    #[test]
    fn the_route_table_names_the_interface_mask_and_gateway() {
        let routes = concat!(
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n",
            "eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n",
            "eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n",
        );
        let found = route_of(routes, Ipv4Addr::new(192, 168, 1, 205));
        assert_eq!(
            found,
            Some((
                "eth0".to_owned(),
                Ipv4Addr::new(255, 255, 255, 0),
                Some(Ipv4Addr::new(192, 168, 1, 1))
            ))
        );
    }

    #[test]
    fn hardware_addresses_and_name_servers_parse() {
        assert_eq!(
            hardware_address("bc:33:29:01:3d:e3\n"),
            Some([0xbc, 0x33, 0x29, 0x01, 0x3d, 0xe3])
        );
        assert_eq!(hardware_address("bc:33:29"), None);
        assert_eq!(
            name_servers("# comment\nnameserver 1.1.1.1\nnameserver fe80::1\nnameserver 8.8.8.8\n"),
            [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)]
        );
    }
}
