use std::collections::BTreeMap;
use std::time::Duration;

use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::import_subscription;
use crate::app::{commands, subscription_refresh};
use crate::cli::Cli;

fn response(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn nodes_response() -> String {
    response("200 OK", "", "vless://uuid-123@example.com:443#One")
}

async fn server(responses: Vec<String>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/private-token", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(60), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let byte = tokio::time::timeout(Duration::from_secs(10), socket.read_u8())
                    .await
                    .unwrap()
                    .unwrap();
                request.push(byte);
            }
            requests.push(String::from_utf8(request).unwrap());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    (url, task)
}

#[tokio::test]
async fn import_without_headers_clears_saved_headers() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("subscription-headers")
        .build_with_root()
        .await;
    let (url, requests) = server(vec![nodes_response(), nodes_response()]).await;
    let cli = Cli::parse_from([
        "xrat",
        "import",
        &url,
        "--header",
        "X-HWID: discarded",
        "--header",
        "x-hwid: device-secret",
        "--header",
        "User-Agent: Happ",
    ]);
    commands::run(&context, &cli.command).await.unwrap();
    let stored = context.db.list_subscriptions().await.unwrap();
    assert_eq!(stored[0].headers["x-hwid"], "device-secret");
    let cli = Cli::parse_from(["xrat", "import", &url]);
    commands::run(&context, &cli.command).await.unwrap();
    let stored = context.db.list_subscriptions().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].headers.is_empty());
    let requests = requests.await.unwrap();
    assert!(requests[0].contains("x-hwid: device-secret\r\n"));
    assert!(requests[0].contains("user-agent: Happ\r\n"));
    assert!(!requests[0].contains("discarded"));
    assert!(!requests[1].contains("x-hwid:"));
    assert!(!requests[1].contains("user-agent: Happ\r\n"));
}

#[tokio::test]
async fn manual_and_automatic_refresh_use_saved_subscription_headers() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("subscription-refresh-headers")
        .build_with_root()
        .await;
    let (url, requests) = server(vec![nodes_response(), nodes_response(), nodes_response()]).await;
    let headers = BTreeMap::from([("x-hwid".into(), "device-secret".into())]);
    import_subscription(&context.db, &url, None, &headers)
        .await
        .unwrap();

    assert_eq!(
        commands::update::update_all(&context)
            .await
            .unwrap()
            .succeeded,
        1
    );
    assert_eq!(
        subscription_refresh::refresh_all(&context).await.succeeded,
        1
    );
    let requests = requests.await.unwrap();
    assert!(requests[1].contains("x-hwid: device-secret\r\n"));
    assert!(requests[2].contains("x-hwid: device-secret\r\n"));
}

#[tokio::test]
async fn failed_import_preserves_saved_headers() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("subscription-failed-import")
        .build_with_root()
        .await;
    let (url, requests) = server(vec![
        nodes_response(),
        response("403 Forbidden", "", "secret-response"),
    ])
    .await;
    let original = BTreeMap::from([("x-hwid".into(), "device-secret".into())]);
    let summary = import_subscription(&context.db, &url, None, &original)
        .await
        .unwrap();

    let error = import_subscription(&context.db, &url, None, &BTreeMap::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("403"));
    assert!(!error.to_string().contains("secret"));
    let stored = context
        .db
        .get_subscription_by_id(summary.subscription_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.headers, original);
    let requests = requests.await.unwrap();
    assert!(!requests[1].contains("x-hwid:"));
}

#[tokio::test]
async fn subscription_headers_reject_non_url_inputs() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("subscription-input")
        .build_with_root()
        .await;
    let headers = BTreeMap::from([("x-hwid".into(), "device-secret".into())]);
    for input in ["./nodes.txt", "vless://uuid-123@example.com:443#One"] {
        let error = import_subscription(&context.db, input, None, &headers)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("requires a subscription URL"));
    }
    assert_eq!(context.db.get_subscription_count().await.unwrap(), 0);
}

