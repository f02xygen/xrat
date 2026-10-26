use super::super::MachineGateway;
use super::*;
use crate::app::context::AppContext;
use crate::app::tests::TestAppBuilder;
use std::collections::BTreeMap;
use std::sync::Mutex;
use xrat_db::{ImportSource, SourceKind};

const OPERATOR: &str = "S-1-5-21-1-2-3-1001";

struct Loopback {
    gateway: MachineGateway,
    calls: Arc<Mutex<Vec<GatewayRequest>>>,
    page_limit: usize,
    fail_at_offset: Option<i64>,
}

#[async_trait::async_trait]
impl Transport for Loopback {
    async fn exchange(&self, request: GatewayRequest) -> Result<DaemonResponse<GatewayPayload>> {
        self.calls.lock().unwrap().push(request.clone());
        let page = match &request.request {
            GatewayRequestKind::Catalog {
                request: CatalogRequest::List { filter } | CatalogRequest::ExportPage { filter },
            } => Some((
                filter.offset.unwrap_or_default(),
                filter.limit.unwrap_or_default() as usize,
            )),
            GatewayRequestKind::Catalog {
                request: CatalogRequest::Subscriptions { offset, limit },
            } => Some((*offset as i64, *limit as usize)),
            GatewayRequestKind::Catalog {
                request: CatalogRequest::RawConfigs { ids },
            } => Some((ids.first().map_or(0, |id| id.0 - 1), ids.len())),
            GatewayRequestKind::Diagnostics {
                request:
                    crate::app::services::diagnostics::DiagnosticsRequest::ProbePage {
                        before,
                        limit,
                        ..
                    },
            } => Some((
                before.as_ref().map_or(0, |cursor| cursor.id),
                usize::from(*limit),
            )),
            GatewayRequestKind::ProbeResults { offset, limit, .. } => {
                Some((i64::from(*offset), usize::from(*limit)))
            }
            _ => None,
        };
        if let Some((offset, limit)) = page {
            if self.fail_at_offset == Some(offset) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "machine pipe disconnected",
                )
                .into());
            }
            if limit > self.page_limit {
                return Ok(DaemonResponse {
                    protocol_version: GATEWAY_VERSION,
                    ok: false,
                    code: DaemonResponseCode::MessageTooLarge,
                    message: "reply too large".into(),
                    payload: None,
                });
            }
        }
        let decoded = serde_json::from_slice(&serde_json::to_vec(&request)?)?;
        let response = self.gateway.dispatch(OPERATOR, decoded).await?;
        Ok(serde_json::from_slice(&serde_json::to_vec(&response)?)?)
    }
}

async fn fixture(
    fail_at_offset: Option<i64>,
) -> (
    AppContext,
    MachineClient,
    Arc<Mutex<Vec<GatewayRequest>>>,
    tempfile::TempDir,
) {
    let (mut context, root) = TestAppBuilder::new("machine-client")
        .build_with_root()
        .await;
    context.app_config.testing.geoip.enabled = false;
    let (sender, _receiver) = tokio::sync::mpsc::channel(4);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let client = MachineClient {
        transport: Arc::new(Loopback {
            gateway: MachineGateway::for_test(context.clone(), OPERATOR, sender),
            calls: calls.clone(),
            page_limit: 2,
            fail_at_offset,
        }),
    };
    (context, client, calls, root)
}

