use crate::app::config::{DnsResolverPath, DnsSettings};
use crate::app::{AppError, Result};
use serde_json::json;
use xrat_engines::singbox::{SingboxConfig, SingboxDnsConfig, SingboxInbound, SingboxRoute};

pub(super) fn build_policy(dns: &DnsSettings) -> Result<SingboxDnsConfig> {
    if dns.disable_fallback || !dns.enable_parallel_query {
        return Err(AppError::InvalidArgument(
            "[dns] disable_fallback/serial-query settings have no exact sing-box policy mapping"
                .into(),
        ));
    }
    let mut servers = Vec::new();
    for resolver in &dns.resolvers {
        let (mut server, needs_resolver) =
            super::singbox::singbox_dns_server(&resolver.address, &resolver.tag)?;
        if needs_resolver {
            server["domain_resolver"] = json!(dns.bootstrap_resolver);
        }
        if resolver.path == DnsResolverPath::Proxy {
            server["detour"] = json!("proxy");
        }
        servers.push(server);
    }
    let strategy = match dns.query_strategy.as_str() {
        "UseIPv4" => Some("ipv4_only".into()),
        "UseIPv6" => Some("ipv6_only".into()),
        "UseIP" => None,
        "UseSystem" => return Err(AppError::InvalidArgument("[dns].query_strategy UseSystem has no exact sing-box named-policy mapping; choose UseIP/UseIPv4/UseIPv6".into())),
        _ => {
            return Err(AppError::InvalidArgument(
                "[dns].query_strategy is invalid".into(),
            ));
        }
    };
    let mut rules = Vec::new();
    if !dns.hosts.is_empty() || dns.use_system_hosts {
        let mut hosts = serde_json::Map::new();
        for (host, value) in &dns.hosts {
            let host = super::singbox::singbox_exact_host(host)?;
            hosts.insert(host, super::singbox::singbox_host_value(value)?);
        }
        let domains: Vec<_> = hosts.keys().cloned().collect();
        let mut server = json!({"type":"hosts","tag":"xrat-dns-hosts","predefined":hosts});
        if !dns.use_system_hosts {
            server["path"] = json!([]);
        }
        servers.push(server);
        if !domains.is_empty() {
            rules.push(json!({"domain":domains,"action":"route","server":"xrat-dns-hosts"}));
        }
        if dns.use_system_hosts {
            rules.push(json!({"ip_accept_any":true,"action":"route","server":"xrat-dns-hosts"}));
        }
    }
    let host_rule_count = rules.len();
    for rule in &dns.rules {
        let mut value = json!({"action":"route","server":rule.resolver});
        if !rule.domain.is_empty() {
            value["domain"] = json!(rule.domain);
        }
        if !rule.domain_suffix.is_empty() {
            value["domain_suffix"] = json!(rule.domain_suffix);
        }
        // Separate matchers implement the common OR contract; sing-box combines fields with AND.
        if !rule.domain.is_empty() && !rule.domain_suffix.is_empty() {
            rules.push(json!({"domain":rule.domain,"action":"route","server":rule.resolver}));
            rules.push(
                json!({"domain_suffix":rule.domain_suffix,"action":"route","server":rule.resolver}),
            );
        } else {
            rules.push(value);
        }
    }
    if dns.fakeip.enabled {
        let mut fake_rule_index = host_rule_count;
        if !dns.fakeip.exclude.is_empty() {
            rules.insert(
                fake_rule_index,
                json!({"domain":dns.fakeip.exclude,"action":"route","server":dns.final_resolver}),
            );
            fake_rule_index += 1;
        }
        let mut fake =
            json!({"type":"fakeip","tag":"xrat-fakeip","inet4_range":dns.fakeip.ipv4_range});
        if !dns.fakeip.ipv6_range.is_empty() {
            fake["inet6_range"] = json!(dns.fakeip.ipv6_range);
        }
        servers.push(fake);
        let queries = if dns.fakeip.ipv6_range.is_empty() {
            vec!["A"]
        } else {
            vec!["A", "AAAA"]
        };
        rules.insert(
            fake_rule_index,
            json!({"query_type":queries,"action":"route","server":"xrat-fakeip"}),
        );
    }
    Ok(SingboxDnsConfig {
        servers,
        rules,
        final_server: dns.final_resolver.clone(),
        strategy,
        disable_cache: dns.disable_cache.then_some(true),
        reverse_mapping: dns.fakeip.enabled.then_some(true),
    })
}

pub(crate) fn apply_runtime(
    config: &mut SingboxConfig,
    dns: &DnsSettings,
    tun: bool,
    cache_path: &std::path::Path,
) -> Result<()> {
    super::dns_validation::validate(dns, "sing-box", tun).map_err(AppError::InvalidArgument)?;
    let route = config.route.get_or_insert_with(|| SingboxRoute {
        rules: Vec::new(),
        rule_set: Vec::new(),
        final_outbound: "proxy".into(),
        default_domain_resolver: None,
        auto_detect_interface: None,
    });
    if !dns.resolvers.is_empty() {
        route.default_domain_resolver = Some(dns.bootstrap_resolver.clone());
        for outbound in &mut config.outbounds {
            if outbound["tag"] == "proxy" {
                outbound["domain_resolver"] = json!(dns.bootstrap_resolver);
            }
        }
    }
    if dns.listener.enabled {
        for inbound in &config.inbounds {
            let value = serde_json::to_value(inbound)?;
            if value["listen_port"] == dns.listener.port {
                return Err(AppError::InvalidArgument(
                    "[dns.listener].port collides with a managed inbound".into(),
                ));
            }
        }
        config.inbounds.push(SingboxInbound::Direct {
            tag: "xrat-dns-in".into(),
            listen: dns.listener.host.clone(),
            listen_port: dns.listener.port,
            override_address: "1.1.1.1".into(),
            override_port: 53,
        });
        route
            .rules
            .insert(0, json!({"inbound":["xrat-dns-in"],"action":"hijack-dns"}));
    }
    if tun && (!dns.resolvers.is_empty() || dns.fakeip.enabled) {
        route.rules.insert(
            0,
            json!({"inbound":["tun-in"],"port":53,"action":"hijack-dns"}),
        );
    }
    if dns.fakeip.enabled && dns.fakeip.persist {
        use std::hash::{Hash, Hasher};
        let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_vec(dns)?.hash(&mut fingerprint);
        for (host, value) in &dns.hosts {
            host.hash(&mut fingerprint);
            match value {
                crate::app::config::DnsHostValue::One(address) => address.hash(&mut fingerprint),
                crate::app::config::DnsHostValue::Many(addresses) => {
                    addresses.hash(&mut fingerprint)
                }
            }
        }
        let id = format!("dns-{:016x}", fingerprint.finish());
        let file = cache_path.join(format!("{id}.db"));
        std::fs::create_dir_all(cache_path)?;
        if let Ok(metadata) = std::fs::symlink_metadata(&file)
            && !metadata.is_file()
        {
            return Err(AppError::InvalidArgument(
                "[dns.fakeip] cache path must be a regular private file".into(),
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            std::fs::set_permissions(cache_path, std::fs::Permissions::from_mode(0o700))?;
        }
        let cache_file = options.open(&file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            cache_file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        drop(cache_file);
        config.enable_cache_file(file.display().to_string());
        if let Some(cache) = config
            .experimental
            .as_mut()
            .and_then(|experimental| experimental.cache_file.as_mut())
        {
            cache.store_fakeip = Some(true);
            cache.cache_id = Some(id);
        }
    }
    Ok(())
}
