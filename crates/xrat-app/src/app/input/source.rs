use std::fs;
use std::path::Path;

use serde::Serialize;

use xrat_db::{ImportSource, SourceKind};
use xrat_support::url::looks_like_url;

pub fn read_input(input: &str) -> crate::app::Result<(ImportSource, Vec<u8>)> {
    read_input_with_client(input, &xrat_support::http::ReqwestBlockingHttpClient)
}

pub fn read_input_with_client(
    input: &str,
    client: &dyn xrat_support::http::BlockingHttpClient,
) -> crate::app::Result<(ImportSource, Vec<u8>)> {
    if looks_like_url(input) {
        return Ok((
            ImportSource {
                kind: SourceKind::Url,
                value: input.to_string(),
                name: None,
            },
            fetch_url_with_client(input, client)?,
        ));
    }

    read_non_url_input(input)
}

pub async fn read_input_async(input: &str) -> crate::app::Result<(ImportSource, Vec<u8>)> {
    read_input_async_with_client(input, &xrat_support::http::Client::new()).await
}

pub async fn read_input_async_with_client(
    input: &str,
    client: &xrat_support::http::Client,
) -> crate::app::Result<(ImportSource, Vec<u8>)> {
    if looks_like_url(input) {
        return Ok((
            ImportSource {
                kind: SourceKind::Url,
                value: input.to_string(),
                name: None,
            },
            fetch_url_async_with_client(input, client).await?,
        ));
    }

    read_non_url_input(input)
}

pub fn fetch_url(url: &str) -> crate::app::Result<Vec<u8>> {
    fetch_url_with_client(url, &xrat_support::http::ReqwestBlockingHttpClient)
}

pub fn fetch_url_with_client(
    url: &str,
    client: &dyn xrat_support::http::BlockingHttpClient,
) -> crate::app::Result<Vec<u8>> {
    let response = client
        .get(url)
        .map_err(safe_input_error)?
        .error_for_status()
        .map_err(safe_input_error)?;
    Ok(response.bytes().map_err(safe_input_error)?.to_vec())
}

pub async fn fetch_url_async(url: &str) -> crate::app::Result<Vec<u8>> {
    fetch_url_async_with_client(url, &xrat_support::http::Client::new()).await
}

pub async fn fetch_url_async_with_client(
    url: &str,
    client: &xrat_support::http::Client,
) -> crate::app::Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(safe_input_error)?
        .error_for_status()
        .map_err(safe_input_error)?;
    Ok(response.bytes().await.map_err(safe_input_error)?.to_vec())
}

fn safe_input_error(error: xrat_support::http::HttpError) -> crate::app::AppError {
    xrat_support::http::HttpError::new(error.kind, error.safe_summary()).into()
}

fn read_non_url_input(input: &str) -> crate::app::Result<(ImportSource, Vec<u8>)> {
    let path = Path::new(input);
    if path.exists() {
        return Ok((
            ImportSource {
                kind: SourceKind::File,
                value: input.to_string(),
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
            },
            fs::read(path)?,
        ));
    }

    Ok((
        ImportSource {
            kind: SourceKind::RawText,
            value: input.to_string(),
            name: None,
        },
        input.as_bytes().to_vec(),
    ))
}

pub fn save_json<T: Serialize>(output_path: &Path, value: &T) -> crate::app::Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let body = serde_json::to_string_pretty(value)?;
    fs::write(output_path, body)?;
    Ok(())
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod diagnostic_tests;

#[cfg(test)]
mod http_port_tests {
    use super::*;
    use std::sync::Arc;
    use xrat_support::http::*;
    struct FakeHttp {
        status: StatusCode,
    }
    #[async_trait::async_trait]
    impl HttpClient for FakeHttp {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
            assert_eq!(request.url, "https://example.invalid/sub");
            Ok(HttpResponse::from_bytes(
                self.status,
                b"vless://fixture".to_vec(),
            ))
        }
    }
    #[tokio::test]
    async fn import_url_uses_fake_transport_and_keeps_status_validation() {
        for status in [StatusCode::OK, StatusCode::NOT_FOUND] {
            let client =
                Client::with_transport(Arc::new(FakeHttp { status }), HttpOptions::default());
            let result = read_input_async_with_client("https://example.invalid/sub", &client).await;
            if status == StatusCode::OK {
                let (source, bytes) = result.unwrap();
                assert_eq!(source.kind, SourceKind::Url);
                assert_eq!(bytes, b"vless://fixture");
            } else {
                assert!(matches!(
                    result,
                    Err(crate::app::AppError::Http(HttpError {
                        kind: HttpErrorKind::Status,
                        ..
                    }))
                ));
            }
        }
    }
}
