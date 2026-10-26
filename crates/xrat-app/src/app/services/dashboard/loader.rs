use super::*;

#[derive(Debug, Clone, Default)]
pub struct DaemonOverview {
    pub running: bool,
    pub rotation_enabled: bool,
    pub interval_secs: u64,
}

#[derive(Debug)]
pub struct DashboardSnapshot {
    pub configs: Vec<ConfigDetail>,
    pub sources: Vec<SubscriptionRecord>,
    pub runtime: RuntimeStatusSnapshot,
    pub tun_label: Option<String>,
    pub tun_active: bool,
    pub local_address: Option<String>,
    pub latest_run: Option<ConnectionTestRunRecord>,
    pub test_results: Vec<ConnectionTestRecord>,
    pub logs: DashboardLogs,
    pub probe_history: Vec<ConnectionTestRecord>,
    pub daemon: DaemonOverview,
    pub db_label: String,
    pub config_path: String,
    pub api_b64_url: String,
    pub server_enabled: bool,
    pub test_stages: Vec<String>,
    pub test_concurrency_managed: bool,
    pub pending_enrichment: Vec<(ConfigId, String)>,
}

pub struct DashboardService<'a> {
    pub(super) context: &'a AppContext,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) local_ip: Arc<dyn xrat_support::net::LocalIpResolver>,
}

impl<'a> DashboardService<'a> {
    pub fn new(context: &'a AppContext) -> Self {
        Self::with_clock(context, Arc::new(SystemClock))
    }

    pub fn with_clock(context: &'a AppContext, clock: Arc<dyn Clock>) -> Self {
        Self::with_dependencies(
            context,
            clock,
            Arc::new(xrat_support::net::SystemLocalIpResolver),
        )
    }

    pub fn with_dependencies(
        context: &'a AppContext,
        clock: Arc<dyn Clock>,
        local_ip: Arc<dyn xrat_support::net::LocalIpResolver>,
    ) -> Self {
        Self {
            context,
            clock,
            local_ip,
        }
    }

    pub async fn clear_events(&self) -> crate::app::Result<u64> {
        use crate::app::services::diagnostics::{
            DiagnosticsPayload, DiagnosticsRequest, DiagnosticsService,
        };
        match DiagnosticsService::new(self.context.clone())
            .dispatch(DiagnosticsRequest::ClearEvents {})
            .await?
        {
            DiagnosticsPayload::Cleared(count) => Ok(count),
            _ => Err(crate::app::AppError::InvalidArgument(
                "unexpected event clear result".into(),
            )),
        }
    }

    pub async fn load(&self, include_deleted: bool) -> crate::app::Result<DashboardSnapshot> {
        let context = self.context;
        let services = context.services();
        let request = super::super::ConfigListRequest {
            include_deleted,
            ..Default::default()
        };
        let mut configs = services.configs.list(&request).await?.items;
        configs.sort_by_key(|row| {
            (
                row.summary.test().real_delay_ms.unwrap_or(i64::MAX),
                row.summary.id,
            )
        });
        let pending_enrichment =
            geo::apply_geo_cache(context, &mut configs, self.clock.as_ref()).await;
        let sources = services.configs.subscriptions().await?;
        let projection = self.load_runtime().await?;
        let runtime = projection.runtime;
        let tun_label = projection.tun_label;
        let tun_active = projection.tun_active;
        let local_address = projection.local_address;
        let server = &context.app_config.server;
        let diagnostics =
            crate::app::services::diagnostics::DiagnosticsService::new(context.clone());
        let latest_run = diagnostics.latest_probe_run().await?;
        let test_results = match &latest_run {
            Some(run) => {
                diagnostics
                    .probe_history(xrat_db::ConnectionTestSelection::Run(run.id), None)
                    .await?
            }
            None => Vec::new(),
        };
        let logs = DashboardLogs::load(context).await?;
        let probe_history = match runtime.active_config.as_ref() {
            Some(config) => {
                diagnostics
                    .probe_history(xrat_db::ConnectionTestSelection::Config(config.id), None)
                    .await?
            }
            None => Vec::new(),
        };
        let db_label = runtime.database_label.clone();
        Ok(DashboardSnapshot {
            configs,
            sources,
            runtime,
            tun_label,
            tun_active,
            latest_run,
            test_results,
            logs,
            probe_history,
            daemon: projection.daemon,
            db_label,
            config_path: projection.config_path,
            api_b64_url: format!(
                "http://{}:{}/b64",
                display_host(&server.host, local_address.as_deref()),
                server.port
            ),
            local_address,
            server_enabled: server.enabled,
            test_stages: projection.test_stages,
            test_concurrency_managed: self.context.is_machine_client(),
            pending_enrichment,
        })
    }
}

pub(super) fn display_host(host: &str, local_address: Option<&str>) -> String {
    match host {
        "0.0.0.0" | "::" => local_address
            .map(str::to_string)
            .unwrap_or_else(|| xrat_support::net::connect_host_for_bind_host(host)),
        _ => host.to_string(),
    }
}
