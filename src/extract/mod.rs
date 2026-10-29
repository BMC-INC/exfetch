pub mod markdown;
pub mod metadata;
pub mod readability;

/// Extract matching elements as HTML for CLI and MCP selectors.
pub fn select_html(html: &str, selector: &str) -> Result<String, String> {
    let selector = scraper::Selector::parse(selector)
        .map_err(|error| format!("Invalid CSS selector: {error}"))?;
    let document = scraper::Html::parse_document(html);
    Ok(document
        .select(&selector)
        .map(|element| element.html())
        .collect::<Vec<_>>()
        .join("\n"))
}
