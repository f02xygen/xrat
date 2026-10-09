use super::*;
use std::sync::Arc;
use xrat_support::http::*;

struct FailingHttp(HttpErrorKind);

#[async_trait::async_trait]
impl HttpClient for FailingHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        Err(HttpError::new(
            self.0,
            format!(
                "{} redirected to https://private.invalid/secret",
                request.url
            ),
        ))
    }
}

impl BlockingHttpClient for FailingHttp {
    fn get(&self, url: &str) -> Result<BlockingResponse, HttpError> {
        Err(HttpError::new(self.0, url))
    }
}

#[tokio::test]
async fn input_failures_preserve_kind_without_disclosing_subscription_secrets() {
    let url = "https://user:password@example.invalid/private-token?key=secret";
    for kind in [
        HttpErrorKind::Timeout,
        HttpErrorKind::Connect,
        HttpErrorKind::Tls,
        HttpErrorKind::Auth,
        HttpErrorKind::Redirect,
        HttpErrorKind::Request,
        HttpErrorKind::Body,
        HttpErrorKind::Status,
        HttpErrorKind::Other,
    ] {
        let client = Client::with_transport(Arc::new(FailingHttp(kind)), HttpOptions::default());
        for error in [
            fetch_url_async_with_client(url, &client).await.unwrap_err(),
            fetch_url_with_client(url, &FailingHttp(kind)).unwrap_err(),
        ] {
            let AppError::Http(error) = error else {
                panic!("unexpected error")
            };
            assert_eq!(error.kind, kind);
            for token in [
                "password",
                "private-token",
                "secret",
                "example.invalid",
                "private.invalid",
            ] {
                assert!(!error.to_string().contains(token));
            }
        }
    }
}

#[tokio::test]
async fn subscription_http_statuses_are_reported_without_response_body_or_url() {
    struct StatusHttp(StatusCode);
    #[async_trait::async_trait]
    impl HttpClient for StatusHttp {
        async fn execute(&self, _: HttpRequest) -> Result<HttpResponse, HttpError> {
            Ok(HttpResponse::from_bytes(
                self.0,
                b"private-response-token".to_vec(),
            ))
        }
    }
    for code in [401, 404, 429, 500, 503] {
        let client = Client::with_transport(
            Arc::new(StatusHttp(StatusCode::from_u16(code).unwrap())),
            HttpOptions::default(),
        );
        let error =
            fetch_url_async_with_client("https://user:secret@example.invalid/token", &client)
                .await
                .unwrap_err();
        let text = error.to_string();
        assert!(text.contains(&code.to_string()));
        assert!(!text.contains("token"));
        assert!(!text.contains("secret"));
    }
}

use crate::app::AppError;
