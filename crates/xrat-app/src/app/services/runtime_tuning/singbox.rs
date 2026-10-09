use super::prelude::*;

pub(crate) fn build_singbox_dns_options(
    dns: &DnsSettings,
) -> crate::app::Result<Option<SingboxDnsConfig>> {
    if dns == &DnsSettings::default() {
        return Ok(None);
    }
    if !dns.resolvers.is_empty() {
        return super::dns_singbox::build_policy(dns).map(Some);
    }

    let strategy = match dns.query_strategy.as_str() {
        "UseIPv4" => "ipv4_only",
        "UseIPv6" => "ipv6_only",
        "UseIP" | "UseSystem" => {
            return Err(AppError::InvalidArgument(format!(
                "[dns].query_strategy = \"{}\" has no exact modern sing-box equivalent; use UseIPv4 or UseIPv6 for sing-box sessions",
                dns.query_strategy
            )));
        }
        other => {
            return Err(AppError::InvalidArgument(format!(
                "[dns].query_strategy must be UseIPv4 or UseIPv6 for sing-box sessions; got \"{other}\""
            )));
        }
    };

    if dns.disable_fallback {
        return Err(AppError::InvalidArgument(
            "[dns].disable_fallback is Xray/V2Ray-only and cannot be represented safely in sing-box"
                .to_string(),
        ));
    }
    if !dns.enable_parallel_query {
        return Err(AppError::InvalidArgument(
            "[dns].enable_parallel_query = false is Xray/V2Ray-only and cannot be represented safely in sing-box"
                .to_string(),
        ));
    }

    let mut servers = Vec::new();
    let mut final_server = None;
    let mut needs_local_resolver = false;
    for (index, server) in dns.servers.iter().enumerate() {
        let tag = format!("xrat-dns-{index}");
        let (value, needs_resolver) = singbox_dns_server(server, &tag)?;
        if final_server.is_none() {
            final_server = Some(tag);
        }
        needs_local_resolver |= needs_resolver;
        servers.push(value);
    }

    if needs_local_resolver {
        servers.push(json!({
            "type": "local",
            "tag": SINGBOX_LOCAL_DNS_TAG,
        }));
    }

    if final_server.is_none() {
        final_server = Some(SINGBOX_LOCAL_DNS_TAG.to_string());
        servers.push(json!({
            "type": "local",
            "tag": SINGBOX_LOCAL_DNS_TAG,
        }));
    }

    let mut rules = Vec::new();
    let mut predefined = serde_json::Map::new();
    let mut exact_hosts = Vec::new();
    for (host, value) in &dns.hosts {
        let host = singbox_exact_host(host)?;
        exact_hosts.push(host.clone());
        predefined.insert(host, singbox_host_value(value)?);
    }

    if dns.use_system_hosts || !predefined.is_empty() {
        let mut hosts_server = json!({
            "type": "hosts",
            "tag": SINGBOX_HOSTS_DNS_TAG,
            "predefined": predefined,
        });
        if !dns.use_system_hosts {
            hosts_server["path"] = json!([]);
        }
        servers.push(hosts_server);

        if !exact_hosts.is_empty() {
            rules.push(json!({
                "domain": exact_hosts,
                "action": "route",
                "server": SINGBOX_HOSTS_DNS_TAG,
            }));
        }
        if dns.use_system_hosts {
            rules.push(json!({
                "ip_accept_any": true,
                "action": "route",
                "server": SINGBOX_HOSTS_DNS_TAG,
            }));
        }
    }

    Ok(Some(SingboxDnsConfig {
        servers,
        rules,
        final_server: final_server.expect("a local fallback is always added"),
        strategy: Some(strategy.to_string()),
        disable_cache: dns.disable_cache.then_some(true),
        reverse_mapping: None,
    }))
}

