use scraper::{Html, Selector};

/// Content selectors tried in priority order for finding the main content area.
const CONTENT_SELECTORS: &[&str] = &[
    "main",
    "article",
    "[role='main']",
    "#content",
    "#main-content",
    ".post-content",
    ".article-content",
    ".entry-content",
];

/// Convert an HTML document to Markdown-like text.
///
/// Finds the main content area using the same priority selectors as the
/// readability extractor, then converts the HTML fragment to text via
/// `html2text` which preserves headings, links, and list structure.
pub fn to_markdown(html: &str) -> String {
    let document = Html::parse_document(html);

    let content_html = find_content_html(&document);

    // html2text produces a Markdown-ish plain text representation
    let md = html2text::from_read(content_html.as_bytes(), 80);
    md.trim().to_string()
}

/// Return the inner HTML of the first matching content selector,
/// or fall back to the body or the whole document.
fn find_content_html(document: &Html) -> String {
    for sel_str in CONTENT_SELECTORS {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&sel).next() {
                return el.inner_html();
            }
        }
    }

    // Fallback: body
    if let Ok(body_sel) = Selector::parse("body") {
        if let Some(body) = document.select(&body_sel).next() {
            return body.inner_html();
        }
    }

    // Last resort
    document.root_element().inner_html()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_html_to_markdown() {
        let html = r#"<html><body><main>
            <h1>Title</h1>
            <p>Hello <strong>world</strong></p>
        </main></body></html>"#;
        let md = to_markdown(html);
        assert!(md.contains("Title"), "should contain heading text");
        assert!(md.contains("world"), "should contain body text");
    }

    #[test]
    fn preserves_links() {
        let html = r#"<html><body><main>
            <p>Visit <a href="https://example.com">Example</a></p>
        </main></body></html>"#;
        let md = to_markdown(html);
        assert!(
            md.contains("Example") && md.contains("https://example.com"),
            "should preserve link text and URL, got: {}",
            md
        );
    }
}
