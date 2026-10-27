use std::io;

use super::find_in_path;
use super::types::KillSwitchConfig;

pub const CHAIN_NAME: &str = "XRAT_KS";

pub fn is_available() -> bool {
    find_in_path("iptables").is_some()
}

pub fn apply(config: &KillSwitchConfig) -> io::Result<()> {
    apply_v4(config)?;
    apply_v6(config)?;
    Ok(())
}

fn apply_v4(config: &KillSwitchConfig) -> io::Result<()> {
    let _ = std::process::Command::new("iptables")
        .args(["-N", CHAIN_NAME])
        .output();

    let check = std::process::Command::new("iptables")
        .args(["-C", "OUTPUT", "-j", CHAIN_NAME])
        .output()?;
    if !check.status.success() {
        let _ = std::process::Command::new("iptables")
            .args(["-I", "OUTPUT", "1", "-j", CHAIN_NAME])
            .output()?;
    }

    let _ = std::process::Command::new("iptables")
        .args(["-F", CHAIN_NAME])
        .output()?;

    let _ = std::process::Command::new("iptables")
        .args(["-A", CHAIN_NAME, "-o", "lo", "-j", "ACCEPT"])
        .output()?;

    let tun = config.tun_interface.trim();
    if !tun.is_empty() {
        let _ = std::process::Command::new("iptables")
            .args(["-A", CHAIN_NAME, "-o", tun, "-j", "ACCEPT"])
            .output()?;
    }

    if let Some(mark) = config.socket_mark.filter(|m| *m > 0) {
        let _ = std::process::Command::new("iptables")
            .args([
                "-A",
                CHAIN_NAME,
                "-m",
                "mark",
                "--mark",
                &format!("{mark:#x}"),
                "-j",
                "ACCEPT",
            ])
            .output()?;
    }

    for endpoint in &config.allowed_endpoints {
        if let std::net::SocketAddr::V4(v4) = endpoint {
            let ip_str = v4.ip().to_string();
            let port_str = v4.port().to_string();
            for proto in ["tcp", "udp"] {
                let _ = std::process::Command::new("iptables")
                    .args([
                        "-A", CHAIN_NAME, "-d", &ip_str, "-p", proto, "--dport", &port_str, "-j",
                        "ACCEPT",
                    ])
                    .output()?;
            }
        }
    }

    let _ = std::process::Command::new("iptables")
        .args([
            "-A", CHAIN_NAME, "-p", "udp", "--sport", "68", "--dport", "67", "-j", "ACCEPT",
        ])
        .output()?;

    if config.allow_lan {
        for cidr in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "224.0.0.0/4",
            "255.255.255.255/32",
        ] {
            let _ = std::process::Command::new("iptables")
                .args(["-A", CHAIN_NAME, "-d", cidr, "-j", "ACCEPT"])
                .output()?;
        }
    }

    let output = std::process::Command::new("iptables")
        .args(["-A", CHAIN_NAME, "-j", "DROP"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "iptables failed to append drop rule: {stderr}"
        )));
    }
    Ok(())
}

fn apply_v6(config: &KillSwitchConfig) -> io::Result<()> {
    if find_in_path("ip6tables").is_none() {
        return Ok(());
    }

    let _ = std::process::Command::new("ip6tables")
        .args(["-N", CHAIN_NAME])
        .output();

    let check = std::process::Command::new("ip6tables")
        .args(["-C", "OUTPUT", "-j", CHAIN_NAME])
        .output()?;
    if !check.status.success() {
        let _ = std::process::Command::new("ip6tables")
            .args(["-I", "OUTPUT", "1", "-j", CHAIN_NAME])
            .output()?;
    }

    let _ = std::process::Command::new("ip6tables")
        .args(["-F", CHAIN_NAME])
        .output()?;

    let _ = std::process::Command::new("ip6tables")
        .args(["-A", CHAIN_NAME, "-o", "lo", "-j", "ACCEPT"])
        .output()?;

    let tun = config.tun_interface.trim();
    if !tun.is_empty() {
        let _ = std::process::Command::new("ip6tables")
            .args(["-A", CHAIN_NAME, "-o", tun, "-j", "ACCEPT"])
            .output()?;
    }

    if let Some(mark) = config.socket_mark.filter(|m| *m > 0) {
        let _ = std::process::Command::new("ip6tables")
            .args([
                "-A",
                CHAIN_NAME,
                "-m",
                "mark",
                "--mark",
                &format!("{mark:#x}"),
                "-j",
                "ACCEPT",
            ])
            .output()?;
    }

    if !config.block_ipv6 {
        for endpoint in &config.allowed_endpoints {
            if let std::net::SocketAddr::V6(v6) = endpoint {
                let ip_str = v6.ip().to_string();
                let port_str = v6.port().to_string();
                for proto in ["tcp", "udp"] {
                    let _ = std::process::Command::new("ip6tables")
                        .args([
                            "-A", CHAIN_NAME, "-d", &ip_str, "-p", proto, "--dport", &port_str,
                            "-j", "ACCEPT",
                        ])
                        .output()?;
                }
            }
        }
    }

    let _ = std::process::Command::new("ip6tables")
        .args([
            "-A", CHAIN_NAME, "-p", "udp", "--sport", "546", "--dport", "547", "-j", "ACCEPT",
        ])
        .output()?;

    for icmp_type in [
        "router-solicitation",
        "router-advertisement",
        "neighbor-solicitation",
        "neighbor-advertisement",
    ] {
        let _ = std::process::Command::new("ip6tables")
            .args([
                "-A",
                CHAIN_NAME,
                "-p",
                "icmpv6",
                "--icmpv6-type",
                icmp_type,
                "-j",
                "ACCEPT",
            ])
            .output()?;
    }

    if config.allow_lan {
        for cidr in ["fe80::/10", "fc00::/7", "ff00::/8"] {
            let _ = std::process::Command::new("ip6tables")
                .args(["-A", CHAIN_NAME, "-d", cidr, "-j", "ACCEPT"])
                .output()?;
        }
    }

    let output = std::process::Command::new("ip6tables")
        .args(["-A", CHAIN_NAME, "-j", "DROP"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "ip6tables failed to append drop rule: {stderr}"
        )));
    }
    Ok(())
}

pub fn remove() -> io::Result<()> {
    remove_v4()?;
    remove_v6()?;
    Ok(())
}

fn remove_v4() -> io::Result<()> {
    let _ = std::process::Command::new("iptables")
        .args(["-D", "OUTPUT", "-j", CHAIN_NAME])
        .output();
    let _ = std::process::Command::new("iptables")
        .args(["-F", CHAIN_NAME])
        .output();
    let _ = std::process::Command::new("iptables")
        .args(["-X", CHAIN_NAME])
        .output();
    Ok(())
}

fn remove_v6() -> io::Result<()> {
    if find_in_path("ip6tables").is_none() {
        return Ok(());
    }
    let _ = std::process::Command::new("ip6tables")
        .args(["-D", "OUTPUT", "-j", CHAIN_NAME])
        .output();
    let _ = std::process::Command::new("ip6tables")
        .args(["-F", CHAIN_NAME])
        .output();
    let _ = std::process::Command::new("ip6tables")
        .args(["-X", CHAIN_NAME])
        .output();
    Ok(())
}

pub fn is_active() -> io::Result<bool> {
    let output = std::process::Command::new("iptables")
        .args(["-C", "OUTPUT", "-j", CHAIN_NAME])
        .output()?;
    Ok(output.status.success())
}
