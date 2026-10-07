use super::*;

pub struct ReqwestHttpClient {
    client: reqwest::Client,
    options: HttpOptions,
}
impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            options: HttpOptions::default(),
        }
    }
}
impl ReqwestHttpClient {
    pub fn new(options: &HttpOptions) -> Result<Self, HttpError> {
        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = options.timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(proxy) = &options.proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(adapt_error)?);
        }
        if let Some(agent) = &options.user_agent {
            builder = builder.user_agent(agent);
        }
        builder = match options.redirect {
            RedirectPolicy::Default => builder,
            RedirectPolicy::None => builder.redirect(reqwest::redirect::Policy::none()),
            RedirectPolicy::Limited(count) => {
                builder.redirect(reqwest::redirect::Policy::limited(count))
            }
        };
        Ok(Self {
            client: builder.build().map_err(adapt_error)?,
            options: options.clone(),
        })
    }
}
#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let alternate = if request.options != self.options {
            Some(Self::new(&request.options)?)
        } else {
            None
        };
        let client = alternate
            .as_ref()
            .map(|adapter| &adapter.client)
            .unwrap_or(&self.client);
        let mut builder = client
            .request(request.method, &request.url)
            .headers(request.headers);
        if !request.body.is_empty() {
            builder = builder.body(request.body);
        }
        let response = builder.send().await.map_err(adapt_error)?;
        Ok(HttpResponse {
            status: response.status(),
            headers: response.headers().clone(),
            content_length: response.content_length(),
            remote_addr: response.remote_addr(),
            body: Box::new(ReqwestBody(response)),
        })
    }
}
struct ReqwestBody(reqwest::Response);
#[async_trait]
impl ResponseBody for ReqwestBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        self.0
            .chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(adapt_error)
    }
}
pub struct ReqwestBlockingHttpClient;
impl BlockingHttpClient for ReqwestBlockingHttpClient {
    fn get(&self, url: &str) -> Result<BlockingResponse, HttpError> {
        let response = reqwest::blocking::get(url).map_err(adapt_error)?;
        let status = response.status();
        Ok(BlockingResponse {
            status,
            bytes: response.bytes().map_err(adapt_error)?.to_vec(),
        })
    }
}
pub(super) fn validate_proxy(value: &str) -> Result<(), HttpError> {
    reqwest::Proxy::all(value).map(|_| ()).map_err(adapt_error)
}
fn adapt_error(error: reqwest::Error) -> HttpError {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !detail.is_empty() && !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    let kind = if error.is_timeout() {
        HttpErrorKind::Timeout
    } else if message.to_lowercase().contains("tls") {
        HttpErrorKind::Tls
    } else if message.contains("407") {
        HttpErrorKind::Auth
    } else if error.is_connect() {
        HttpErrorKind::Connect
    } else if error.is_redirect() {
        HttpErrorKind::Redirect
    } else if error.is_request() {
        HttpErrorKind::Request
    } else if error.is_body() || error.is_decode() {
        HttpErrorKind::Body
    } else {
        HttpErrorKind::Other
    };
    HttpError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_underlying_request_error_details() {
        let error = reqwest::Client::new().get("invalid").build().unwrap_err();
        let source = std::error::Error::source(&error).unwrap().to_string();
        let adapted = adapt_error(error);
        assert!(adapted.message.contains(&source));
    }
}
