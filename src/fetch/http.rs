use std::time::{Duration, Instant};

use reqwest::redirect::Policy;
use thiserror::Error;
use url::Url;

/// Response returned from a successful fetch.
#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub body: String,
    pub status: u16,
    pub content_type: String,
    pub final_url: String,
    pub headers: Vec<(String, String)>,
    pub fetch_time_ms: u64,
}

/// Errors that can occur during fetching.
#[derive(Debug, Error)]
pub enum FetchError {
    #[error("HTTP {status} for {url}")]
    HttpError { status: u16, url: String },

    #[error("request timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    #[error("too many redirects")]
    TooManyRedirects,

    #[error("binary content type rejected: {content_type}")]
    BinaryContent { content_type: String },

    #[error("network error: {0}")]
    Network(String),

    #[error("invalid URL: {0}")]
    InvalidUrl(String),
}

/// Normalize a raw URL string into a proper `Url`.
///
/// If the input has no scheme, `https://` is automatically prepended.
pub fn normalize_url(raw: &str) -> Result<Url, FetchError> {
    let input = if !raw.contains("://") {
        format!("https://{}", raw)
    } else {
        raw.to_string()
    };

    Url::parse(&input).map_err(|e| FetchError::InvalidUrl(format!("{}: {}", raw, e)))
}

/// Returns `true` if the content-type header value represents allowed (textual) content.
///
/// Allowed patterns: text/*, application/json, application/xml, application/xhtml+xml,
/// application/pdf, and anything containing "+xml" or "+json".
fn is_allowed_content_type(ct: &str) -> bool {
    let lower = ct.to_lowercase();
    lower.starts_with("text/")
        || lower.contains("application/json")
        || lower.contains("application/xml")
        || lower.contains("application/xhtml+xml")
        || lower.contains("application/pdf")
        || lower.contains("+xml")
        || lower.contains("+json")
}

/// Fetch a URL with the given timeout and user-agent.
///
/// Follows up to 10 redirects, rejects 4xx/5xx responses, and rejects binary content types.
pub async fn fetch_url(
    raw_url: &str,
    timeout: Duration,
    user_agent: &str,
) -> Result<FetchResponse, FetchError> {
    let url = normalize_url(raw_url)?;

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(Policy::limited(10))
        .user_agent(user_agent)
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;

    let start = Instant::now();

    let response = client.get(url.as_str()).send().await.map_err(|e| {
        if e.is_timeout() {
            FetchError::Timeout {
                timeout_secs: timeout.as_secs(),
            }
        } else if e.is_redirect() {
            FetchError::TooManyRedirects
        } else {
            FetchError::Network(e.to_string())
        }
    })?;

    let status = response.status().as_u16();
    let final_url = response.url().to_string();

    // Collect headers
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    // Extract content-type
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Reject 4xx/5xx
    if status >= 400 {
        return Err(FetchError::HttpError {
            status,
            url: final_url,
        });
    }

    // Reject binary content types (only if content-type header is present and non-empty)
    if !content_type.is_empty() && !is_allowed_content_type(&content_type) {
        return Err(FetchError::BinaryContent { content_type });
    }

    // Read body
    let body = response
        .text()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;

    let fetch_time_ms = start.elapsed().as_millis() as u64;

    Ok(FetchResponse {
        body,
        status,
        content_type,
        final_url,
        headers,
        fetch_time_ms,
    })
}

/// Response returned when fetching raw bytes (e.g. for PDFs).
#[derive(Debug, Clone)]
pub struct BytesResponse {
    pub bytes: Vec<u8>,
    pub status: u16,
    pub content_type: String,
    pub final_url: String,
    pub headers: Vec<(String, String)>,
    pub fetch_time_ms: u64,
}

/// Fetch a URL and return the response body as raw bytes.
///
/// Unlike [`fetch_url`], this does not reject binary content types and
/// reads the body as bytes rather than text. Useful for PDF and other
/// binary content that needs further processing.
pub async fn fetch_bytes(
    raw_url: &str,
    timeout: Duration,
    user_agent: &str,
) -> Result<BytesResponse, FetchError> {
    let url = normalize_url(raw_url)?;

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(Policy::limited(10))
        .user_agent(user_agent)
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;

    let start = Instant::now();

    let response = client.get(url.as_str()).send().await.map_err(|e| {
        if e.is_timeout() {
            FetchError::Timeout {
                timeout_secs: timeout.as_secs(),
            }
        } else if e.is_redirect() {
            FetchError::TooManyRedirects
        } else {
            FetchError::Network(e.to_string())
        }
    })?;

    let status = response.status().as_u16();
    let final_url = response.url().to_string();

    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if status >= 400 {
        return Err(FetchError::HttpError {
            status,
            url: final_url,
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?
        .to_vec();

    let fetch_time_ms = start.elapsed().as_millis() as u64;

    Ok(BytesResponse {
        bytes,
        status,
        content_type,
        final_url,
        headers,
        fetch_time_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url_with_scheme() {
        let url = normalize_url("https://example.com").unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_normalize_url_without_scheme() {
        let url = normalize_url("example.com/path").unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("example.com"));
    }

    #[test]
    fn test_is_allowed_content_type_html() {
        assert!(is_allowed_content_type("text/html; charset=utf-8"));
    }

    #[test]
    fn test_is_allowed_content_type_json() {
        assert!(is_allowed_content_type("application/json"));
    }

    #[test]
    fn test_is_allowed_content_type_binary_rejected() {
        assert!(!is_allowed_content_type("image/png"));
        assert!(!is_allowed_content_type("application/octet-stream"));
    }
}
