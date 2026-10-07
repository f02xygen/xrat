use crate::app::ports::ReleaseProvider;

pub struct GithubReleaseProvider;

#[cfg(test)]
mod failure_tests;

#[async_trait::async_trait]
impl ReleaseProvider for GithubReleaseProvider {
    async fn latest_tag(&self, timeout_secs: u64) -> crate::app::Result<String> {
        let client = xrat_support::http::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent(concat!("xrat/", env!("CARGO_PKG_VERSION")))
            .build()?;
        latest_tag_with_client(&client).await
    }
}

pub struct HttpReleaseProvider(pub xrat_support::http::Client);
#[async_trait::async_trait]
impl ReleaseProvider for HttpReleaseProvider {
    async fn latest_tag(&self, timeout_secs: u64) -> crate::app::Result<String> {
        latest_tag_with_client(
            &self
                .0
                .clone()
                .with_timeout(std::time::Duration::from_secs(timeout_secs)),
        )
        .await
    }
}

async fn latest_tag_with_client(client: &xrat_support::http::Client) -> crate::app::Result<String> {
    let response = client
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            super::REPO
        ))
        .send()
        .await
        .map_err(|source| crate::app::AppError::ReleaseHttp {
            operation: "querying latest xrat release from api.github.com",
            source,
        })?;
    if !response.status().is_success() {
        return Err(crate::app::AppError::InvalidArgument(format!(
            "failed to query latest release: HTTP {}",
            response.status()
        )));
    }
    let body = response
        .text()
        .await
        .map_err(|source| crate::app::AppError::ReleaseHttp {
            operation: "reading latest xrat release metadata from api.github.com",
            source,
        })?;
    let payload: serde_json::Value = serde_json::from_str(&body)?;
    payload
        .get("tag_name")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            crate::app::AppError::InvalidArgument("latest release response had no tag_name".into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use xrat_support::http::*;
    struct FakeHttp(&'static str);
    #[async_trait::async_trait]
    impl HttpClient for FakeHttp {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
            assert_eq!(
                request.options.timeout,
                Some(std::time::Duration::from_secs(8))
            );
            assert!(request.url.ends_with("/releases/latest"));
            Ok(HttpResponse::from_bytes(
                StatusCode::OK,
                self.0.as_bytes().to_vec(),
            ))
        }
    }
    #[tokio::test]
    async fn release_metadata_uses_fake_http_and_validates_payload() {
        for (body, expected) in [
            ("{\"tag_name\":\"v0.21.0\"}", Some("v0.21.0")),
            ("{}", None),
            ("broken", None),
        ] {
            let provider = HttpReleaseProvider(Client::with_transport(
                Arc::new(FakeHttp(body)),
                HttpOptions::default(),
            ));
            let result = provider.latest_tag(8).await;
            match expected {
                Some(expected) => assert_eq!(result.unwrap(), expected),
                None => assert!(result.is_err()),
            }
        }
    }
}
