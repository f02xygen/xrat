mod common;

use xrat_sdk::{
    config::parse_link,
    engines::{singbox, xray},
    model::{Node, Protocol},
};

#[test]
fn parsed_nodes_round_trip_and_generate_both_engine_configs() {
    for node in common::nodes::nodes() {
        let json = serde_json::to_string(&node).unwrap();
        assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), node);

        let config = xray::generate_runtime_config(&node, 1080, Some(8080)).unwrap();
        let config = serde_json::to_value(config).unwrap();
        assert_eq!(config["inbounds"][0]["port"], 1080);
        assert_eq!(config["inbounds"][1]["port"], 8080);
        assert!(!config["outbounds"].as_array().unwrap().is_empty());

        let config = singbox::generate_singbox_runtime_config(
            &node,
            vec![singbox::SingboxInbound::socks(
                "local",
                "127.0.0.1",
                1080,
                None,
            )],
            None,
            None,
        )
        .unwrap();
        let config = serde_json::to_value(config).unwrap();
        let expected = match node.protocol {
            Protocol::Ss => "shadowsocks",
            Protocol::Socks5 => "socks",
            Protocol::Hy2 => "hysteria2",
            _ => node.protocol.as_str(),
        };
        assert_eq!(config["outbounds"][0]["type"], expected);
    }
}

#[test]
fn engine_options_are_available_without_internal_crate_imports() {
    let node = parse_link("vless://11111111-1111-1111-1111-111111111111@example.com:443")
        .unwrap()
        .unwrap();
    let options = xray::XrayGenOptions {
        dns: Some(xray::XrayDnsConfig {
            servers: vec!["1.1.1.1".into()],
            hosts: Default::default(),
            query_strategy: "UseIP".into(),
            use_system_hosts: true,
            disable_cache: false,
            disable_fallback: false,
            disable_fallback_if_match: None,
            enable_parallel_query: false,
            tag: None,
        }),
        routing: Some(xray::XrayRoutingOptions {
            domain_strategy: "AsIs".into(),
            direct: xray::XrayRouteList {
                domain: vec!["full:localhost".into()],
                ..Default::default()
            },
            block: Default::default(),
        }),
        ..Default::default()
    };
    let config = xray::generate_runtime_config_for_inbounds_with_options(
        &node,
        Some(("127.0.0.1", 1080, false)),
        None,
        &options,
    )
    .unwrap();
    assert_eq!(config.dns.unwrap().servers, vec!["1.1.1.1"]);
    assert!(config.routing.is_some());
    let dns = singbox::SingboxDnsConfig {
        servers: vec![serde_json::json!({"type":"local", "tag":"local"})],
        rules: vec![],
        final_server: "local".into(),
        strategy: None,
        disable_cache: None,
        reverse_mapping: None,
    };
    let config = singbox::generate_singbox_runtime_config_with_dns(
        &node,
        vec![singbox::SingboxInbound::http("http", "127.0.0.1", 8080)],
        None,
        None,
        Some(&dns),
    )
    .unwrap();
    let route: singbox::SingboxRoute = config.route.unwrap();
    assert_eq!(route.default_domain_resolver.as_deref(), Some("local"));
}

#[test]
fn unsupported_settings_fail_instead_of_returning_misleading_json() {
    let mut node = parse_link("vless://11111111-1111-1111-1111-111111111111@127.0.0.1:443")
        .unwrap()
        .unwrap();
    node.uuid = None;
    assert!(xray::generate_probe_config(&node, 1080).is_err());
    assert!(singbox::generate_singbox_probe_config(&node, 1080).is_err());
}
