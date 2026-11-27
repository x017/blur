//! Blocking HTTP client utility backed by `ureq`.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::Context;

/// An outgoing HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (e.g. `"GET"`, `"POST"`, `"PUT"`, `"DELETE"`, `"PATCH"`, `"HEAD"`).
    pub method: String,
    /// Target URL.
    pub url: String,
    /// Request headers as `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Optional request body.
    pub body: Option<String>,
    /// Optional request timeout (defaults to 30 seconds if `None`).
    pub timeout: Option<Duration>,
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            url: String::new(),
            headers: Vec::new(),
            body: None,
            timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// An HTTP response with timing metadata.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code (e.g. 200, 404).
    pub status: u16,
    /// HTTP status reason phrase (e.g. `"OK"`).
    pub status_text: String,
    /// Response headers as `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Response body decoded as UTF-8 (lossy for binary bodies).
    pub body: String,
    /// Raw byte length of the response body.
    pub size: usize,
    /// Total round-trip duration in milliseconds.
    pub duration_ms: u64,
}

/// Execute a blocking HTTP request.
///
/// Non-2xx HTTP responses are returned as `Ok(HttpResponse)` with their status code and body;
/// only transport/network errors return `Err`.
pub fn send(req: HttpRequest) -> anyhow::Result<HttpResponse> {
    let timeout = req.timeout.unwrap_or_else(|| Duration::from_secs(30));
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();

    let method = req.method.trim().to_uppercase();
    let mut request = agent.request(&method, &req.url);
    for (k, v) in &req.headers {
        request = request.set(k, v);
    }

    let start = Instant::now();
    let result = match req.body {
        Some(body) => request.send_string(&body),
        None => request.call(),
    };

    let response = match result {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(ureq::Error::Transport(err)) => {
            return Err(anyhow::anyhow!("HTTP transport error: {err}"));
        }
    };

    let status = response.status();
    let status_text = response.status_text().to_string();
    let mut headers = Vec::new();
    for name in response.headers_names() {
        for value in response.all(&name) {
            headers.push((name.clone(), value.to_string()));
        }
    }

    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .context("failed to read HTTP response body")?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let size = bytes.len();
    let body = String::from_utf8_lossy(&bytes).into_owned();

    Ok(HttpResponse {
        status,
        status_text,
        headers,
        body,
        size,
        duration_ms,
    })
}
