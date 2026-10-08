use super::fake_runtime::write_fake_runtime_script;
use super::support::*;
use super::*;

#[tokio::test]
async fn replace_success_validates_new_then_stops_old_runtime() {
    let mut context = test_context().await;
    let (active_config, next_config) = import_two_configs(&context, "old", "new").await;

    write_fake_runtime_script(&context);
    context.runtime_paths.xray_path = context.runtime_paths.root_dir.join("fake-xray.py");

    let mut old = spawn_sleep(30);
    let old_pid = i64::from(old.id());

    let old_session_id = set_running_session(&context, active_config.id, old_pid).await;

    let result = RuntimeService::new(&context)
        .replace(ReplaceRequest {
            trigger: RotationTrigger::Manual,
            candidate_id: Some(next_config.id),
        })
        .await
        .expect("replace should succeed");

    assert_eq!(result.old_session_id, Some(old_session_id));
    assert_ne!(result.new_session_id, old_session_id);

    let mut old_stopped = false;
    for _ in 0..20 {
        if old
            .try_wait()
            .expect("old process wait should succeed")
            .is_some()
        {
            old_stopped = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(old_stopped, "old runtime process should be stopped");

    let running = context
        .db
        .get_running_runtime_session()
        .await
        .expect("running should load")
        .expect("new running session should exist");
    assert_eq!(running.id, result.new_session_id);
    assert_eq!(running.status, RuntimeSessionStatus::Running);
    assert_eq!(running.config_id, Some(next_config.id));
    assert_eq!(
        running.socks_port,
        Some(i64::from(context.app_config.runtime.socks.port))
    );
    assert_eq!(
        running.last_transition_reason_code.as_deref(),
        Some("replace_commit_success")
    );
    assert_eq!(
        running.last_transition_reason_detail.as_deref(),
        Some("runtime replace handoff completed")
    );
    assert_eq!(running.last_transition_origin.as_deref(), Some("daemon"));

    let active_config = context
        .db
        .get_active_config()
        .await
        .expect("active config should load")
        .expect("active config should exist");
    assert_eq!(active_config.id, next_config.id);

    let _ = xray_runtime::terminate_process_gracefully(result.new_pid as i64, SHUTDOWN_TIMEOUT);
    let _ = old.kill();
    let _ = old.wait();
}

#[tokio::test]
async fn replace_preserves_old_runtime_when_tun_bootstrap_resolution_fails() {
    let mut context = test_context().await;
    context.app_config.runtime.engine = "xray".to_string();
    context.app_config.runtime.tun.enabled = true;
    let (active_config, next_config) = import_two_configs(&context, "old", "new").await;

    write_fake_runtime_script(&context);
    context.runtime_paths.xray_path = context.runtime_paths.root_dir.join("fake-xray.py");

    let mut old = spawn_sleep(30);
    let old_pid = i64::from(old.id());

    let old_session_id = set_running_session(&context, active_config.id, old_pid).await;

    struct FailingHostResolver;
    impl xrat_support::net::HostResolver for FailingHostResolver {
        fn resolve(&self, _host: &str, _port: u16) -> Option<std::net::IpAddr> {
            None
        }
    }

    let ports = xrat_support::readiness::RuntimeProcessPorts {
        resolver: std::sync::Arc::new(FailingHostResolver),
        ..Default::default()
    };
    let service = RuntimeService::with_process_ports(&context, ports);

    let result = service
        .replace(ReplaceRequest {
            trigger: RotationTrigger::Manual,
            candidate_id: Some(next_config.id),
        })
        .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("failed to resolve proxy endpoint"),
        "unexpected error: {err_msg}"
    );

    assert!(
        old.try_wait().expect("wait should succeed").is_none(),
        "old runtime process should still be running"
    );

    let running = context
        .db
        .get_running_runtime_session()
        .await
        .expect("running should load")
        .expect("running session should exist");
    assert_eq!(running.id, old_session_id);
    assert_eq!(running.status, RuntimeSessionStatus::Running);

    let active = context
        .db
        .get_active_config()
        .await
        .expect("active config should load")
        .expect("active config should exist");
    assert_eq!(active.id, active_config.id);

    let _ = old.kill();
    let _ = old.wait();
}
