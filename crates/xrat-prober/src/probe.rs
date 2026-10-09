use std::path::Path;
use std::time::Duration;

use xrat_engines::singbox::{
    SingboxProbeError, SingboxProbeProcess, generate_singbox_probe_config,
};
use xrat_engines::xray::{
    XrayGenOptions, XrayProcess, XrayProcessError, generate_probe_config_with_options,
};
use xrat_model::Node;

use super::FailureKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeEngineKind {
    Xray,
    Singbox,
}

pub enum ProbeProcess {
    Xray(XrayProcess),
    Singbox(SingboxProbeProcess),
}

impl ProbeProcess {
    /// Spawn the engine-specific probe process for `node`. Configuration or
    /// process failures are returned as an already-classified
    /// `(FailureKind, reason)` pair so every probe stage reports them the same
    /// way.
    pub async fn spawn(
        node: &Node,
        local_port: u16,
        engine: ProbeEngineKind,
        binary_path: &Path,
        gen_options: &XrayGenOptions,
        startup_timeout: Duration,
    ) -> Result<Self, (FailureKind, String)> {
        let mut node = node.clone();
        if engine == ProbeEngineKind::Xray
            && let Some(server) = gen_options.bootstrap_dns
            && node.address.parse::<std::net::IpAddr>().is_err()
        {
            let original = node.address.clone();
            let strategy = gen_options
                .dns
                .as_ref()
                .map(|dns| dns.query_strategy.clone())
                .unwrap_or_default();
            let hostname = original.clone();
            let addresses = tokio::task::spawn_blocking(move || {
                xrat_support::dns::resolve_udp(
                    &hostname,
                    server,
                    strategy != "UseIPv6",
                    strategy != "UseIPv4",
                    Duration::from_secs(2),
                )
            })
            .await
            .map_err(|_| (FailureKind::Process, "bootstrap DNS task failed".into()))?
            .map_err(|_| {
                (
                    FailureKind::Process,
                    "configured bootstrap DNS returned no usable proxy address".into(),
                )
            })?;
            let address = addresses.first().ok_or_else(|| {
                (
                    FailureKind::Process,
                    "bootstrap DNS returned no usable address".into(),
                )
            })?;
            if matches!(node.tls.as_deref(), Some("tls" | "reality")) && node.sni.is_none() {
                node.sni = Some(original.clone());
            }
            if matches!(node.network.as_str(), "ws" | "httpupgrade" | "xhttp")
                && node.host.is_none()
            {
                node.host = Some(original);
            }
            node.address = address.to_string();
        }
        let mut options = gen_options.clone();
        if engine == ProbeEngineKind::Xray
            && let Some(server) = options.bootstrap_dns
        {
            for hostname in &options.bootstrap_hosts {
                let host = hostname.clone();
                let strategy = options
                    .dns
                    .as_ref()
                    .map(|dns| dns.query_strategy.clone())
                    .unwrap_or_default();
                let addresses = tokio::task::spawn_blocking(move || {
                    xrat_support::dns::resolve_udp(
                        &host,
                        server,
                        strategy != "UseIPv6",
                        strategy != "UseIPv4",
                        Duration::from_secs(2),
                    )
                })
                .await
                .map_err(|_| (FailureKind::Process, "bootstrap DNS task failed".into()))?
                .map_err(|_| {
                    (
                        FailureKind::Process,
                        "configured bootstrap DNS returned no usable resolver address".into(),
                    )
                })?;
                let address = addresses.first().ok_or_else(|| {
                    (
                        FailureKind::Process,
                        "bootstrap DNS returned no usable address".into(),
                    )
                })?;
                if let Some(dns) = &mut options.dns {
                    dns.hosts.insert(
                        hostname.clone(),
                        xrat_engines::xray::config::XrayDnsHostValue::One(address.to_string()),
                    );
                }
            }
        }
        let gen_options = &options;
        let node = &node;
        match engine {
            ProbeEngineKind::Xray => {
                let config = generate_probe_config_with_options(node, local_port, gen_options)
                    .map_err(|error| {
                        (
                            FailureKind::Process,
                            format!("Failed to generate config: {error}"),
                        )
                    })?;
                XrayProcess::spawn_with_binary(binary_path, &config, startup_timeout)
                    .await
                    .map(Self::Xray)
                    .map_err(|error| classify_xray(&error))
            }
            ProbeEngineKind::Singbox => {
                let mut config = if gen_options.singbox_dns.is_some() {
                    xrat_engines::singbox::generate_singbox_runtime_config_with_dns(
                        node,
                        vec![xrat_engines::singbox::SingboxInbound::socks(
                            "socks-in",
                            "127.0.0.1",
                            local_port,
                            None,
                        )],
                        None,
                        None,
                        gen_options.singbox_dns.as_ref(),
                    )
                } else {
                    generate_singbox_probe_config(node, local_port)
                }
                .map_err(|error| {
                    (
                        FailureKind::Process,
                        format!("Failed to generate config: {error}"),
                    )
                })?;
                if let Some(resolver) = &gen_options.bootstrap_resolver {
                    if let Some(route) = &mut config.route {
                        route.default_domain_resolver = Some(resolver.clone());
                    }
                    if let Some(outbound) = config.outbounds.first_mut() {
                        outbound["domain_resolver"] = resolver.clone().into();
                    }
                }
                SingboxProbeProcess::spawn_with_binary(
                    binary_path,
                    &config,
                    local_port,
                    startup_timeout,
                )
                .await
                .map(Self::Singbox)
                .map_err(|error| classify_singbox(&error))
            }
        }
    }