pub(super) fn singbox_dns_server(raw: &str, tag: &str) -> crate::app::Result<(Value, bool)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::InvalidArgument(
            "[dns].servers cannot contain an empty server".to_string(),
        ));
    }
    if raw == "localhost" {
        return Ok((json!({"type": "local", "tag": tag}), false));
    }
    if raw == "fakedns" {
        return Err(AppError::InvalidArgument(
            "[dns].servers entry \"fakedns\" is not supported by the generated sing-box DNS configuration"
                .to_string(),
        ));
    }

    let (scheme, rest) = raw.split_once("://").unwrap_or(("udp", raw));
    let scheme = scheme.to_ascii_lowercase();
    let kind = match scheme.as_str() {
        "udp" | "udp+local" => "udp",
        "tcp" | "tcp+local" => "tcp",
        "tls" | "tls+local" => "tls",
        "quic" | "quic+local" => "quic",
        "https" | "https+local" => "https",
        "h3" | "h3+local" => "h3",
        "http" | "http+local" | "h2c" | "h2c+local" => {
            return Err(AppError::InvalidArgument(format!(
                "[dns].servers entry \"{raw}\" uses {scheme}, which has no safe modern sing-box mapping"
            )));
        }
        _ => {
            return Err(AppError::InvalidArgument(format!(
                "[dns].servers entry \"{raw}\" uses unsupported scheme \"{scheme}\""
            )));
        }
    };

    let url = Url::parse(&format!("{kind}://{rest}")).map_err(|error| {
        AppError::InvalidArgument(format!(
            "[dns].servers entry \"{raw}\" is not a valid {kind} DNS endpoint: {error}"
        ))
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidArgument(format!(
            "[dns].servers entry \"{raw}\" cannot contain credentials"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::InvalidArgument(format!(
            "[dns].servers entry \"{raw}\" cannot contain a query or fragment"
        )));
    }

    let host = url.host_str().ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "[dns].servers entry \"{raw}\" is missing a server host"
        ))
    })?;
    let is_https = matches!(kind, "https" | "h3");
    let host = host.trim_matches(['[', ']']);
    let path = url.path();
    if !is_https && !path.is_empty() && path != "/" {
        return Err(AppError::InvalidArgument(format!(
            "[dns].servers entry \"{raw}\" has a path, but {kind} endpoints do not support one"
        )));
    }

    let default_port = match kind {
        "tls" | "quic" => 853,
        "https" | "h3" => 443,
        _ => 53,
    };
    let port = url.port().unwrap_or(default_port);
    let needs_resolver = host.parse::<IpAddr>().is_err();
    let mut value = json!({
        "type": kind,
        "tag": tag,
        "server": host,
        "server_port": port,
    });
    if needs_resolver {
        value["domain_resolver"] = json!(SINGBOX_LOCAL_DNS_TAG);
    }
    if matches!(kind, "tls" | "quic" | "https" | "h3") {
        value["tls"] = json!({"server_name": host});
    }
    if is_https {
        value["path"] = json!(if path == "/" { "/dns-query" } else { path });
    }
    Ok((value, needs_resolver))
}

pub(super) fn singbox_exact_host(host: &str) -> crate::app::Result<String> {
    if let Some(host) = host.strip_prefix("full:") {
        if host.is_empty() {
            return Err(AppError::InvalidArgument(
                "[dns.hosts] contains an empty full: hostname".to_string(),
            ));
        }
        return Ok(host.to_string());
    }
    if host.is_empty()
        || host.starts_with("domain:")
        || host.starts_with("keyword:")
        || host.starts_with("regexp:")
        || host.starts_with("geosite:")
        || host.starts_with("ext:")
        || host.starts_with("dotless:")
    {
        return Err(AppError::InvalidArgument(format!(
            "[dns.hosts] key \"{host}\" is not an exact hostname; sing-box supports only plain and full: keys"
        )));
    }
    Ok(host.to_string())
}

pub(super) fn singbox_host_value(value: &DnsHostValue) -> crate::app::Result<Value> {
    let values = match value {
        DnsHostValue::One(value) => vec![value],
        DnsHostValue::Many(values) => values.iter().collect(),
    };
    if values.is_empty() || values.iter().any(|value| value.parse::<IpAddr>().is_err()) {
        return Err(AppError::InvalidArgument(
            "[dns.hosts] sing-box values must be non-empty IP addresses".to_string(),
        ));
    }
    if values.len() == 1 {
        Ok(json!(values[0]))
    } else {
        Ok(json!(values))
    }
}

pub(crate) fn build_singbox_routing_options(routing: &RoutingSettings) -> SingboxRoutingOptions {
    SingboxRoutingOptions {
        direct: singbox_route_list(&routing.direct),
        block: singbox_route_list(&routing.block),
    }
}

fn singbox_route_list(routes: &RouteList) -> SingboxRouteList {
    SingboxRouteList {
        domain: routes.domain.clone(),
        ip: routes.ip.clone(),
        geosite: routes.geosite.clone(),
        geoip: routes.geoip.clone(),
    }
}
