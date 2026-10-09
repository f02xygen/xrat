use crate::app::config::TunSettings;

pub(crate) fn validate(tun: &TunSettings, engine: &str) -> Result<(), String> {
    if !tun.enabled {
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err("[runtime.tun] managed capture is Linux-only".into());
    }
    if !matches!(engine, "xray" | "sing-box") {
        return Err("[runtime.tun] requires Xray or sing-box; V2Ray TUN is unsupported".into());
    }
    if tun.interface_name.is_empty()
        || tun.interface_name.len() > 15
        || tun
            .interface_name
            .chars()
            .any(|value| value.is_whitespace() || matches!(value, '/' | '\0'))
    {
        return Err(
            "[runtime.tun].interface_name requires 1..15 bytes without whitespace, slash or NUL"
                .into(),
        );
    }
    if !(1280..=65535).contains(&tun.mtu) {
        return Err("[runtime.tun].mtu must be 1280..65535".into());
    }
    if tun.address.is_empty() {
        return Err("[runtime.tun].address requires at least one interface CIDR".into());
    }
    for cidr in tun.address.iter().chain(&tun.route_exclude_address) {
        let parsed = cidr.split_once('/').and_then(|(address, prefix)| {
            Some((
                address.parse::<std::net::IpAddr>().ok()?,
                prefix.parse::<u8>().ok()?,
            ))
        });
        if parsed.is_none_or(|(address, prefix)| prefix > if address.is_ipv4() { 32 } else { 128 })
        {
            return Err(
                "[runtime.tun] addresses and exclusions must be valid IPv4/IPv6 CIDRs".into(),
            );
        }
    }
    if !matches!(tun.stack.as_str(), "system" | "gvisor" | "mixed") {
        return Err("[runtime.tun].stack must be system, gvisor or mixed".into());
    }
    if engine == "xray"
        && (tun.stack != "system" || tun.strict_route || !tun.route_exclude_address.is_empty())
    {
        return Err("[runtime.tun] stack selection, strict_route and route_exclude_address are sing-box-only; Xray uses its fixed native stack".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn rejects_ignored_xray_options_and_invalid_interface_names_before_launch() {
        let mut tun = TunSettings {
            enabled: true,
            ..Default::default()
        };
        assert!(validate(&tun, "xray").is_ok());
        tun.strict_route = true;
        assert!(validate(&tun, "xray").is_err());
        assert!(validate(&tun, "sing-box").is_ok());
        tun.strict_route = false;
        tun.stack = "gvisor".into();
        assert!(validate(&tun, "xray").is_err());
        tun.interface_name = "interface-name-too-long".into();
        assert!(validate(&tun, "sing-box").is_err());
        tun.enabled = false;
        assert!(validate(&tun, "v2ray").is_ok());
    }
}