    pub fn local_port(&self) -> u16 {
        match self {
            Self::Xray(process) => process.local_port(),
            Self::Singbox(process) => process.local_port(),
        }
    }

    pub fn kill(self) -> Result<(), std::io::Error> {
        match self {
            Self::Xray(process) => process.kill(),
            Self::Singbox(process) => process.kill(),
        }
    }
}

fn classify_xray(error: &XrayProcessError) -> (FailureKind, String) {
    match error {
        XrayProcessError::SpawnError(_) => (
            FailureKind::Process,
            format!("Failed to spawn xray: {error}"),
        ),
        XrayProcessError::StartupTimeout => {
            (FailureKind::Timeout, "Xray startup timeout".to_string())
        }
        XrayProcessError::ProcessExited(stderr) => (
            FailureKind::Process,
            format!("Xray process exited unexpectedly: {stderr}"),
        ),
        XrayProcessError::PortNotReady(_) => (
            FailureKind::Process,
            format!("Xray port not ready: {error}"),
        ),
        _ => (FailureKind::Process, format!("Xray error: {error}")),
    }
}

fn classify_singbox(error: &SingboxProbeError) -> (FailureKind, String) {
    match error {
        SingboxProbeError::Spawn(_) => (
            FailureKind::Process,
            format!("Failed to spawn sing-box: {error}"),
        ),
        SingboxProbeError::ProcessExited(stderr) => (
            FailureKind::Process,
            format!("sing-box process exited unexpectedly: {stderr}"),
        ),
        SingboxProbeError::PortNotReady(_) => (
            FailureKind::Process,
            format!("sing-box port not ready: {error}"),
        ),
        _ => (FailureKind::Process, format!("sing-box error: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use xrat_model::Protocol;

    fn node(protocol: Protocol) -> Node {
        Node {
            protocol,
            address: "example.com".to_string(),
            port: 443,
            username: None,
            uuid: Some("00000000-0000-0000-0000-000000000001".to_string()),
            password: Some("secret".to_string()),
            method: None,
            network: "tcp".to_string(),
            tls: Some("tls".to_string()),
            sni: None,
            host: None,
            path: None,
            name: None,
            extensions: None,
            raw_config: "vless://00000000-0000-0000-0000-000000000001@example.com:443?security=tls"
                .to_string(),
        }
    }

    #[tokio::test]
    async fn singbox_probe_generates_config_and_reports_spawn_failure() {
        let error = ProbeProcess::spawn(
            &node(Protocol::Vless),
            1080,
            ProbeEngineKind::Singbox,
            Path::new("/definitely-not-installed/sing-box"),
            &XrayGenOptions::default(),
            Duration::from_millis(100),
        )
        .await
        .err()
        .expect("missing sing-box binary must fail to spawn");

        assert!(matches!(error.0, FailureKind::Process));
        assert!(error.1.contains("Failed to spawn sing-box"));
    }

    #[tokio::test]
    async fn singbox_probe_rejects_unrepresentable_config_before_spawning() {
        let mut invalid = node(Protocol::Vless);
        invalid.uuid = None;
        let error = ProbeProcess::spawn(
            &invalid,
            1080,
            ProbeEngineKind::Singbox,
            Path::new("/definitely-not-installed/sing-box"),
            &XrayGenOptions::default(),
            Duration::from_millis(100),
        )
        .await
        .err()
        .expect("invalid node must fail config generation");

        assert!(error.1.contains("Failed to generate config"));
        assert!(error.1.contains("VLESS requires UUID"));
    }

    #[test]
    fn classifies_singbox_process_errors() {
        let (kind, reason) =
            classify_singbox(&SingboxProbeError::ProcessExited("boom".to_string()));
        assert!(matches!(kind, FailureKind::Process));
        assert!(reason.contains("sing-box process exited unexpectedly: boom"));
    }
}
