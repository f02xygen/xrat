use crate::app::commands::output;
use crate::app::commands::progress::CliProgress;
use crate::app::context::AppContext;
use crate::app::services::runtime_control;
use crate::cli::ConnectArgs;

pub async fn run(context: &AppContext, args: &ConnectArgs) -> crate::app::Result<()> {
    let config_id = resolve_target_config_id(context, args.id.as_deref()).await?;
    let control = runtime_control::daemon_control(context);
    let progress = CliProgress::spinner(!args.json, format!("connecting config {config_id}"));
    let result = control.connect(config_id).await;
    progress.finish_and_clear();
    let outcome = result?;
    print_outcome(outcome, args.json, "daemon")
}

async fn resolve_target_config_id(
    context: &AppContext,
    target: Option<&str>,
) -> crate::app::Result<xrat_model::ConfigId> {
    let target = target.map(str::trim).filter(|s| !s.is_empty());

    match target {
        Some(id_str) if !id_str.eq_ignore_ascii_case("latest") => {
            context
                .services()
                .lifecycle
                .resolve_config_id(&xrat_model::ConfigRef::from(id_str))
                .await
        }
        _ => {
            let session = context.db.get_latest_runtime_session().await?;
            session.and_then(|s| s.config_id).ok_or_else(|| {
                crate::app::AppError::InvalidArgument(
                    "no previous connection found; provide a config ID (e.g. 'xrat connect <id>')"
                        .into(),
                )
            })
        }
    }
}

pub(super) fn print_outcome(
    outcome: crate::app::services::runtime_control::RuntimeConnectOutcome,
    json: bool,
    owner: &str,
) -> crate::app::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "connected",
                "daemon": true,
                "config": { "id": outcome.config_id },
                "session": { "id": outcome.session_id, "pid": outcome.pid },
            }))?
        );
    } else {
        println!(
            "{}",
            output::success(
                format!("Connected config {} via {owner}.", outcome.config_id),
                output::color_enabled()
            )
        );
        println!(
            "{}",
            output::format_kv(
                None,
                &[
                    ("session", outcome.session_id.to_string()),
                    ("pid", outcome.pid.to_string()),
                ],
                output::color_enabled(),
            )
        );
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::app::AppError;

    /// The daemon IPC client only exists on Unix, so the "daemon is not
    /// running" hint cannot be produced on Windows.
    #[tokio::test]
    async fn connect_returns_daemon_unreachable_hint() {
        let context = test_context("connect-daemon-hint").await;
        seed_config(&context).await;
        let err = run(
            &context,
            &ConnectArgs {
                id: Some("1".to_string()),
                json: false,
            },
        )
        .await
        .expect_err("connect should require daemon reachability");
        match err {
            AppError::InvalidArgument(message) => {
                assert!(message.contains("xrat daemon start"));
                assert!(message.contains("daemon is not running"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_target_explicit_config_id() {
        let context = test_context("connect-resolve-explicit").await;
        seed_config(&context).await;
        let id = resolve_target_config_id(&context, Some("1"))
            .await
            .expect("explicit id should resolve");
        assert_eq!(id, xrat_model::ConfigId(1));
    }

    #[tokio::test]
    async fn resolve_target_without_id_fails_when_no_sessions() {
        let context = test_context("connect-resolve-no-session").await;
        for target in [None, Some("latest"), Some("  LATEST  "), Some("   ")] {
            let err = resolve_target_config_id(&context, target)
                .await
                .expect_err("missing session must fail");
            match err {
                AppError::InvalidArgument(message) => {
                    assert!(message.contains("no previous connection found"));
                }
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn resolve_target_without_id_resolves_latest_session() {
        let context = test_context("connect-resolve-latest").await;
        seed_config(&context).await;
        record_session(&context, xrat_model::ConfigId(1)).await;

        for target in [None, Some("latest"), Some("  LATEST  "), Some("   ")] {
            let id = resolve_target_config_id(&context, target)
                .await
                .expect("latest session should resolve");
            assert_eq!(id, xrat_model::ConfigId(1));
        }
    }

    #[tokio::test]
    async fn connect_without_id_fails_without_session() {
        let context = test_context("connect-run-no-session").await;
        let err = run(
            &context,
            &ConnectArgs {
                id: None,
                json: false,
            },
        )
        .await
        .expect_err("connect without id should fail when no session exists");
        match err {
            AppError::InvalidArgument(message) => {
                assert!(message.contains("no previous connection found"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn connect_without_id_with_session_attempts_daemon() {
        let context = test_context("connect-run-latest").await;
        seed_config(&context).await;
        record_session(&context, xrat_model::ConfigId(1)).await;

        let err = run(
            &context,
            &ConnectArgs {
                id: None,
                json: false,
            },
        )
        .await
        .expect_err("connect should resolve latest and reach daemon");
        match err {
            AppError::InvalidArgument(message) => {
                assert!(message.contains("xrat daemon start"));
                assert!(message.contains("daemon is not running"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    async fn record_session(context: &AppContext, config_id: xrat_model::ConfigId) {
        context
            .db
            .insert_runtime_session(&xrat_db::RuntimeSessionInsert {
                config_id: Some(config_id),
                status: xrat_db::RuntimeSessionStatus::Stopped,
                socks_host: None,
                socks_port: None,
                http_host: None,
                http_port: None,
                shadowsocks_host: None,
                shadowsocks_port: None,
                process_id: None,
                failure_reason: None,
                started_at: None,
                stopped_at: None,
            })
            .await
            .expect("insert session should succeed");
    }

    async fn seed_config(context: &AppContext) {
        let source = xrat_db::ImportSource {
            kind: xrat_db::SourceKind::File,
            value: "seed.txt".to_string(),
            name: None,
        };
        context
            .db
            .import_nodes(
                &source,
                &[xrat_model::Node {
                    protocol: xrat_model::Protocol::Vless,
                    address: "example.com".to_string(),
                    port: 443,
                    username: None,
                    uuid: Some("uuid-123".to_string()),
                    password: None,
                    method: None,
                    network: "ws".to_string(),
                    tls: Some("tls".to_string()),
                    sni: Some("cdn.example.com".to_string()),
                    host: Some("cdn.example.com".to_string()),
                    path: Some("/socket".to_string()),
                    name: Some("seed".to_string()),
                    extensions: None,
                    raw_config: "vless://uuid-123@example.com:443?type=ws#seed".to_string(),
                }],
            )
            .await
            .expect("import should succeed");
    }

    async fn test_context(prefix: &str) -> AppContext {
        crate::app::tests::TestAppBuilder::new(prefix).build().await
    }
}
