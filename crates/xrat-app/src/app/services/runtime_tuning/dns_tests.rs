use super::*;
use crate::app::config::*;
use serde_json::json;

fn settings() -> DnsSettings {
    DnsSettings {
        query_strategy: "UseIP".into(),
        use_system_hosts: false,
        servers: vec![],
        resolvers: vec![
            DnsResolverSettings {
                tag: "bootstrap".into(),
                address: "udp://192.0.2.2:5353".into(),
                path: DnsResolverPath::Bootstrap,
            },
            DnsResolverSettings {
                tag: "remote".into(),
                address: "tcp://192.0.2.2:5353".into(),
                path: DnsResolverPath::Proxy,
            },
        ],
        bootstrap_resolver: "bootstrap".into(),
        final_resolver: "remote".into(),
        listener: DnsListenerSettings {
            enabled: true,
            host: "127.0.0.1".into(),
            port: 1053,
        },
        ..Default::default()
    }
}

#[test]
fn validates_dns_references_bootstrap_and_engine_gates() {
    let mut dns = settings();
    assert!(dns_validation::validate(&dns, "xray", false).is_ok());
    assert!(dns_validation::validate(&dns, "v2ray", false).is_err());
    dns.bootstrap_resolver = "missing".into();
    assert!(
        dns_validation::validate(&dns, "xray", false)
            .unwrap_err()
            .contains("bootstrap_resolver")
    );
    dns.bootstrap_resolver = "bootstrap".into();
    dns.resolvers[0].address = "https://resolver.test/dns-query".into();
    assert!(
        dns_validation::validate(&dns, "xray", false)
            .unwrap_err()
            .contains("literal-IP")
    );
    dns.resolvers[0].address = "udp://192.0.2.2:5353".into();
    dns.resolvers.push(dns.resolvers[0].clone());
    assert!(
        dns_validation::validate(&dns, "xray", false)
            .unwrap_err()
            .contains("unique")
    );
}

#[test]
fn rejects_dns_listener_collisions_before_launch() {
    let mut dns = settings();
    let mut runtime = RuntimeSettings::default();
    dns.listener.port = runtime.socks.port;
    assert!(dns_validation::validate_listener_ports(&dns, &runtime).is_err());
    runtime.socks.enabled = false;
    assert!(dns_validation::validate_listener_ports(&dns, &runtime).is_ok());
    dns.listener.port = runtime.stats.port;
    assert!(dns_validation::validate_listener_ports(&dns, &runtime).is_err());
}

#[test]
fn resolver_tags_cannot_redirect_ordinary_proxy_inbounds() {
    for tag in [
        "socks-in",
        "http-in",
        "shadowsocks-in",
        "tun-in",
        "probe-in",
        "api",
    ] {
        let mut dns = settings();
        dns.resolvers[0].tag = tag.into();
        dns.bootstrap_resolver = tag.into();
        assert!(
            dns_validation::validate(&dns, "xray", false)
                .unwrap_err()
                .contains("reserved")
        );
    }
}

#[test]
fn preserves_order_and_or_match_semantics_for_singbox() {
    let mut dns = settings();
    dns.rules = vec![
        DnsPolicyRule {
            domain: vec!["one.test".into()],
            domain_suffix: vec!["suffix.test".into()],
            resolver: "bootstrap".into(),
        },
        DnsPolicyRule {
            domain: vec!["suffix.test".into()],
            domain_suffix: vec![],
            resolver: "remote".into(),
        },
    ];
    let output = build_singbox_dns_options(&dns).unwrap().unwrap();
    assert_eq!(output.rules[0]["domain"], json!(["one.test"]));
    assert_eq!(output.rules[1]["domain_suffix"], json!(["suffix.test"]));
    assert_eq!(output.rules[2]["server"], "remote");
    assert!(
        dns_policy::xray_servers(&dns)
            .unwrap_err()
            .to_string()
            .contains("overlapping")
    );
}

#[test]
fn fakeip_validates_range_cache_exclusions_and_persistence() {
    let mut dns = settings();
    dns.fakeip.enabled = true;
    assert!(dns_validation::validate(&dns, "xray", false).is_ok());
    dns.fakeip.ipv4_range = "192.168.0.0/16".into();
    assert!(dns_validation::validate(&dns, "xray", false).is_err());
    dns.fakeip.ipv4_range = "198.18.0.0/15".into();
    dns.fakeip.persist = true;
    assert!(dns_validation::validate(&dns, "xray", false).is_err());
    assert!(dns_validation::validate(&dns, "sing-box", false).is_ok());
    dns.disable_cache = true;
    assert!(dns_validation::validate(&dns, "sing-box", false).is_err());
}

#[test]
fn editor_does_not_turn_structured_dns_rules_into_empty_lists() {
    let path = tempfile::tempdir().unwrap();
    let config_path = path.path().join("config.toml");
    std::fs::write(&config_path,"[dns]\nresolvers=[{tag='bootstrap',address='udp://192.0.2.2',path='bootstrap'}]\nbootstrap_resolver='bootstrap'\nfinal_resolver='bootstrap'\n").unwrap();
    let session = crate::app::config::ConfigEditSession::open(&config_path).unwrap();
    assert!(
        !session
            .settings
            .iter()
            .any(|setting| setting.path == "dns.resolvers")
    );
}

