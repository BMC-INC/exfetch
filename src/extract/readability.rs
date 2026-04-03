use scraper::{Html, Selector};

/// Content selectors tried in priority order.
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

/// Selectors for junk elements to strip before conversion.
const JUNK_SELECTORS: &[&str] = &[
    "nav",
    "footer",
    "header",
    "aside",
    ".sidebar",
    "#sidebar",
    ".nav",
    ".footer",
    ".header",
    ".menu",
    ".ad",
    ".advertisement",
    "script",
    "style",
    "noscript",
];

/// Extract the main readable content from an HTML document.
///
/// Tries a series of well-known content selectors, strips junk elements,
/// then converts the remaining HTML to plain text via `html2text`.
/// Falls back to `<body>` if no content selector matches.
pub fn extract(html: &str) -> String {
    let document = Html::parse_document(html);

    // Find the best content root.
    let content_html = find_content_html(&document);

    // Re-parse the fragment so we can strip junk from it.
    let fragment = Html::parse_fragment(&content_html);
    let cleaned = strip_junk(&fragment);

    // Convert to plain text.
    let plain = html2text::from_read(cleaned.as_bytes(), 80);
    let trimmed = plain.trim().to_string();

    // If extraction returned empty/whitespace, fall back to html2text on full body
    if trimmed.is_empty() {
        let fallback = html2text::from_read(html.as_bytes(), 80);
        return fallback.trim().to_string();
    }

    trimmed
}

/// Return the inner HTML of the first matching content selector,
/// or fall back to the body's inner HTML, or the whole document.
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

    // Last resort: entire document
    document.root_element().inner_html()
}

/// Remove junk elements from a parsed fragment and return the cleaned HTML string.
fn strip_junk(fragment: &Html) -> String {
    let mut html = fragment.root_element().inner_html();

    for sel_str in JUNK_SELECTORS {
        // Re-parse each iteration because removing nodes changes the tree.
        let doc = Html::parse_fragment(&html);
        if let Ok(sel) = Selector::parse(sel_str) {
            for el in doc.select(&sel) {
                let outer = el.html();
                html = html.replace(&outer, "");
            }
        }
    }

    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_html_does_not_panic() {
        let result = extract("");
        // Should return something (possibly empty) but not panic.
        assert!(result.len() < 10000);
    }

    #[test]
    fn strips_nav_and_footer() {
        let html = r#"
        <html><body>
        <nav><ul><li>Menu</li></ul></nav>
        <main><p>Hello world</p></main>
        <footer><p>Copyright</p></footer>
        </body></html>"#;

        let text = extract(html);
        assert!(text.contains("Hello world"));
        assert!(!text.contains("Menu"));
        assert!(!text.contains("Copyright"));
    }
}
