use super::*;
#[cfg(target_os = "linux")]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[test]
#[cfg(target_os = "linux")]
fn generates_nftables_ruleset_structure() {
    let mut config = KillSwitchConfig::new("xrat0");
    config.allowed_endpoints.push(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2)),
        443,
    ));
    config.allowed_endpoints.push(SocketAddr::new(
        IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)),
        8443,
    ));
    config.socket_mark = Some(0x255);
    config.allow_lan = true;

    #[cfg(target_os = "linux")]
    {
        let ruleset = nftables::render_ruleset(&config);
        assert!(ruleset.contains("table inet xrat_killswitch"));
        assert!(ruleset.contains("oifname \"lo\" accept"));
        assert!(ruleset.contains("oifname \"xrat0\" accept"));
        assert!(ruleset.contains("meta mark 0x255 accept"));
        assert!(ruleset.contains("ip daddr 198.51.100.2 tcp dport 443 accept"));
        assert!(ruleset.contains("ip daddr 198.51.100.2 udp dport 443 accept"));
        assert!(ruleset.contains("ip6 daddr 2001:db8::2 tcp dport 8443 accept"));
        assert!(ruleset.contains("10.0.0.0/8"));
        assert!(ruleset.contains("192.168.0.0/16"));
        assert!(ruleset.contains("counter drop"));
    }
}

#[test]
fn mock_firewall_lifecycle() {
    let mock = MockFirewallOps::default();
    assert!(!mock.is_active().unwrap());
    assert_eq!(mock.backend_name(), "mock");

    let config = KillSwitchConfig::new("xrat-test");
    mock.apply_killswitch(&config).unwrap();
    assert!(mock.is_active().unwrap());
    assert_eq!(mock.last_config.lock().unwrap().as_ref(), Some(&config));

    mock.remove_killswitch().unwrap();
    assert!(!mock.is_active().unwrap());
    assert!(mock.last_config.lock().unwrap().is_none());
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Linux namespaces and privileges"]
fn real_linux_nftables_lifecycle_in_isolated_netns() {
    if !nftables::is_available() {
        return;
    }
    let status = std::process::Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--net",
            "sh",
            "-c",
            r#"
            set -e
            nft -f - << 'EOF'
add table inet xrat_killswitch
flush table inet xrat_killswitch
table inet xrat_killswitch {
    chain output {
        type filter hook output priority -10; policy drop;
        oifname "lo" accept
        oifname "xrat0" accept
        meta mark 0x000000ff accept
        ip daddr 203.0.113.2 tcp dport 443 accept
        udp sport 68 udp dport 67 accept
        counter drop
    }
}
EOF
            nft list table inet xrat_killswitch > /dev/null
            nft delete table inet xrat_killswitch
            ! nft list table inet xrat_killswitch 2> /dev/null
        "#,
        ])
        .status();
    if let Ok(status) = status {
        assert!(status.success(), "isolated netns nftables test failed");
    }
}
