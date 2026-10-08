pub fn connect_host_for_bind_host(host: &str) -> String {
    match host {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "::1".to_string(),
        _ => host.to_string(),
    }
}

/// Best-effort primary LAN IP of this host. Opens a UDP socket toward a public
/// address (no packets are sent) so the OS picks the outbound interface, then
/// reads back its local address. Returns `None` when no route can be resolved.
pub trait LocalIpResolver: Send + Sync {
    fn primary_ip(&self) -> Option<std::net::IpAddr>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLocalIpResolver;
impl LocalIpResolver for SystemLocalIpResolver {
    fn primary_ip(&self) -> Option<std::net::IpAddr> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        socket.local_addr().ok().map(|addr| addr.ip())
    }
}

pub fn primary_local_ip() -> Option<String> {
    SystemLocalIpResolver.primary_ip().map(|ip| ip.to_string())
}

/// Resolve a host/port to an IP address. Used for DNS pre-resolution in TUN mode
/// to break routing loops before tunnel creation.
pub trait HostResolver: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> Option<std::net::IpAddr>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHostResolver;

impl HostResolver for SystemHostResolver {
    fn resolve(&self, host: &str, port: u16) -> Option<std::net::IpAddr> {
        use std::net::ToSocketAddrs;
        (host, port)
            .to_socket_addrs()
            .ok()?
            .next()
            .map(|addr| addr.ip())
    }
}

/// Resolve a network interface name to a bindable address, preferring IPv4.
/// Used to turn an inbound `listen_interface` setting into a concrete `listen`
/// address. Returns `None` when the interface is unknown or has no address.
pub fn interface_address(name: &str) -> Option<String> {
    let addrs = if_addrs::get_if_addrs().ok()?;
    let mut ipv6: Option<String> = None;
    for iface in addrs {
        if iface.name != name {
            continue;
        }
        let ip = iface.ip();
        if ip.is_ipv4() {
            return Some(ip.to_string());
        }
        ipv6.get_or_insert_with(|| ip.to_string());
    }
    ipv6
}

/// Whether a network interface with `name` currently exists. Checks the Linux
/// sysfs view first so interfaces without an assigned address are still
/// detected, then falls back to the cross-platform address list.
pub fn interface_exists(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    if std::path::Path::new("/sys/class/net").join(name).exists() {
        return true;
    }
    if_addrs::get_if_addrs()
        .map(|addrs| addrs.iter().any(|iface| iface.name == name))
        .unwrap_or(false)
}

/// Information about a network interface detected in the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelInterfaceInfo {
    pub name: String,
    pub ifindex: u32,
    pub is_tun: bool,
}

/// Operations on TUN interfaces, abstracting kernel interface inspection and
/// deletion so unit tests can run deterministically without modifying host devices.
pub trait TunInterfaceOps: Send + Sync {
    fn inspect_interface(&self, name: &str) -> std::io::Result<Option<KernelInterfaceInfo>>;
    fn delete_interface(&self, name: &str, expected_ifindex: u32) -> std::io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTunInterfaceOps;

impl TunInterfaceOps for SystemTunInterfaceOps {
    fn inspect_interface(&self, name: &str) -> std::io::Result<Option<KernelInterfaceInfo>> {
        let name = name.trim();
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Ok(None);
        }
        #[cfg(target_os = "linux")]
        {
            let sysfs_net = std::path::Path::new("/sys/class/net").join(name);
            if !sysfs_net.exists() {
                return Ok(None);
            }
            let ifindex = std::fs::read_to_string(sysfs_net.join("ifindex")).and_then(|value| {
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
            })?;

            let tun_flags_path = sysfs_net.join("tun_flags");
            let is_tun = if let Ok(content) = std::fs::read_to_string(&tun_flags_path) {
                let flags =
                    u32::from_str_radix(content.trim().trim_start_matches("0x"), 16).unwrap_or(0);
                (flags & 0x0003) == 0x0001
            } else {
                false
            };

            Ok(Some(KernelInterfaceInfo {
                name: name.to_string(),
                ifindex,
                is_tun,
            }))
        }
        #[cfg(not(target_os = "linux"))]
        {
            if interface_exists(name) {
                Ok(Some(KernelInterfaceInfo {
                    name: name.to_string(),
                    ifindex: 0,
                    is_tun: false,
                }))
            } else {
                Ok(None)
            }
        }
    }

