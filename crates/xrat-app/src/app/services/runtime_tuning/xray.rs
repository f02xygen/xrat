use super::network::*;
use super::prelude::*;

/// Translate runtime tuning settings into outbound generation options. Routing
/// is added separately for managed sessions so probe configs remain proxy-only.
pub(crate) fn build_xray_gen_options(runtime: &RuntimeSettings) -> XrayGenOptions {
    let mux = runtime.mux.enabled.then(|| MuxOptions {
        concurrency: runtime.mux.concurrency,
        xudp_concurrency: runtime.mux.xudp_concurrency,
        xudp_proxy_udp443: runtime.mux.xudp_proxy_udp443.clone(),
    });
    let fragment = runtime.fragment.enabled.then(|| FragmentOptions {
        packets: fragment_packets(runtime),
        length: format_range(runtime.fragment.length),
        interval: format_range(runtime.fragment.interval),
    });

    XrayGenOptions {
        compatibility: match runtime.xray_compatibility {
            XrayCompatibilityPolicy::Prerelease => XrayCompatibilityTarget::PrereleaseV26_7_28,
            XrayCompatibilityPolicy::Auto | XrayCompatibilityPolicy::Stable => {
                XrayCompatibilityTarget::StableV26_3_27
            }
        },
        mux,
        fragment,
        interface: non_empty(&runtime.network.interface),
        mark: (runtime.network.mark != 0).then_some(runtime.network.mark),
        bind_address: non_empty(&runtime.network.bind_address),
        routing: None,
        dns: None,
        dns_routes: vec![],
        singbox_dns: None,
        bootstrap_resolver: None,
        bootstrap_dns: None,
        bootstrap_hosts: vec![],
    }
}

