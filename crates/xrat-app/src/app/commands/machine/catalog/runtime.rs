use crate::app::Result;
use crate::app::commands::{connect, disconnect, progress::CliProgress, status};
use crate::app::services::machine_service::{GatewayPayload, GatewayRequestKind, MachineClient};
use crate::app::services::runtime_control::{MachineRuntimeControl, RuntimeControl};
use crate::cli::{ConnectArgs, DisconnectArgs, StatusArgs};
use xrat_model::ConfigRef;

pub(super) async fn connect(client: &MachineClient, args: &ConnectArgs) -> Result<()> {
    let target = args.id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let id = match target {
        Some(target) if !target.eq_ignore_ascii_case("latest") => {
            client.resolve_config(&ConfigRef::from(target)).await?
        }
        _ => {
            let overview = client.overview().await?;
            overview
                .runtime
                .session
                .and_then(|session| session.config_id)
                .ok_or_else(|| {
                    crate::app::AppError::InvalidArgument(
                        "no previous connection found; provide a config ID (e.g. 'xrat connect <id>')".into()
                    )
                })?
        }
    };
    let control = MachineRuntimeControl::new(client.clone());
    let progress = CliProgress::spinner(!args.json, format!("connecting config {id}"));
    let result = control.connect(id).await;
    progress.finish_and_clear();
    connect::print_outcome(result?, args.json, "machine service")
}

pub(super) async fn disconnect(client: &MachineClient, args: &DisconnectArgs) -> Result<()> {
    let control = MachineRuntimeControl::new(client.clone());
    let progress = CliProgress::spinner(!args.json, "disconnecting runtime");
    let result = control.disconnect().await;
    progress.finish_and_clear();
    disconnect::print_outcome(result?, args.json)
}

pub(super) async fn status(client: &MachineClient, args: &StatusArgs) -> Result<()> {
    let GatewayPayload::Status(payload) = client.call(GatewayRequestKind::Status {}).await? else {
        return Err(super::entry::unexpected());
    };
    let reference = match payload.active_config_id {
        Some(id) => client.config_reference(id).await?,
        None => None,
    };
    status::print_payload(payload, reference, args.json, "machine service")
}