    fn delete_interface(&self, _name: &str, expected_ifindex: u32) -> std::io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            linux_netlink::delete_link_by_index(expected_ifindex)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = expected_ifindex;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "in-process TUN interface deletion is only supported on Linux",
            ))
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_netlink {
    #[repr(C)]
    struct Nlmsghdr {
        nlmsg_len: u32,
        nlmsg_type: u16,
        nlmsg_flags: u16,
        nlmsg_seq: u32,
        nlmsg_pid: u32,
    }

    #[repr(C)]
    struct Ifinfomsg {
        ifi_family: u8,
        __ifi_pad: u8,
        ifi_type: u16,
        ifi_index: i32,
        ifi_flags: u32,
        ifi_change: u32,
    }

    unsafe extern "C" {
        fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
        fn send(fd: i32, buf: *const u8, len: usize, flags: i32) -> isize;
        fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
        fn close(fd: i32) -> i32;
    }

    const AF_NETLINK: i32 = 16;
    const SOCK_RAW: i32 = 3;
    const SOCK_CLOEXEC: i32 = 0x80000;
    const NETLINK_ROUTE: i32 = 0;
    const RTM_DELLINK: u16 = 17;
    const NLM_F_REQUEST: u16 = 1;
    const NLM_F_ACK: u16 = 4;
    const NLMSG_ERROR: u16 = 2;

    pub fn delete_link_by_index(ifindex: u32) -> std::io::Result<()> {
        let fd = unsafe { socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        struct FdGuard(i32);
        impl Drop for FdGuard {
            fn drop(&mut self) {
                unsafe { close(self.0) };
            }
        }
        let _guard = FdGuard(fd);

        let req_len = (std::mem::size_of::<Nlmsghdr>() + std::mem::size_of::<Ifinfomsg>()) as u32;
        #[repr(C)]
        struct DellinkRequest {
            hdr: Nlmsghdr,
            ifi: Ifinfomsg,
        }

        let req = DellinkRequest {
            hdr: Nlmsghdr {
                nlmsg_len: req_len,
                nlmsg_type: RTM_DELLINK,
                nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
                nlmsg_seq: 1,
                nlmsg_pid: 0,
            },
            ifi: Ifinfomsg {
                ifi_family: 0,
                __ifi_pad: 0,
                ifi_type: 0,
                ifi_index: ifindex as i32,
                ifi_flags: 0,
                ifi_change: 0,
            },
        };

        let bytes = unsafe {
            std::slice::from_raw_parts(
                &req as *const _ as *const u8,
                std::mem::size_of::<DellinkRequest>(),
            )
        };

        let sent = unsafe { send(fd, bytes.as_ptr(), bytes.len(), 0) };
        if sent < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut buf = [0u8; 1024];
        let n = unsafe { recv(fd, buf.as_mut_ptr(), buf.len(), 0) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if (n as usize) < std::mem::size_of::<Nlmsghdr>() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated netlink response",
            ));
        }

        let hdr = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const Nlmsghdr) };
        if hdr.nlmsg_type == NLMSG_ERROR {
            if (n as usize) < std::mem::size_of::<Nlmsghdr>() + 4 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "truncated netlink error",
                ));
            }
            let err_code = unsafe {
                std::ptr::read_unaligned(
                    buf.as_ptr().add(std::mem::size_of::<Nlmsghdr>()) as *const i32
                )
            };
            if err_code == 0 {
                return Ok(());
            }
            return Err(std::io::Error::from_raw_os_error(-err_code));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::interface_exists;

    #[test]
    fn detects_loopback_and_missing_interfaces() {
        assert!(interface_exists("lo"));
        assert!(!interface_exists("xrat-missing-interface"));
        assert!(!interface_exists("  "));
    }
}
