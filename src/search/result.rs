use serde::Serialize;

/// A single search result from a web search engine.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A search result that also includes fetched page content.
#[derive(Debug, Clone, Serialize)]
pub struct FetchedSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// The extracted text content from fetching the URL, if successful.
    pub content: Option<String>,
    /// Error message if the fetch failed.
    pub fetch_error: Option<String>,
}