pub(crate) fn detect_xray_compatibility(
    policy: XrayCompatibilityPolicy,
    binary_path: &Path,
) -> XrayCompatibilityTarget {
    detect_xray_compatibility_with_spawner(
        policy,
        binary_path,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

pub(crate) fn detect_xray_compatibility_with_spawner(
    policy: XrayCompatibilityPolicy,
    binary_path: &Path,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> XrayCompatibilityTarget {
    match policy {
        XrayCompatibilityPolicy::Stable => XrayCompatibilityTarget::StableV26_3_27,
        XrayCompatibilityPolicy::Prerelease => XrayCompatibilityTarget::PrereleaseV26_7_28,
        XrayCompatibilityPolicy::Auto => Command::with_spawner(binary_path, spawner.clone())
            .arg("version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .filter(|version| version.contains("26.7.28"))
            .map(|_| XrayCompatibilityTarget::PrereleaseV26_7_28)
            .unwrap_or(XrayCompatibilityTarget::StableV26_3_27),
    }
}

/// Minimum Xray version whose Linux TUN inbound configures the interface
/// address and system routes. Earlier releases create the device but leave it
/// unconfigured, so TUN capture would silently do nothing.
pub(crate) const XRAY_TUN_MIN_VERSION: (u32, u32, u32) = (26, 7, 11);

/// Reject a managed Xray binary that predates working TUN support. When the
/// version cannot be determined, report how to repair the configured binary.
pub(crate) fn ensure_xray_tun_supported_with_spawner(
    binary_path: &Path,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    let Some(version) = xray_binary_version_with_spawner(binary_path, spawner) else {
        return Err(AppError::InvalidArgument(format!(
            "[runtime.tun].enabled could not determine the Xray version at {}; ensure it is Xray >= {}.{}.{} with working TUN support",
            binary_path.display(),
            XRAY_TUN_MIN_VERSION.0,
            XRAY_TUN_MIN_VERSION.1,
            XRAY_TUN_MIN_VERSION.2,
        )));
    };
    if version < XRAY_TUN_MIN_VERSION {
        return Err(AppError::InvalidArgument(format!(
            "Cannot enable TUN: Xray {}.{}.{} is required, but {} reports {}.{}.{}. Upgrade the configured core with `xrat install xray --prerelease`, then run `xrat tun setup`.",
            XRAY_TUN_MIN_VERSION.0,
            XRAY_TUN_MIN_VERSION.1,
            XRAY_TUN_MIN_VERSION.2,
            binary_path.display(),
            version.0,
            version.1,
            version.2,
        )));
    }
    Ok(())
}

pub(crate) fn xray_binary_version_with_spawner(
    binary_path: &Path,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> Option<(u32, u32, u32)> {
    let output = Command::with_spawner(binary_path, spawner)
        .arg("version")
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    parse_xray_version(&String::from_utf8_lossy(&output.stdout))
}

pub(crate) fn parse_xray_version(text: &str) -> Option<(u32, u32, u32)> {
    for token in text.split(|character: char| !(character.is_ascii_digit() || character == '.')) {
        let mut parts = token.split('.');
        let (Some(major), Some(minor), Some(patch)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if let (Ok(major), Ok(minor), Ok(patch)) = (
            major.parse::<u32>(),
            minor.parse::<u32>(),
            patch.parse::<u32>(),
        ) {
            return Some((major, minor, patch));
        }
    }
    None
}

pub(crate) fn apply_xray_dns_options(
    options: &mut XrayGenOptions,
    dns: &DnsSettings,
) -> crate::app::Result<()> {
    if dns == &DnsSettings::default() {
        return Ok(());
    }

    let query_strategy = match dns.query_strategy.as_str() {
        "UseIP" | "UseIPv4" | "UseIPv6" | "UseSystem" => dns.query_strategy.clone(),
        other => {
            return Err(AppError::InvalidArgument(format!(
                "[dns].query_strategy must be UseIP, UseIPv4, UseIPv6, or UseSystem; got \"{other}\""
            )));
        }
    };

    let mut servers = Vec::with_capacity(dns.servers.len());
    for server in &dns.servers {
        let server = server.trim();
        if server.is_empty() {
            return Err(AppError::InvalidArgument(
                "[dns].servers cannot contain an empty server".to_string(),
            ));
        }
        servers.push(json!(server));
    }

    let hosts = dns
        .hosts
        .iter()
        .map(|(host, value)| {
            let value = match value {
                DnsHostValue::One(value) => XrayDnsHostValue::One(value.clone()),
                DnsHostValue::Many(value) => XrayDnsHostValue::Many(value.clone()),
            };
            (host.clone(), value)
        })
        .collect();

    if !dns.resolvers.is_empty() {
        servers = super::dns_policy::xray_servers(dns)?;
        options.dns_routes = super::dns_policy::resolver_routes(dns);
        for resolver in &dns.resolvers {
            let endpoint = super::dns_validation::endpoint(&resolver.address)
                .map_err(AppError::InvalidArgument)?;
            if let Some(host) = endpoint.host_str()
                && host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_err()
                && !options
                    .bootstrap_hosts
                    .iter()
                    .any(|existing| existing == host)
            {
                options.bootstrap_hosts.push(host.to_string());
            }
        }
        let bootstrap = dns
            .resolvers
            .iter()
            .find(|resolver| resolver.tag == dns.bootstrap_resolver)
            .ok_or_else(|| {
                AppError::InvalidArgument("[dns].bootstrap_resolver is missing".into())
            })?;
        options.bootstrap_dns = Some(
            super::dns_validation::bootstrap_socket(&bootstrap.address)
                .map_err(AppError::InvalidArgument)?,
        );
    }
    options.dns = Some(XrayDnsConfig {
        servers,
        hosts,
        query_strategy,
        use_system_hosts: dns.use_system_hosts,
        disable_cache: dns.disable_cache,
        disable_fallback: dns.disable_fallback,
        disable_fallback_if_match: (!dns.resolvers.is_empty()).then_some(true),
        enable_parallel_query: dns.enable_parallel_query,
        tag: None,
    });
    Ok(())
}

pub(crate) fn apply_xray_routing_options(options: &mut XrayGenOptions, routing: &RoutingSettings) {
    options.routing = Some(XrayRoutingOptions {
        domain_strategy: routing.domain_strategy.clone(),
        direct: xray_route_list(&routing.direct),
        block: xray_route_list(&routing.block),
    });
}

fn xray_route_list(routes: &RouteList) -> XrayRouteList {
    XrayRouteList {
        domain: routes.domain.clone(),
        ip: routes.ip.clone(),
        geosite: routes.geosite.clone(),
        geoip: routes.geoip.clone(),
    }
}
