use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

use crate::app::config::{DnsAction, DnsResolverPath, DnsSettings};

pub(crate) fn validate_listener_ports(
    dns: &DnsSettings,
    runtime: &crate::app::config::RuntimeSettings,
) -> Result<(), String> {
    if dns.listener.enabled
        && [
            (runtime.socks.enabled, runtime.socks.port),
            (runtime.http.enabled, runtime.http.port),
            (runtime.shadowsocks.enabled, runtime.shadowsocks.port),
            (runtime.stats.enabled, runtime.stats.port),
        ]
        .iter()
        .any(|(enabled, port)| *enabled && *port == dns.listener.port)
    {
        return Err("[dns.listener].port collides with a managed proxy or stats listener".into());
    }
    Ok(())
}

pub(crate) fn validate(dns: &DnsSettings, engine: &str, tun: bool) -> Result<(), String> {
    let advanced = !dns.resolvers.is_empty()
        || dns.listener.enabled
        || dns.fakeip.enabled
        || dns.outbound.is_some();
    if advanced && engine == "v2ray" {
        return Err("[dns] advanced resolver/listener/FakeIP/outbound settings are not validated for V2Ray; use Xray or sing-box".into());
    }
    if dns.resolvers.is_empty() {
        if !dns.rules.is_empty()
            || !dns.final_resolver.is_empty()
            || !dns.bootstrap_resolver.is_empty()
        {
            return Err("[dns.resolvers] named policies require configured resolvers".into());
        }
    } else {
        if !dns.servers.is_empty() {
            return Err("[dns] servers and named resolvers cannot be combined; migrate server entries into resolvers".into());
        }
        let mut tags = BTreeSet::new();
        for resolver in &dns.resolvers {
            if !valid_tag(&resolver.tag)
                || resolver.tag.starts_with("xrat-")
                || matches!(
                    resolver.tag.as_str(),
                    "socks-in" | "http-in" | "shadowsocks-in" | "tun-in" | "probe-in" | "api"
                )
                || !tags.insert(&resolver.tag)
            {
                return Err("[dns.resolvers].tag must be unique, nonempty and contain only letters, digits, '-' or '_'; xrat- and managed inbound tags are reserved".into());
            }
            endpoint(&resolver.address)?;
            if engine == "xray" && endpoint(&resolver.address)?.scheme().starts_with("tls") {
                return Err("[dns.resolvers].address DNS-over-TLS is unsupported by Xray; use HTTPS/TCP/UDP".into());
            }
        }
        let bootstrap = dns
            .resolvers
            .iter()
            .find(|resolver| resolver.tag == dns.bootstrap_resolver)
            .ok_or("[dns].bootstrap_resolver must reference a configured resolver")?;
        if bootstrap.path != DnsResolverPath::Bootstrap {
            return Err("[dns].bootstrap_resolver must select a Bootstrap resolver".into());
        }
        bootstrap_socket(&bootstrap.address)?;
        if !tags.contains(&dns.final_resolver) {
            return Err("[dns].final_resolver must reference a configured resolver".into());
        }
        for rule in &dns.rules {
            if !tags.contains(&rule.resolver)
                || (rule.domain.is_empty() && rule.domain_suffix.is_empty())
            {
                return Err("[dns.rules] each rule needs a configured resolver and a nonempty domain matcher".into());
            }
            for domain in rule.domain.iter().chain(&rule.domain_suffix) {
                if !valid_domain(domain) {
                    return Err(
                        "[dns.rules] domain matches must be plain exact/suffix hostnames".into(),
                    );
                }
            }
        }
    }
    if dns.listener.enabled
        && (dns.listener.port == 0
            || dns.listener.host.parse::<IpAddr>().is_err()
            || !dns
                .listener
                .host
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()))
    {
        return Err("[dns.listener] requires a loopback IP host and a nonzero port".into());
    }
    if let Some(outbound) = &dns.outbound {
        if engine != "xray" {
            return Err("[dns.outbound] is Xray-only; sing-box uses hijack-dns".into());
        }
        if !valid_tag(&outbound.tag)
            || matches!(
                outbound.tag.as_str(),
                "proxy" | "direct" | "block" | "fragment" | "api" | "dns-out"
            )
        {
            return Err(
                "[dns.outbound].tag is invalid or collides with a generated outbound".into(),
            );
        }
        if outbound.rewrite_port == Some(0) {
            return Err("[dns.outbound].rewrite_port must be nonzero".into());
        }
        if let Some(address) = &outbound.rewrite_address
            && address.parse::<IpAddr>().is_err()
            && !valid_domain(address)
        {
            return Err(
                "[dns.outbound].rewrite_address must be an IP address or plain hostname".into(),
            );
        }
        for rule in &outbound.rules {
            if rule.domain.iter().any(|domain| domain.trim().is_empty())
                || rule.query_type.contains(&0)
            {
                return Err(
                    "[dns.outbound.rules] domain matches and query types cannot be empty/zero"
                        .into(),
                );
            }
            if rule.response_code > 15
                || (rule.response_code != 0 && rule.action != DnsAction::Return)
            {
                return Err("[dns.outbound.rules].response_code is 0..15 and only applies to return actions".into());
            }
        }
    }
    if dns.fakeip.enabled {
        if engine == "xray" && dns.query_strategy != "UseIP" {
            return Err(
                "[dns.fakeip] Xray requires query_strategy = UseIP for fake-address allocation"
                    .into(),
            );
        }
        if engine == "sing-box" && dns.fakeip.pool_size != 65535 {
            return Err(
                "[dns.fakeip].pool_size is Xray-only; sing-box allocates from the configured CIDR"
                    .into(),
            );
        }
        if !tun && !dns.listener.enabled {
            return Err("[dns.fakeip] requires an enabled DNS listener or managed TUN".into());
        }
        if dns.disable_cache {
            return Err("[dns.fakeip] requires DNS caching for domain recovery".into());
        }
        if dns.resolvers.is_empty() {
            return Err(
                "[dns.fakeip] requires named real resolvers for exclusions and bootstrap".into(),
            );
        }
        let (ip, prefix) = cidr(&dns.fakeip.ipv4_range)?;
        let IpAddr::V4(ip) = ip else {
            return Err("[dns.fakeip].ipv4_range must be IPv4".into());
        };
        if prefix < 15
            || u32::from(ip) & 0xfffe0000 != u32::from(std::net::Ipv4Addr::new(198, 18, 0, 0))
        {
            return Err(
                "[dns.fakeip].ipv4_range must be within the reserved 198.18.0.0/15 range".into(),
            );
        }
        if dns.fakeip.pool_size == 0
            || (engine == "xray" && u64::from(dns.fakeip.pool_size) > (1u64 << (32 - prefix)))
        {
            return Err("[dns.fakeip].pool_size must fit the configured IPv4 pool".into());
        }
        if !dns.fakeip.ipv6_range.is_empty() {
            let (ip, prefix) = cidr(&dns.fakeip.ipv6_range)?;
            let IpAddr::V6(ip) = ip else {
                return Err("[dns.fakeip].ipv6_range must be IPv6".into());
            };
            if prefix < 7 || ip.segments()[0] & 0xfe00 != 0xfc00 {
                return Err("[dns.fakeip].ipv6_range must be an IPv6 ULA prefix".into());
            }
            if engine == "xray"
                && prefix > 96
                && u128::from(dns.fakeip.pool_size) > (1u128 << (128 - prefix))
            {
                return Err("[dns.fakeip].pool_size must fit the configured IPv6 pool".into());
            }
        }
        if engine == "xray" && dns.fakeip.persist {
            return Err(
                "[dns.fakeip].persist is sing-box-only; Xray mappings reset on restart".into(),
            );
        }
        if dns
            .fakeip
            .exclude
            .iter()
            .any(|domain| !valid_domain(domain))
        {
            return Err("[dns.fakeip].exclude must contain plain exact hostnames".into());
        }
    }
    Ok(())
}