#[tokio::test]
async fn subscription_contents_are_nodes_not_nested_subscriptions() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("subscription-nested")
        .build_with_root()
        .await;
    let nested_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nested = format!("http://{}", nested_listener.local_addr().unwrap());
    let (url, requests) = server(vec![response("200 OK", "", &nested)]).await;
    let headers = BTreeMap::from([("x-hwid".into(), "device-secret".into())]);
    assert_eq!(
        import_subscription(&context.db, &url, None, &headers)
            .await
            .unwrap()
            .imported_configs,
        1
    );
    assert!(requests.await.unwrap()[0].contains("x-hwid: device-secret\r\n"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), nested_listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn https_nodes_survive_import_and_both_refresh_paths_without_connections() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("https-nodes")
        .build_with_root()
        .await;
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node = format!("https://{}#Proxy", proxy.local_addr().unwrap());
    let payload = format!("{node}\nvless://uuid-123@example.com:443#One");
    let (url, requests) = server(vec![response("200 OK", "", &payload); 3]).await;
    assert_eq!(
        import_subscription(&context.db, &url, None, &BTreeMap::new())
            .await
            .unwrap()
            .imported_configs,
        2
    );
    assert_eq!(
        commands::update::update_all(&context)
            .await
            .unwrap()
            .succeeded,
        1
    );
    assert_eq!(
        subscription_refresh::refresh_all(&context).await.succeeded,
        1
    );
    assert_eq!(requests.await.unwrap().len(), 3);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), proxy.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_url_list_fetches_once_and_does_not_expand_responses() {
    let root = tempfile::tempdir().unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node = format!("https://{}#Proxy", proxy.local_addr().unwrap());
    let (first, first_requests) = server(vec![response("200 OK", "", &node)]).await;
    let (second, second_requests) = server(vec![nodes_response()]).await;
    let path = root.path().join("urls.txt");
    std::fs::write(&path, format!("# subscriptions\n{first}\n\n{second}\n")).unwrap();
    let (source, nodes) = super::load_url_list(path.to_str().unwrap()).await.unwrap();
    assert_eq!(source.kind, xrat_db::SourceKind::File);
    assert_eq!(nodes.len(), 2);
    assert_eq!(first_requests.await.unwrap().len(), 1);
    assert_eq!(second_requests.await.unwrap().len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), proxy.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn url_list_rejects_remote_inputs_and_invalid_entries_before_fetching() {
    assert!(
        super::load_url_list("https://example.invalid/list")
            .await
            .is_err()
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("urls.txt");
    std::fs::write(
        &path,
        "https://example.invalid/sub\nvless://uuid@example.com:443",
    )
    .unwrap();
    assert!(super::load_url_list(path.to_str().unwrap()).await.is_err());
}

#[tokio::test]
async fn import_subscription_automatically_detects_name_from_profile_title_header() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("sub-name-header")
        .build_with_root()
        .await;
    let resp = response(
        "200 OK",
        "Profile-Title: ServerProvider\r\n",
        "vless://uuid-123@example.com:443#One",
    );
    let (url, _requests) = server(vec![resp]).await;
    import_subscription(&context.db, &url, None, &BTreeMap::new())
        .await
        .unwrap();

    let stored = context.db.list_subscriptions().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name.as_deref(), Some("ServerProvider"));
}

#[tokio::test]
async fn import_subscription_automatically_detects_name_from_url_fragment() {
    let (context, _root) = crate::app::tests::TestAppBuilder::new("sub-name-fragment")
        .build_with_root()
        .await;
    let (url, _requests) = server(vec![nodes_response()]).await;
    let url_with_frag = format!("{url}#CustomFragmentName");
    import_subscription(&context.db, &url_with_frag, None, &BTreeMap::new())
        .await
        .unwrap();

    let stored = context.db.list_subscriptions().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name.as_deref(), Some("CustomFragmentName"));
}
