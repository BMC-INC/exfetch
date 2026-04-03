use serde::Serialize;

use crate::extract::{metadata, readability};
use crate::fetch::http::FetchResponse;
use crate::output::text;

/// Structured JSON output for a fetched page.
#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: String,
    pub word_count: usize,
    pub fetch_time_ms: u64,
}

/// Build a `JsonOutput` from a `FetchResponse`, extract readable content +
/// metadata, and serialize to pretty-printed JSON.
pub fn format(response: &FetchResponse, max_length: Option<usize>) -> String {
    let meta = metadata::extract(&response.body);
    let content = readability::extract(&response.body);
    let content = text::format_raw(&content, max_length);
    let word_count = content.split_whitespace().count();

    let output = JsonOutput {
        url: response.final_url.clone(),
        title: meta.title.or(meta.og_title),
        description: meta.description.or(meta.og_description),
        content,
        word_count,
        fetch_time_ms: response.fetch_time_ms,
    };

    serde_json::to_string_pretty(&output)
        .unwrap_or_else(|e| format!("{{\"error\": \"JSON serialization failed: {}\"}}", e))
}
