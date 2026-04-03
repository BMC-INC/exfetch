use std::time::Duration;

use anyhow::{Context, Result};
use scraper::{Html, Selector};

use super::result::SearchResult;

/// Parse DuckDuckGo HTML search results page into structured results.
///
/// DDG's HTML search (html.duckduckgo.com) uses:
/// - `.result` class for each result container
/// - `.result__a` for the title link
/// - `.result__snippet` for the description snippet
pub fn parse_ddg_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);

    let result_sel = Selector::parse(".result").unwrap();
    let link_sel = Selector::parse(".result__a").unwrap();
    let snippet_sel = Selector::parse(".result__snippet").unwrap();

    let mut results = Vec::new();

    for result_el in document.select(&result_sel) {
        let title_and_url = result_el.select(&link_sel).next().map(|el| {
            let title = el.text().collect::<Vec<_>>().join("").trim().to_string();
            let href = el.value().attr("href").unwrap_or("").to_string();
            (title, href)
        });

        let snippet = result_el
            .select(&snippet_sel)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        if let Some((title, url)) = title_and_url {
            // Skip results with empty titles or URLs
            if title.is_empty() || url.is_empty() {
                continue;
            }

            // DDG HTML sometimes wraps URLs in a redirect; extract the real URL
            let clean_url = extract_ddg_url(&url);

            results.push(SearchResult {
                title,
                url: clean_url,
                snippet,
            });
        }
    }

    results
}

/// Extract the actual URL from a DuckDuckGo redirect link.
///
/// DDG HTML results sometimes use links like:
/// `//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=...`
///
/// This function extracts the real destination URL from the `uddg` parameter.
fn extract_ddg_url(raw: &str) -> String {
    if raw.contains("duckduckgo.com/l/") || raw.contains("duckduckgo.com/l?") {
        // Try to extract the uddg parameter
        if let Some(uddg_start) = raw.find("uddg=") {
            let value_start = uddg_start + 5;
            let value_end = raw[value_start..]
                .find('&')
                .map(|i| value_start + i)
                .unwrap_or(raw.len());
            let encoded = &raw[value_start..value_end];
            return urlencoding::decode(encoded)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| raw.to_string());
        }
    }
    raw.to_string()
}

/// Search DuckDuckGo's HTML interface and return parsed results.
///
/// Posts to `https://html.duckduckgo.com/html/` with the query, parses the
/// response, and returns up to `num_results` search results.
///
/// Uses POST (the standard DDG HTML form method) and a browser-like
/// User-Agent to reduce the likelihood of CAPTCHA challenges.
pub async fn search_ddg(
    query: &str,
    num_results: usize,
    timeout: Duration,
) -> Result<Vec<SearchResult>> {
    let url = "https://html.duckduckgo.com/html/";

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .build()
        .context("failed to build HTTP client")?;

    let params = [("q", query)];
    let response = client
        .post(url)
        .form(&params)
        .send()
        .await
        .context("DuckDuckGo search request failed")?;

    let html = response
        .text()
        .await
        .context("failed to read DuckDuckGo response body")?;

    let mut results = parse_ddg_html(&html);
    results.truncate(num_results);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ddg_url_plain() {
        let url = "https://example.com/page";
        assert_eq!(extract_ddg_url(url), "https://example.com/page");
    }

    #[test]
    fn test_extract_ddg_url_redirect() {
        let url = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc";
        assert_eq!(extract_ddg_url(url), "https://example.com/page");
    }

    #[test]
    fn test_parse_ddg_html_empty() {
        let results = parse_ddg_html("<html><body></body></html>");
        assert!(results.is_empty());
    }
}
