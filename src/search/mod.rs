pub mod engine;
pub mod result;

use std::time::Duration;

use anyhow::{Context, Result};

use crate::extract::readability;
use crate::fetch::http::fetch_url;

use result::{FetchedSearchResult, SearchResult};

/// Search DuckDuckGo and then fetch the content of the top results.
///
/// Performs a search, then fetches up to `fetch_count` of the top results,
/// with a 500ms delay between fetches to be polite to servers.
/// Returns all search results, with fetched content attached to the top ones.
pub async fn search_and_fetch(
    query: &str,
    num_results: usize,
    fetch_count: usize,
    timeout: Duration,
) -> Result<Vec<FetchedSearchResult>> {
    let search_results = engine::search_ddg(query, num_results, timeout)
        .await
        .context("search failed")?;

    let mut fetched_results = Vec::with_capacity(search_results.len());

    for (i, result) in search_results.iter().enumerate() {
        if i < fetch_count {
            // Add a 500ms delay between fetches (but not before the first one)
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            let fetch_result = fetch_url(&result.url, timeout, "exfetch/0.1").await;
            match fetch_result {
                Ok(resp) => {
                    let content = readability::extract(&resp.body);
                    fetched_results.push(FetchedSearchResult {
                        title: result.title.clone(),
                        url: result.url.clone(),
                        snippet: result.snippet.clone(),
                        content: Some(content),
                        fetch_error: None,
                    });
                }
                Err(e) => {
                    fetched_results.push(FetchedSearchResult {
                        title: result.title.clone(),
                        url: result.url.clone(),
                        snippet: result.snippet.clone(),
                        content: None,
                        fetch_error: Some(e.to_string()),
                    });
                }
            }
        } else {
            // Non-fetched results: include search data only
            fetched_results.push(FetchedSearchResult {
                title: result.title.clone(),
                url: result.url.clone(),
                snippet: result.snippet.clone(),
                content: None,
                fetch_error: None,
            });
        }
    }

    Ok(fetched_results)
}

/// Format search results as plain text.
pub fn format_results_text(results: &[SearchResult]) -> String {
    let mut output = String::new();
    for (i, r) in results.iter().enumerate() {
        output.push_str(&format!("{}. {}\n", i + 1, r.title));
        output.push_str(&format!("   {}\n", r.url));
        if !r.snippet.is_empty() {
            output.push_str(&format!("   {}\n", r.snippet));
        }
        output.push('\n');
    }
    output.trim_end().to_string()
}

/// Format fetched search results as plain text.
pub fn format_fetched_results_text(results: &[FetchedSearchResult]) -> String {
    let mut output = String::new();
    for (i, r) in results.iter().enumerate() {
        output.push_str(&format!("{}. {}\n", i + 1, r.title));
        output.push_str(&format!("   {}\n", r.url));
        if !r.snippet.is_empty() {
            output.push_str(&format!("   {}\n", r.snippet));
        }
        if let Some(content) = &r.content {
            let preview = if content.len() > 500 {
                format!("{}...", &content[..500])
            } else {
                content.clone()
            };
            output.push_str(&format!("   --- fetched content ---\n   {}\n", preview));
        }
        if let Some(err) = &r.fetch_error {
            output.push_str(&format!("   [fetch error: {}]\n", err));
        }
        output.push('\n');
    }
    output.trim_end().to_string()
}
