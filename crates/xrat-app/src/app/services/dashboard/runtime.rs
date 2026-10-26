use super::{
    AppContext, DaemonOverview, DashboardService, RuntimeService, RuntimeStatusSnapshot, daemon,
};
use crate::app::daemon::ipc::TunStatePayload;

pub(super) struct DashboardRuntime {
    pub runtime: RuntimeStatusSnapshot,
    pub tun: TunStatePayload,
    pub daemon: DaemonOverview,
    pub config_path: String,
    pub test_stages: Vec<String>,
}

pub(crate) struct DashboardRuntimeSnapshot {
    pub runtime: RuntimeStatusSnapshot,
    pub tun_label: Option<String>,
    pub tun_active: bool,
    pub local_address: Option<String>,
    pub daemon: DaemonOverview,
    pub config_path: String,
    pub test_stages: Vec<String>,
}

impl DashboardService<'_> {
    pub(crate) async fn load_runtime(&self) -> crate::app::Result<DashboardRuntimeSnapshot> {
        let projection = load(self.context).await?;
        let tun = &projection.tun;
        let split_summary = crate::app::services::tun_control::split_summary(tun);
        let tun_label = if tun.active {
            Some(format!(
                "{} ({}) · {split_summary}",
                tun.interface.as_deref().unwrap_or("tun"),
                tun.engine,
            ))
        } else if tun.enabled {
            Some(format!("enabled (inactive) · {split_summary}"))
        } else if tun.split_mode != "all" || tun.blacklist_count > 0 || tun.whitelist_count > 0 {
            Some(format!("off · {split_summary}"))
        } else {
            None
        };
        let runtime = projection.runtime;
        let needs_local_address = matches!(
            self.context.app_config.server.host.as_str(),
            "0.0.0.0" | "::"
        ) || [
            &runtime.inbound_health.socks,
            &runtime.inbound_health.http,
            &runtime.inbound_health.shadowsocks,
        ]
        .into_iter()
        .flatten()
        .any(|health| matches!(health.endpoint.host.as_str(), "0.0.0.0" | "::"));
        let local_address = needs_local_address
            .then(|| self.local_ip.primary_ip().map(|ip| ip.to_string()))
            .flatten();
        Ok(DashboardRuntimeSnapshot {
            runtime,
            tun_label,
            tun_active: tun.active,
            local_address,
            daemon: projection.daemon,
            config_path: projection.config_path,
            test_stages: projection.test_stages,
        })
    }
}

pub(super) async fn load(context: &AppContext) -> crate::app::Result<DashboardRuntime> {
    #[cfg(any(windows, test))]
    if let Some(client) = &context.machine_client {
        let overview = client.overview().await?;
        return Ok(DashboardRuntime {
            runtime: overview.runtime,
            tun: overview.tun,
            daemon: DaemonOverview {
                running: overview.daemon_ready,
                rotation_enabled: overview.rotation_enabled,
                interval_secs: overview.rotation_interval_secs,
            },
            config_path: overview.config_path,
            test_stages: overview.test_stages,
        });
    }
    let runtime = RuntimeService::new(context).status().await?;
    let tun = crate::app::services::tun::capture_state(context, &runtime, &Default::default());
    Ok(DashboardRuntime {
        runtime,
        tun,
        daemon: daemon::load_daemon_info(context).await,
        config_path: context.runtime_paths.config_path.display().to_string(),
        test_stages: context.app_config.runtime.rotation.test_stages.clone(),
    })
}