pub(crate) fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

pub(crate) fn validate_tun_ranges(
    dns: &DnsSettings,
    tun: &crate::app::config::TunSettings,
) -> Result<(), String> {
    if !dns.fakeip.enabled || !tun.enabled {
        return Ok(());
    }
    for pool in [&dns.fakeip.ipv4_range, &dns.fakeip.ipv6_range] {
        if pool.is_empty() {
            continue;
        }
        let (pool_ip, pool_prefix) = cidr(pool)?;
        for configured in tun.address.iter().chain(&tun.route_exclude_address) {
            let Some((address, prefix)) = configured.split_once('/') else {
                continue;
            };
            let (Ok(address), Ok(prefix)) = (address.parse::<IpAddr>(), prefix.parse::<u8>())
            else {
                continue;
            };
            let overlap = match (pool_ip, address) {
                (IpAddr::V4(pool), IpAddr::V4(address)) if prefix <= 32 => {
                    let mask = u32::MAX
                        .checked_shl(u32::from(32 - prefix.min(pool_prefix)))
                        .unwrap_or(0);
                    u32::from(pool) & mask == u32::from(address) & mask
                }
                (IpAddr::V6(pool), IpAddr::V6(address)) if prefix <= 128 => {
                    let mask = u128::MAX
                        .checked_shl(u32::from(128 - prefix.min(pool_prefix)))
                        .unwrap_or(0);
                    u128::from(pool) & mask == u128::from(address) & mask
                }
                _ => false,
            };
            if overlap {
                return Err(
                    "[dns.fakeip] pool overlaps a TUN interface or capture exclusion CIDR".into(),
                );
            }
        }
    }
    Ok(())
}

fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

pub(crate) fn endpoint(raw: &str) -> Result<url::Url, String> {
    let raw = if raw.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()) {
        format!("udp://[{raw}]")
    } else if raw.contains("://") {
        raw.to_string()
    } else {
        format!("udp://{raw}")
    };
    let url =
        url::Url::parse(&raw).map_err(|_| "[dns.resolvers].address is not a valid endpoint")?;
    if !matches!(
        url.scheme(),
        "udp" | "tcp" | "tls" | "https" | "udp+local" | "tcp+local" | "tls+local" | "https+local"
    ) || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err("[dns.resolvers].address must be credential-free UDP/TCP/TLS/HTTPS without query/fragment".into());
    }
    if !url.scheme().starts_with("https") && !matches!(url.path(), "" | "/") {
        return Err("[dns.resolvers].address only HTTPS endpoints can have a path".into());
    }
    Ok(url)
}

pub(crate) fn bootstrap_socket(raw: &str) -> Result<SocketAddr, String> {
    let url = endpoint(raw)?;
    let ip = url.host_str().unwrap_or_default().trim_matches(['[', ']']).parse::<IpAddr>()
        .map_err(|_| "[dns].bootstrap_resolver requires a literal-IP UDP endpoint to avoid recursive bootstrap")?;
    if !matches!(url.scheme(), "udp" | "udp+local") {
        return Err("[dns].bootstrap_resolver currently supports literal-IP UDP only".into());
    }
    Ok(SocketAddr::new(ip, url.port().unwrap_or(53)))
}

pub(crate) fn cidr(value: &str) -> Result<(IpAddr, u8), String> {
    let (ip, prefix) = value
        .split_once('/')
        .ok_or("[dns.fakeip] expected an IP CIDR")?;
    let ip = ip
        .parse::<IpAddr>()
        .map_err(|_| "[dns.fakeip] invalid IP CIDR")?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| "[dns.fakeip] invalid prefix")?;
    if prefix > if ip.is_ipv4() { 32 } else { 128 } {
        return Err("[dns.fakeip] invalid prefix".into());
    }
    let canonical = match ip {
        IpAddr::V4(ip) => u32::from(ip) & u32::MAX.checked_shr(u32::from(prefix)).unwrap_or(0) == 0,
        IpAddr::V6(ip) => {
            u128::from(ip) & u128::MAX.checked_shr(u32::from(prefix)).unwrap_or(0) == 0
        }
    };
    if !canonical {
        return Err("[dns.fakeip] pool CIDRs must use their network address".into());
    }
    Ok((ip, prefix))
}
