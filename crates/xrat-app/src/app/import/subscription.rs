use std::collections::BTreeMap;

use crate::app::AppError;
use crate::app::input::source::read_input_async_with_headers;
use xrat_db::{Database, ImportSummary};
use xrat_support::http::{Client, HeaderMap, HeaderName, HeaderValue, RedirectPolicy};
use xrat_support::url::looks_like_url;

pub async fn import_subscription(
    database: &Database,
    input: &str,
    name: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> crate::app::Result<ImportSummary> {
    import_subscription_with_fallback(database, input, name, None, headers).await
}

pub(crate) async fn import_subscription_with_fallback(
    database: &Database,
    input: &str,
    name: Option<&str>,
    fallback_name: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> crate::app::Result<ImportSummary> {
    let (mut source, bytes) = super::load_subscription_input(input, headers).await?;
    source.name = source.name.or_else(|| fallback_name.map(str::to_owned));
    let nodes = super::parse_nodes(&bytes)?;
    let stored_headers = matches!(source.kind, xrat_db::SourceKind::Url).then_some(headers);
    super::persist_nodes(database, source, &nodes, name, stored_headers).await
}

pub(crate) async fn load_subscription_input(
    input: &str,
    headers: &BTreeMap<String, String>,
) -> crate::app::Result<(xrat_db::ImportSource, Vec<u8>)> {
    if !looks_like_url(input) {
        if !headers.is_empty() {
            return Err(AppError::InvalidArgument(
                "--header requires a subscription URL".into(),
            ));
        }
        return crate::app::input::source::read_input_async(input).await;
    }

    let request_headers = parse_request_headers(headers)?;
    let client = if request_headers.is_empty() {
        Client::new()
    } else {
        Client::builder()
            .redirect(RedirectPolicy::SameOrigin)
            .build()?
    };
    read_input_async_with_headers(input, &client, request_headers).await
}

pub(crate) fn parse_request_headers(
    headers: &BTreeMap<String, String>,
) -> crate::app::Result<HeaderMap> {
    let mut request_headers = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| AppError::InvalidArgument("invalid subscription header name".into()))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| AppError::InvalidArgument("invalid subscription header value".into()))?;
        value.set_sensitive(true);
        request_headers.insert(name, value);
    }
    Ok(request_headers)
}
