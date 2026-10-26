use super::*;
use xrat_web::types::{Action, Job};

#[tokio::test]
async fn http_import_and_refresh_preserve_shared_naming_rules() {
    let (router, state) = fixture().await;
    let context = state.web.as_ref().unwrap().context.as_ref().unwrap();
    std::fs::write(&context.runtime_paths.config_path, "").unwrap();
    let (url, server) = crate::app::tests::fixtures::named_subscription_url(3).await;
    let (cookie, me) = login(&router, "admin", "correct horse battery").await;
    for (action, expected) in [
        (
            Action::Import {
                input: url.clone(),
                name: None,
            },
            "ServerProvider",
        ),
        (
            Action::Import {
                input: url,
                name: Some("sub-work".into()),
            },
            "sub-work",
        ),
        (Action::Refresh { id: None }, "sub-work"),
    ] {
        let response = request(
            &router,
            "/api/v1/actions",
            "POST",
            serde_json::to_value(action).unwrap(),
            &cookie,
            &me.csrf_token,
            "http://127.0.0.1:18203",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let job: Job =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let current = state
                    .db
                    .web_jobs()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|record| record.id == job.id)
                    .unwrap();
                if current.state != "running" {
                    assert_eq!(current.state, "succeeded", "{:?}", current.result);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let stored = state.db.list_subscriptions().await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name.as_deref(), Some(expected));
    }
    server.await.unwrap();
}