async fn import(client: &MachineClient, index: u32) {
    let content = format!(
        "vless://00000000-0000-0000-0000-{index:012}@node{index}.example:443?security=tls#node{index}"
    );
    client
        .catalog(CatalogRequest::Import {
            contents: vec![content.clone()],
            source: ImportSource {
                kind: SourceKind::RawText,
                value: content,
                name: None,
            },
            name: None,
            headers: BTreeMap::new(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn client_aggregates_smaller_pages_without_losing_filters_or_export_matches() {
    let (context, client, calls, _root) = fixture(None).await;
    for index in 1..=5 {
        import(&client, index).await;
    }
    let expected = context
        .services()
        .configs
        .list(&Default::default())
        .await
        .unwrap();
    let actual = client.list(&Default::default()).await.unwrap();
    assert_eq!(actual.total, 5);
    assert_eq!(actual.items, expected.items);
    let filtered = ConfigListRequest {
        only_enabled: true,
        offset: Some(1),
        limit: Some(2),
        ..Default::default()
    };
    let local = context.services().configs.list(&filtered).await.unwrap();
    let remote = client.list(&filtered).await.unwrap();
    assert_eq!(local.total, remote.total);
    assert_eq!(local.items, remote.items);
    assert_eq!(
        client.export(&filtered).await.unwrap(),
        context
            .services()
            .configs
            .export_raw_configs(&filtered)
            .await
            .unwrap()
    );
    assert_eq!(
        client.subscriptions().await.unwrap(),
        context.services().configs.subscriptions().await.unwrap()
    );
    assert!(calls.lock().unwrap().iter().any(|request| matches!(&request.request,GatewayRequestKind::Catalog {request:CatalogRequest::List {filter}} if filter.limit==Some(1))));
    let empty = client
        .list(&ConfigListRequest {
            only_active: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.items.is_empty());
}

#[tokio::test]
async fn client_resolves_references_and_preserves_zero_limit_semantics() {
    let (context, client, _calls, _root) = fixture(None).await;
    import(&client, 1).await;
    let record = context
        .db
        .list_configs(&Default::default())
        .await
        .unwrap()
        .remove(0);
    let reference = ConfigRef::from(record.r#ref.as_str());
    assert_eq!(client.resolve_config(&reference).await.unwrap(), record.id);
    assert_eq!(client.config(&reference).await.unwrap(), record);
    let result = client
        .list(&ConfigListRequest {
            offset: Some(0),
            limit: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(result.total, 1);
    assert!(result.items.is_empty());
}

#[tokio::test]
async fn disconnected_later_page_returns_an_error_instead_of_partial_success() {
    let (_context, client, _calls, _root) = fixture(Some(1)).await;
    for index in 1..=4 {
        import(&client, index).await;
    }
    let error = client.list(&Default::default()).await.unwrap_err();
    assert!(error.to_string().contains("pipe disconnected"));
    assert!(error.to_string().contains("user profile was not opened"));
}

struct RejectedReply {
    version: u16,
    code: DaemonResponseCode,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl Transport for RejectedReply {
    async fn exchange(&self, _: GatewayRequest) -> Result<DaemonResponse<GatewayPayload>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(DaemonResponse {
            protocol_version: self.version,
            ok: false,
            code: self.code.clone(),
            message: "operation completed but reply too large".into(),
            payload: None,
        })
    }
}

#[tokio::test]
async fn mutation_is_not_replayed_after_a_lost_oversized_result_and_versions_are_checked() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let client = MachineClient {
        transport: Arc::new(RejectedReply {
            version: GATEWAY_VERSION,
            code: DaemonResponseCode::MessageTooLarge,
            calls: calls.clone(),
        }),
    };
    assert!(
        client
            .catalog(CatalogRequest::Delete {
                reference: ConfigRef::from("1"),
                hard: true
            })
            .await
            .is_err()
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let client = MachineClient {
        transport: Arc::new(RejectedReply {
            version: GATEWAY_VERSION + 1,
            code: DaemonResponseCode::InvalidState,
            calls,
        }),
    };
    assert!(
        client
            .catalog(CatalogRequest::ResolveConfig {
                reference: ConfigRef::from("1")
            })
            .await
            .unwrap_err()
            .to_string()
            .contains("protocol mismatch")
    );
}

#[tokio::test]
async fn machine_refresh_fetches_as_client_and_preserves_headers_names_and_partial_failures() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let worker = tokio::spawn(async move {
        for index in 0..4 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let read = socket.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
                assert!(request.len() < 65536);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("x-subscription-key: fixture-secret"));
            let (status, body) = if index == 3 {
                ("503 Service Unavailable", "unavailable".to_string())
            } else {
                (
                    "200 OK",
                    format!("vless://id@node{index}.example:443#node{index}"),
                )
            };
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    });
    let (context, client, calls, _root) = fixture(None).await;
    let headers = BTreeMap::from([("x-subscription-key".into(), "fixture-secret".into())]);
    let first = client
        .import_subscription(&format!("http://{address}/one"), Some("main"), &headers)
        .await
        .unwrap();
    let second = client
        .import_subscription(&format!("http://{address}/two"), Some("backup"), &headers)
        .await
        .unwrap();
    let summary = client
        .refresh_subscriptions(&[
            second.subscription_id.to_string(),
            first.subscription_id.to_string(),
            first.subscription_id.to_string(),
        ])
        .await
        .unwrap();
    assert_eq!(
        (summary.attempted, summary.succeeded, summary.failed),
        (2, 1, 1)
    );
    assert_eq!(summary.successes[0].subscription_id, first.subscription_id);
    assert_eq!(summary.failures[0].subscription_id, second.subscription_id);
    assert_eq!(summary.successes[0].subscription_name, "main");
    assert_eq!(
        client
            .subscription(&first.subscription_id.to_string())
            .await
            .unwrap()
            .headers,
        headers
    );
    let configs = context.db.list_configs(&Default::default()).await.unwrap();
    assert_eq!(configs.len(), 2);
    assert!(configs.iter().any(|row| row.address == "node1.example"));
    assert!(configs.iter().any(|row| row.address == "node2.example"));
    worker.await.unwrap();
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(
                request.request,
                GatewayRequestKind::Catalog {
                    request: CatalogRequest::Import { .. }
                }
            ))
            .count(),
        3
    );
}

#[tokio::test]
async fn application_facades_preserve_shared_reads_lifecycle_and_ref_resolution() {
    use crate::app::ports::ConfigRepository;
    use crate::app::services::{AppServices, ConfigExportRequest};
    let (context, client, _calls, _root) = fixture(None).await;
    for index in 1..=3 {
        import(&client, index).await;
    }
    let local = context.services();
    let remote = AppServices::from_machine(client);
    let configs = local.configs.list(&Default::default()).await.unwrap();
    assert_eq!(
        remote
            .configs
            .list(&Default::default())
            .await
            .unwrap()
            .items,
        configs.items
    );
    let id = configs.items[0].summary.id;
    let reference = remote.configs.reference(id).await.unwrap().unwrap();
    let reference = ConfigRef::from(&reference[..8]);
    assert_eq!(
        remote.configs.detail(id).await.unwrap(),
        local.configs.detail(id).await.unwrap()
    );
    assert_eq!(
        remote.configs.resolve_id(&reference).await.unwrap(),
        Some(id)
    );
    assert_eq!(
        remote
            .lifecycle
            .resolve_config_id(&reference)
            .await
            .unwrap(),
        id
    );
    for missing in ["", "not-a-reference", "999999999"] {
        let reference = ConfigRef::from(missing);
        assert_eq!(remote.configs.resolve_id(&reference).await.unwrap(), None);
        assert!(
            remote
                .lifecycle
                .resolve_config_id(&reference)
                .await
                .is_err()
        );
        assert_eq!(
            remote
                .configs
                .resolve_subscription_id(missing)
                .await
                .unwrap(),
            None
        );
    }
    assert_eq!(
        remote.configs.detail(ConfigId(i64::MAX)).await.unwrap(),
        None
    );
    let outcome = remote.lifecycle.disable(&reference).await.unwrap();
    assert_eq!(outcome, crate::app::services::ToggleOutcome::Changed);
    assert_eq!(
        remote.lifecycle.disable(&reference).await.unwrap(),
        crate::app::services::ToggleOutcome::AlreadyDisabled
    );
    for filter in [
        ConfigListRequest::default(),
        ConfigListRequest {
            only_enabled: true,
            offset: Some(1),
            limit: Some(1),
            ..Default::default()
        },
    ] {
        let expected = local.configs.list(&filter).await.unwrap();
        let actual = remote.configs.list(&filter).await.unwrap();
        assert_eq!(actual.total, expected.total);
        assert_eq!(actual.items, expected.items);
    }
    let export = ConfigExportRequest::default();
    assert_eq!(
        remote.configs.export_subscription(&export).await.unwrap(),
        local.configs.export_subscription(&export).await.unwrap()
    );
    assert_eq!(
        remote.configs.export_summaries(&export).await.unwrap(),
        local.configs.export_summaries(&export).await.unwrap()
    );
    let subscriptions = remote.configs.subscriptions().await.unwrap();
    assert_eq!(subscriptions, local.configs.subscriptions().await.unwrap());
    let source = &subscriptions[0];
    assert_eq!(
        remote
            .configs
            .resolve_subscription_id(&source.r#ref)
            .await
            .unwrap(),
        Some(source.id)
    );
    assert_eq!(
        remote
            .lifecycle
            .resolve_subscription_id(&source.r#ref)
            .await
            .unwrap(),
        source.id
    );
    assert_eq!(
        remote.lifecycle.subscription(&source.r#ref).await.unwrap(),
        *source
    );
    assert_eq!(
        remote
            .lifecycle
            .rename_subscription(&source.r#ref, "renamed")
            .await
            .unwrap(),
        *source
    );
    assert_eq!(
        remote
            .lifecycle
            .subscription(&source.r#ref)
            .await
            .unwrap()
            .name
            .as_deref(),
        Some("renamed")
    );
    remote.lifecycle.enable(&reference).await.unwrap();
    remote.lifecycle.delete(&reference, false).await.unwrap();
    assert_eq!(
        remote.lifecycle.enable(&reference).await.unwrap(),
        crate::app::services::ToggleOutcome::DeletedConfig
    );
    assert_eq!(remote.lifecycle.count_deleted().await.unwrap(), 1);
    remote.lifecycle.restore(&reference).await.unwrap();
    assert_eq!(remote.lifecycle.count_deleted().await.unwrap(), 0);
    assert_eq!(remote.lifecycle.delete_many(&[id], false).await.unwrap(), 1);
    assert_eq!(remote.lifecycle.restore_many(&[id]).await.unwrap(), 1);
    remote.lifecycle.delete(&reference, true).await.unwrap();
    assert_eq!(remote.configs.detail(id).await.unwrap(), None);
    assert_eq!(remote.configs.reference(id).await.unwrap(), None);
    remote
        .lifecycle
        .delete_subscription(&source.r#ref)
        .await
        .unwrap();
    assert_eq!(
        remote
            .configs
            .resolve_subscription_id(&source.r#ref)
            .await
            .unwrap(),
        None
    );
    assert_eq!(remote.lifecycle.purge().await.unwrap(), 0);
    let repository = remote.configs.repository();
    let error = repository
        .list_configs(&Default::default())
        .await
        .unwrap_err();
    assert!(
        matches!(error, xrat_db::DbError::Io(error) if error.kind() == std::io::ErrorKind::Unsupported)
    );
    assert!(repository.purge_deleted_configs().await.is_err());
}

#[tokio::test]
async fn http_exports_use_machine_catalog_while_api_auth_and_ui_store_remain_local() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use base64::Engine;
    use tower::ServiceExt;
    let (machine, client, calls, _machine_root) = fixture(None).await;
    import(&client, 1).await;
    import(&client, 2).await;
    let disabled = machine
        .db
        .list_configs(&Default::default())
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.address == "node2.example")
        .unwrap();
    machine
        .services()
        .lifecycle
        .disable(&ConfigRef::from(disabled.r#ref.as_str()))
        .await
        .unwrap();
    let source = machine
        .services()
        .configs
        .subscriptions()
        .await
        .unwrap()
        .into_iter()
        .find(|row| Some(row.id) == disabled.subscription_id)
        .unwrap();
    let ui = TestAppBuilder::new("machine-http-user-store").build().await;
    let mut state =
        crate::server::ServerState::for_test(ui.db.clone(), Some("operator-secret".into()));
    state.services = crate::app::services::AppServices::from_machine(client);
    let router = crate::server::build_router(state);
    let count = calls.lock().unwrap().len();
    for uri in ["/b64", "/b64?key=incorrect"] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        calls.lock().unwrap().len(),
        count,
        "HTTP auth must precede gateway access"
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/b64?key=operator-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(body)
        .unwrap();
    let expected = machine
        .services()
        .configs
        .export_subscription(&crate::app::services::ConfigExportRequest::default())
        .await
        .unwrap();
    assert_eq!(raw, expected.as_bytes());
    for uri in [
        format!(
            "/b64?key=operator-secret&enabled=false&state=all&subscription={}",
            source.r#ref
        ),
        "/b64?key=operator-secret&enabled=false&state=all&q=node2".into(),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();
        assert_eq!(raw, disabled.raw_config.as_bytes());
    }
    assert!(
        ui.db
            .list_configs(&Default::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn raw_id_exports_preserve_order_missing_semantics_and_bounded_read_retries() {
    let (context, client, calls, _root) = fixture(None).await;
    for index in 1..=5 {
        import(&client, index).await;
    }
    let remote = crate::app::services::AppServices::from_machine(client);
    let ids = vec![
        ConfigId(5),
        ConfigId(1),
        ConfigId(3),
        ConfigId(3),
        ConfigId(2),
    ];
    assert_eq!(
        remote.configs.raw_configs_by_ids(&ids).await.unwrap(),
        context
            .services()
            .configs
            .raw_configs_by_ids(&ids)
            .await
            .unwrap()
    );
    assert_eq!(
        remote
            .configs
            .raw_configs_by_ids(&[ConfigId(1), ConfigId(999)])
            .await
            .unwrap(),
        None
    );
    let before = calls.lock().unwrap().len();
    assert_eq!(
        remote.configs.raw_configs_by_ids(&[]).await.unwrap(),
        Some(Vec::new())
    );
    assert_eq!(before, calls.lock().unwrap().len());
    assert!(calls.lock().unwrap().iter().any(|request| matches!(&request.request, GatewayRequestKind::Catalog { request: CatalogRequest::RawConfigs { ids } } if ids.len() == 1)));
    let (_context, client, _, _root) = fixture(Some(2)).await;
    for index in 1..=5 {
        import(&client, index).await;
    }
    let remote = crate::app::services::AppServices::from_machine(client);
    let error = remote
        .configs
        .raw_configs_by_ids(&[ConfigId(1), ConfigId(2), ConfigId(3), ConfigId(4)])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("pipe disconnected"));
}

mod history;

#[tokio::test]
async fn machine_probe_client_preserves_job_identity_and_optional_status_over_the_wire() {
    use crate::app::services::machine_service::{ProbeStages, ProbeState};
    let (_context, client, calls, _root) = fixture(None).await;
    assert!(client.probe_status(None).await.unwrap().is_none());
    let start = client
        .start_probe(vec![ConfigId(i64::MAX)], ProbeStages::default())
        .await
        .unwrap();
    let finished = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = client
                .probe_status(Some(start.job_id.clone()))
                .await
                .unwrap()
                .unwrap();
            if snapshot.state == ProbeState::Completed {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        (finished.completed, finished.total, finished.run_id),
        (0, 0, None)
    );
    assert_eq!(
        client.cancel_probe(start.job_id.clone()).await.unwrap(),
        Some(finished.clone())
    );
    assert_eq!(client.probe_status(None).await.unwrap(), Some(finished));
    assert!(
        client
            .probe_status(Some(uuid::Uuid::new_v4().to_string()))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| matches!(
                call.request,
                GatewayRequestKind::Probes {
                    request: crate::app::services::machine_service::ProbeRequest::Start { .. }
                }
            ))
            .count(),
        1
    );
}

mod probes;

#[tokio::test]
async fn machine_import_naming_preserves_fallback_explicit_names_and_refresh() {
    let (context, client, _calls, _root) = fixture(None).await;
    let (url, server) = crate::app::tests::fixtures::named_subscription_url(3).await;
    let summary = client
        .import_subscription_with_fallback(&url, None, Some("UrlProvider"), &BTreeMap::new())
        .await
        .unwrap();
    assert_eq!(
        context
            .db
            .get_subscription_by_id(summary.subscription_id)
            .await
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("ServerProvider")
    );
    client
        .import_subscription(&url, Some("sub-work"), &BTreeMap::new())
        .await
        .unwrap();
    client.refresh_subscriptions(&[]).await.unwrap();
    let stored = context.db.list_subscriptions().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name.as_deref(), Some("sub-work"));
    server.await.unwrap();
}
