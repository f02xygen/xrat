use super::*;
use std::sync::Arc;
use xrat_support::http::*;

struct FailingHttp {
    fail_body: bool,
}

struct FailingBody;

#[async_trait::async_trait]
impl ResponseBody for FailingBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        Err(HttpError::new(HttpErrorKind::Timeout, "response timed out"))
    }
}

#[async_trait::async_trait]
impl HttpClient for FailingHttp {
    async fn execute(&self, _: HttpRequest) -> Result<HttpResponse, HttpError> {
        if !self.fail_body {
            return Err(HttpError::new(
                HttpErrorKind::Connect,
                "connection reset by peer",
            ));
        }
        let mut response = HttpResponse::from_bytes(StatusCode::OK, vec![]);
        response.body = Box::new(FailingBody);
        Ok(response)
    }
}

#[tokio::test]
async fn metadata_failures_report_operation_cause_and_proxy_guidance() {
    for (fail_body, operation, cause, kind) in [
        (
            false,
            "querying",
            "connection reset by peer",
            HttpErrorKind::Connect,
        ),
        (
            true,
            "reading",
            "response timed out",
            HttpErrorKind::Timeout,
        ),
    ] {
        let provider = HttpReleaseProvider(Client::with_transport(
            Arc::new(FailingHttp { fail_body }),
            HttpOptions::default(),
        ));
        let error = provider.latest_tag(8).await.unwrap_err();
        let message = error.to_string();
        assert!(message.contains(operation));
        assert!(message.contains("api.github.com"));
        assert!(message.contains(cause));
        assert!(message.contains("HTTPS_PROXY"));
        assert!(
            matches!(error, crate::app::AppError::ReleaseHttp { source, .. } if source.kind == kind)
        );
    }
}