#[test]
#[ignore = "requires checksum-pinned runtime binaries and an explicit fixture directory"]
fn dns_runtime_native_fixtures() {
    let directory = std::path::PathBuf::from(
        std::env::var("XRAT_RUNTIME_FIXTURE_DIR").expect("fixture directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let node = xrat_config::parse_link("socks5://192.0.2.2:1081")
        .unwrap()
        .unwrap();
    for (engine, label) in [
        ("xray", "xray"),
        ("xray", "xray-new"),
        ("sing-box", "sing-box"),
    ] {
        let binary = std::env::var(if engine == "xray" {
            "XRAT_RUNTIME_XRAY"
        } else {
            "XRAT_RUNTIME_SINGBOX"
        })
        .expect("pinned engine");
        let binary = if label == "xray-new" {
            std::path::Path::new(&binary).with_file_name("xray-new")
        } else {
            std::path::PathBuf::from(binary)
        };
        if engine == "xray" {
            let spawner = std::sync::Arc::new(xrat_support::process::SystemProcessSpawner);
            ensure_xray_tun_supported_with_spawner(&binary, spawner.clone()).unwrap();
            assert!(
                ensure_xray_tun_supported_with_spawner(&binary.with_file_name("xray-old"), spawner)
                    .unwrap_err()
                    .to_string()
                    .contains("26.7.11")
            );
        }
        for mode in [
            "real",
            "fake",
            "encrypted",
            "rewrite",
            "drop",
            "reject",
            "forward",
            "hijack",
        ] {
            if engine == "sing-box" && !matches!(mode, "real" | "fake" | "encrypted") {
                continue;
            }
            let mut dns = settings();
            dns.rules = vec![DnsPolicyRule {
                domain: vec!["direct.test".into()],
                domain_suffix: vec![],
                resolver: "bootstrap".into(),
            }];
            dns.hosts
                .insert("fixed.test".into(), DnsHostValue::One("203.0.113.2".into()));
            if mode == "encrypted" {
                dns.resolvers[1].address = "https://resolver.test:5443/dns-query".into();
                dns.hosts.insert(
                    "resolver.test".into(),
                    DnsHostValue::One("203.0.113.2".into()),
                );
            }
            if mode == "fake" {
                dns.fakeip.enabled = true;
                dns.fakeip.ipv6_range = "fd00:198:18::/96".into();
                dns.fakeip.exclude = vec!["excluded.test".into()];
                dns.fakeip.persist = engine == "sing-box";
                // Native Xray exclusions are real bootstrap snapshots.
                if engine == "xray" {
                    dns.hosts.insert(
                        "excluded.test".into(),
                        DnsHostValue::One("203.0.113.2".into()),
                    );
                }
            }
            if !matches!(mode, "real" | "fake" | "encrypted") {
                let action = match mode {
                    "drop" => DnsAction::Drop,
                    "reject" => DnsAction::Return,
                    "forward" | "rewrite" => DnsAction::Direct,
                    _ => DnsAction::Hijack,
                };
                dns.outbound = Some(DnsOutboundSettings {
                    rewrite_address: matches!(mode, "rewrite" | "forward")
                        .then(|| "192.0.2.2".into()),
                    rewrite_port: matches!(mode, "rewrite" | "forward").then_some(5353),
                    rewrite_network: (mode == "forward").then_some(DnsNetwork::Tcp),
                    rules: vec![DnsOutboundRule {
                        action,
                        domain: vec![],
                        query_type: vec![],
                        response_code: if mode == "reject" { 5 } else { 0 },
                    }],
                    ..Default::default()
                });
            }
            dns_validation::validate(&dns, engine, false).unwrap();
            let mut config = if engine == "xray" {
                let mut options = xrat_engines::xray::XrayGenOptions::default();
                apply_xray_dns_options(&mut options, &dns).unwrap();
                let mut config =
                    xrat_engines::xray::generate_runtime_config_for_inbounds_with_options(
                        &node,
                        Some(("127.0.0.1", 1080, true)),
                        None,
                        &options,
                    )
                    .unwrap();
                apply_xray_dns_runtime(&mut config, &dns, false).unwrap();
                serde_json::to_value(config).unwrap()
            } else {
                let options = build_singbox_dns_options(&dns).unwrap();
                let mut config = xrat_engines::singbox::generate_singbox_runtime_config_with_dns(
                    &node,
                    vec![xrat_engines::singbox::SingboxInbound::socks(
                        "socks-in",
                        "127.0.0.1",
                        1080,
                        None,
                    )],
                    None,
                    None,
                    options.as_ref(),
                )
                .unwrap();
                apply_singbox_dns_runtime(&mut config, &dns, false, &directory).unwrap();
                serde_json::to_value(config).unwrap()
            };
            if mode == "fake" {
                let rules = config[if engine == "xray" { "routing" } else { "route" }]["rules"]
                    .as_array_mut()
                    .unwrap();
                rules.insert(0, if engine == "xray" {
                    json!({"type":"field","domain":["full:fake-direct.test"],"outboundTag":"direct"})
                } else {
                    json!({"domain":["fake-direct.test"],"action":"route","outbound":"direct"})
                });
            }
            let file = directory.join(format!("{label}-{mode}.json"));
            std::fs::write(&file, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
            let mut command = std::process::Command::new(&binary);
            command
                .args(if engine == "xray" {
                    vec!["run", "-test", "-c"]
                } else {
                    vec!["check", "-c"]
                })
                .arg(&file);
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{engine}/{mode}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
