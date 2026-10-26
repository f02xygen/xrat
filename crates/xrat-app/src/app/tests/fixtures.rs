//! Node and import-source fixtures shared by application tests.

use xrat_db::{ImportSource, SourceKind};
use xrat_model::{Node, Protocol};

/// A minimal raw-text import source for tests.
pub fn test_source() -> ImportSource {
    ImportSource {
        kind: SourceKind::RawText,
        value: "test".to_string(),
        name: Some("test".to_string()),
    }
}

/// A VLESS/TLS node pointing at `address`.
pub fn test_node(address: &str) -> Node {
    test_node_with(address, "test")
}

/// A VLESS/TLS node with an explicit display name.
pub fn test_node_with(address: &str, name: &str) -> Node {
    Node {
        protocol: Protocol::Vless,
        address: address.to_string(),
        port: 443,
        username: None,
        uuid: Some("00000000-0000-0000-0000-000000000000".to_string()),
        password: None,
        method: None,
        network: "tcp".to_string(),
        tls: Some("tls".to_string()),
        sni: Some(address.to_string()),
        host: None,
        path: None,
        name: Some(name.to_string()),
        extensions: None,
        raw_config: format!(
            "vless://00000000-0000-0000-0000-000000000000@{address}:443?security=tls#{name}"
        ),
    }
}

pub async fn named_subscription_url(request_count: usize) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/subscription#UrlProvider",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        let body = "vless://uuid-123@example.com:443#One";
        let response = format!(
            "HTTP/1.1 200 OK\r\nProfile-Title: ServerProvider\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        for _ in 0..request_count {
            let (mut socket, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (url, task)
}
