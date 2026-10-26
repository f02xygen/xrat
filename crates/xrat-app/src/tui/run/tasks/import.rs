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
        name: Option<String>,
        fallback_name: String,
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
        TuiImport::Subscription {
            url,
            name,
            fallback_name,
        } => {
            let summary = crate::app::services::catalog_import::subscription_with_fallback(
                context,
                &url,
                name.as_deref(),
                Some(&fallback_name),
                &Default::default(),
            )
            .await?;
            let display_name = context
                .services()
                .lifecycle
                .subscription(&summary.subscription_id.to_string())
                .await
                .ok()
                .and_then(|s| s.name)
                .unwrap_or_else(|| name.unwrap_or(fallback_name));
            Ok(format!(
                "imported {} configs into {display_name}",
                summary.imported_configs
            ))
        }
    }
}

#[cfg(test)]
mod tests;
