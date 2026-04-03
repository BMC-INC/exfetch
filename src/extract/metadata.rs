use scraper::{Html, Selector};

/// Metadata extracted from an HTML page.
#[derive(Debug, Clone, Default)]
pub struct PageMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub word_count: usize,
}

/// Extract metadata from an HTML document string.
pub fn extract(html: &str) -> PageMetadata {
    let document = Html::parse_document(html);

    let title = extract_title(&document);
    let description = extract_meta_content(&document, "name", "description");
    let og_title = extract_meta_content(&document, "property", "og:title");
    let og_description = extract_meta_content(&document, "property", "og:description");
    let og_image = extract_meta_content(&document, "property", "og:image");
    let word_count = count_words(&document);

    PageMetadata {
        title,
        description,
        og_title,
        og_description,
        og_image,
        word_count,
    }
}

/// Extract the text content of the <title> tag.
fn extract_title(document: &Html) -> Option<String> {
    let sel = Selector::parse("title").ok()?;
    let el = document.select(&sel).next()?;
    let text = el.text().collect::<String>().trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Extract the `content` attribute of a `<meta>` tag matching the given
/// attribute name and value.
///
/// Example: `extract_meta_content(doc, "property", "og:title")` finds
/// `<meta property="og:title" content="...">`.
fn extract_meta_content(document: &Html, attr: &str, value: &str) -> Option<String> {
    let selector_str = format!("meta[{}=\"{}\"]", attr, value);
    let sel = Selector::parse(&selector_str).ok()?;
    let el = document.select(&sel).next()?;
    let content = el.value().attr("content")?.trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// Count words in the visible body text.
fn count_words(document: &Html) -> usize {
    let body_sel = Selector::parse("body").ok();
    let root = match body_sel.as_ref().and_then(|s| document.select(s).next()) {
        Some(el) => el,
        None => return 0,
    };

    root.text()
        .flat_map(|t| t.split_whitespace())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title() {
        let html = "<html><head><title>Hello World</title></head><body></body></html>";
        let meta = extract(html);
        assert_eq!(meta.title.as_deref(), Some("Hello World"));
    }

    #[test]
    fn extracts_og_tags() {
        let html = r#"
        <html><head>
        <meta property="og:title" content="OG Title">
        <meta property="og:description" content="OG Desc">
        <meta property="og:image" content="https://example.com/img.png">
        </head><body></body></html>"#;

        let meta = extract(html);
        assert_eq!(meta.og_title.as_deref(), Some("OG Title"));
        assert_eq!(meta.og_description.as_deref(), Some("OG Desc"));
        assert_eq!(meta.og_image.as_deref(), Some("https://example.com/img.png"));
    }

    #[test]
    fn empty_html_returns_defaults() {
        let meta = extract("");
        assert!(meta.title.is_none());
        assert_eq!(meta.word_count, 0);
    }
}
