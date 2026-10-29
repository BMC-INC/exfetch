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

    fetch_search_results(search_results, fetch_count, timeout).await
}

/// Fetch content for already-selected results without changing the search engine.
pub async fn fetch_search_results(
    search_results: Vec<SearchResult>,
    fetch_count: usize,
    timeout: Duration,
) -> Result<Vec<FetchedSearchResult>> {
    fetch_search_results_with_policy(search_results, fetch_count, timeout, None).await
}

pub async fn search_and_fetch_with_policy(
    query: &str,
    num_results: usize,
    fetch_count: usize,
    timeout: Duration,
    policy: &crate::engine::policy::PolicyEngine,
) -> Result<Vec<FetchedSearchResult>> {
    let results = engine::search_ddg(query, num_results, timeout).await?;
    fetch_search_results_with_policy(results, fetch_count, timeout, Some(policy)).await
}

pub async fn fetch_search_results_with_policy(
    search_results: Vec<SearchResult>,
    fetch_count: usize,
    timeout: Duration,
    policy: Option<&crate::engine::policy::PolicyEngine>,
) -> Result<Vec<FetchedSearchResult>> {
    let mut fetched_results = Vec::with_capacity(search_results.len());

    for (i, result) in search_results.iter().enumerate() {
        if i < fetch_count {
            // Add a 500ms delay between fetches (but not before the first one)
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            let fetch_result = if let Some(policy) = policy {
                crate::fetch::http::fetch_url_with_policy(
                    &result.url,
                    timeout,
                    "exfetch/0.1",
                    policy,
                )
                .await
            } else {
                fetch_url(&result.url, timeout, "exfetch/0.1").await
            };
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
                format!("{}...", content.chars().take(500).collect::<String>())
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

/// Format search results as Markdown links with optional extracted content.
pub fn format_results_markdown(results: &[SearchResult]) -> String {
    results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            format!(
                "{}. [{}]({})\n\n{}",
                index + 1,
                result.title.replace(']', "\\]"),
                result.url,
                result.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn format_fetched_results_markdown(results: &[FetchedSearchResult]) -> String {
    results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let mut text = format!(
                "## {}. [{}]({})\n\n{}",
                index + 1,
                result.title.replace(']', "\\]"),
                result.url,
                result.snippet
            );
            if let Some(content) = &result.content {
                text.push_str(&format!("\n\n{}", content));
            }
            if let Some(error) = &result.fetch_error {
                text.push_str(&format!("\n\nFetch error: {}", error));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn format_results_html(results: &[SearchResult]) -> String {
    let items = results
        .iter()
        .map(|result| {
            format!(
                "<li><h2>{}</h2><p>{}</p><p>{}</p></li>",
                escape_html(&result.title),
                escape_html(&result.url),
                escape_html(&result.snippet)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<ol>\n{}\n</ol>", items)
}

pub fn format_fetched_results_html(results: &[FetchedSearchResult]) -> String {
    let items = results
        .iter()
        .map(|result| {
            format!(
                "<li><h2>{}</h2><p>{}</p><p>{}</p><pre>{}</pre></li>",
                escape_html(&result.title),
                escape_html(&result.url),
                escape_html(&result.snippet),
                escape_html(
                    result
                        .content
                        .as_deref()
                        .or(result.fetch_error.as_deref())
                        .unwrap_or("")
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<ol>\n{}\n</ol>", items)
}

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn html_search_results_escape_untrusted_titles_and_content() {
        let results = vec![FetchedSearchResult {
            title: "<script>alert(1)</script>".into(),
            url: "https://example.com".into(),
            snippet: "A & B".into(),
            content: Some("<img src=x onerror=alert(1)>".into()),
            fetch_error: None,
        }];
        let html = format_fetched_results_html(&results);
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("A &amp; B"));
    }
}
