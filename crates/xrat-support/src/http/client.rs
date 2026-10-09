use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpErrorKind {
    Timeout,
    Connect,
    Tls,
    Auth,
    Redirect,
    Request,
    Body,
    Status,
    Other,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct HttpError {
    pub kind: HttpErrorKind,
    pub message: String,
}

impl HttpError {
    pub fn new(kind: HttpErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn is_timeout(&self) -> bool {
        self.kind == HttpErrorKind::Timeout
    }
    pub fn is_connect(&self) -> bool {
        self.kind == HttpErrorKind::Connect
    }
    pub fn is_redirect(&self) -> bool {
        self.kind == HttpErrorKind::Redirect
    }
    pub fn is_request(&self) -> bool {
        self.kind == HttpErrorKind::Request
    }

    pub fn safe_summary(&self) -> String {
        match self.kind {
            HttpErrorKind::Timeout => "request timed out".into(),
            HttpErrorKind::Connect => {
                let message = self.message.to_ascii_lowercase();
                if message.contains("dns error") || message.contains("failed to lookup address") {
                    "DNS lookup failed".into()
                } else {
                    "connection failed".into()
                }
            }
            HttpErrorKind::Tls => "TLS handshake or certificate verification failed".into(),
            HttpErrorKind::Auth => "proxy authentication failed".into(),
            HttpErrorKind::Redirect => "redirect failed or exceeded the redirect limit".into(),
            HttpErrorKind::Request => "invalid HTTP request".into(),
            HttpErrorKind::Body => "response body could not be read".into(),
            HttpErrorKind::Status => {
                let status = self
                    .message
                    .strip_prefix("HTTP status ")
                    .and_then(|message| message.split_whitespace().next())
                    .and_then(|value| value.parse::<u16>().ok())
                    .and_then(|value| StatusCode::from_u16(value).ok())
                    .filter(|status| status.is_client_error() || status.is_server_error());
                status
                    .map(|status| format!("HTTP status {status}"))
                    .unwrap_or_else(|| "unsuccessful HTTP response".into())
            }
            HttpErrorKind::Other => "HTTP transport failed".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedirectPolicy {
    #[default]
    Default,
    None,
    Limited(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpOptions {
    pub timeout: Option<Duration>,
    pub proxy: Option<String>,
    pub redirect: RedirectPolicy,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub options: HttpOptions,
}

#[allow(
    clippy::double_must_use,
    reason = "async_trait adds must_use to methods returning already must-use boxed futures"
)]
#[async_trait]
pub trait ResponseBody: Send {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError>;
}

pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub content_length: Option<u64>,
    pub remote_addr: Option<SocketAddr>,
    pub body: Box<dyn ResponseBody>,
}

pub(super) struct BufferedBody(pub(super) Option<Vec<u8>>);

#[async_trait]
impl ResponseBody for BufferedBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        Ok(self.0.take())
    }
}

impl HttpResponse {
    pub fn from_bytes(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            content_length: Some(body.len() as u64),
            remote_addr: None,
            body: Box::new(BufferedBody(Some(body))),
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }
    pub async fn chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        self.body.next_chunk().await
    }
    pub async fn bytes(mut self) -> Result<Vec<u8>, HttpError> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.chunk().await? {
            bytes.extend(chunk);
        }
        Ok(bytes)
    }
    pub async fn text(self) -> Result<String, HttpError> {
        Ok(String::from_utf8_lossy(&self.bytes().await?)
            .trim_start_matches('\u{feff}')
            .to_string())
    }
    pub fn error_for_status(self) -> Result<Self, HttpError> {
        if self.status.is_client_error() || self.status.is_server_error() {
            Err(HttpError::new(
                HttpErrorKind::Status,
                format!("HTTP status {}", self.status),
            ))
        } else {
            Ok(self)
        }
    }
}

#[allow(
    clippy::double_must_use,
    reason = "async_trait adds must_use to methods returning already must-use boxed futures"
)]
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

#[derive(Clone)]
pub struct Client {
    pub(super) transport: Arc<dyn HttpClient>,
    pub(super) options: HttpOptions,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        Self::with_transport(
            Arc::new(ReqwestHttpClient::default()),
            HttpOptions::default(),
        )
    }
    pub fn with_transport(transport: Arc<dyn HttpClient>, options: HttpOptions) -> Self {
        Self { transport, options }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.options.timeout = Some(timeout);
        self
    }
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }
    pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::GET, url)
    }
    pub fn head(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }
    pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::POST, url)
    }
    pub(super) fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
        RequestBuilder {
            client: self.clone(),
            request: HttpRequest {
                method,
                url: url.as_ref().into(),
                headers: HeaderMap::new(),
                body: Vec::new(),
                options: self.options.clone(),
            },
            error: None,
        }
    }
}

#[derive(Default)]
pub struct ClientBuilder {
    pub(super) options: HttpOptions,
}

impl ClientBuilder {
    pub fn timeout(mut self, value: Duration) -> Self {
        self.options.timeout = Some(value);
        self
    }
    pub fn proxy(mut self, value: Proxy) -> Self {
        self.options.proxy = Some(value.0);
        self
    }
    pub fn redirect(mut self, value: RedirectPolicy) -> Self {
        self.options.redirect = value;
        self
    }
    pub fn user_agent(mut self, value: impl Into<String>) -> Self {
        self.options.user_agent = Some(value.into());
        self
    }
    pub fn build(self) -> Result<Client, HttpError> {
        let transport = Arc::new(ReqwestHttpClient::new(&self.options)?);
        Ok(Client::with_transport(transport, self.options))
    }
}

pub struct Proxy(pub(super) String);

impl Proxy {
    pub fn all(value: &str) -> Result<Self, HttpError> {
        reqwest_adapter::validate_proxy(value)?;
        Ok(Self(value.into()))
    }
}

pub struct RequestBuilder {
    pub(super) client: Client,
    pub(super) request: HttpRequest,
    pub(super) error: Option<HttpError>,
}

impl RequestBuilder {
    pub fn bearer_auth(mut self, value: &str) -> Self {
        match HeaderValue::from_str(&format!("Bearer {value}")) {
            Ok(value) => {
                self.request.headers.insert("authorization", value);
            }
            Err(error) => {
                self.error = Some(HttpError::new(HttpErrorKind::Request, error.to_string()));
            }
        }
        self
    }
    pub fn body(mut self, value: Vec<u8>) -> Self {
        self.request.body = value;
        self
    }
    pub async fn send(self) -> Result<HttpResponse, HttpError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.client.transport.execute(self.request).await
    }
}

pub async fn get(url: &str) -> Result<HttpResponse, HttpError> {
    Client::new().get(url).send().await
}

pub struct BlockingResponse {
    pub status: StatusCode,
    pub bytes: Vec<u8>,
}

impl BlockingResponse {
    pub fn error_for_status(self) -> Result<Self, HttpError> {
        if self.status.is_client_error() || self.status.is_server_error() {
            Err(HttpError::new(
                HttpErrorKind::Status,
                format!("HTTP status {}", self.status),
            ))
        } else {
            Ok(self)
        }
    }
    pub fn bytes(self) -> Result<Vec<u8>, HttpError> {
        Ok(self.bytes)
    }
}

pub trait BlockingHttpClient: Send + Sync {
    fn get(&self, url: &str) -> Result<BlockingResponse, HttpError>;
}
