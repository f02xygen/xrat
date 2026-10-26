use tokio::sync::mpsc;

use crate::app::context::AppContext;
use crate::tui::data::TuiData;
use crate::tui::task::{TuiTaskEvent, TuiTaskKind};
use xrat_db::ImportSource;
use xrat_model::Node;

pub enum TuiImport {
    Config {
        source: ImportSource,
        node: Box<Node>,
    },
    Subscription {
        url: String,
        name: String,
    },
}

pub fn spawn_import(
    context: AppContext,
    import: TuiImport,
    include_deleted: bool,
    task_tx: &mpsc::UnboundedSender<TuiTaskEvent>,
) {
    let kind = TuiTaskKind::Import;
    let _ = task_tx.send(TuiTaskEvent::Started { kind });
    let task_tx = task_tx.clone();
    tokio::spawn(async move {
        let event = match run_import(&context, import).await {
            Ok(message) => match TuiData::load(&context, include_deleted).await {
                Ok(data) => TuiTaskEvent::Completed {
                    kind,
                    message,
                    data: Some(data),
                },
                Err(error) => TuiTaskEvent::Failed {
                    kind,
                    error: format!("import completed but reload failed: {error}"),
                    data: None,
                },
            },
            Err(error) => TuiTaskEvent::Failed {
                kind,
                error: format!("import failed: {error}"),
                data: None,
            },
        };
        let _ = task_tx.send(event);
    });
}

async fn run_import(context: &AppContext, import: TuiImport) -> crate::app::Result<String> {
    match import {
        TuiImport::Config { source, node } => {
            crate::app::services::catalog_import::persist_nodes(
                context,
                source,
                std::slice::from_ref(node.as_ref()),
                None,
                None,
            )
            .await?;
            Ok("added 1 config".to_string())
        }
        TuiImport::Subscription { url, name } => {
            let summary = crate::app::services::catalog_import::subscription(
                context,
                &url,
                Some(&name),
                &Default::default(),
            )
            .await?;
            let display_name = context
                .db
                .get_subscription_by_id(summary.subscription_id)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.name)
                .unwrap_or(name);
            Ok(format!(
                "imported {} configs into {display_name}",
                summary.imported_configs
            ))
        }
    }
}

#[cfg(test)]
mod tests;
