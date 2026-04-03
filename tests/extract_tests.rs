use exfetch::extract::{markdown, metadata, readability};
use exfetch::output::text;
use std::fs;

fn load_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{}", name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path, e))
}

#[test]
fn test_extract_article_content() {
    let html = load_fixture("wikipedia_sample.html");
    let text = readability::extract(&html);

    // Should contain main content
    assert!(
        text.contains("Rust is a multi-paradigm"),
        "Expected article text about Rust, got: {}",
        text
    );
    assert!(
        text.contains("ownership system"),
        "Expected mention of ownership system, got: {}",
        text
    );

    // Should NOT contain nav, sidebar, or footer junk
    assert!(
        !text.contains("Main page"),
        "Nav content should be stripped, got: {}",
        text
    );
    assert!(
        !text.contains("Navigation"),
        "Sidebar content should be stripped, got: {}",
        text
    );
    assert!(
        !text.contains("last edited"),
        "Footer content should be stripped, got: {}",
        text
    );
}

#[test]
fn test_extract_metadata() {
    let html = load_fixture("wikipedia_sample.html");
    let meta = metadata::extract(&html);

    assert_eq!(
        meta.title.as_deref(),
        Some("Rust (programming language) - Wikipedia")
    );
    assert_eq!(
        meta.description.as_deref(),
        Some("Rust is a multi-paradigm programming language.")
    );
    assert_eq!(
        meta.og_title.as_deref(),
        Some("Rust (programming language)")
    );
}

#[test]
fn test_extract_empty_html_returns_fallback() {
    // Empty body should not crash
    let text = readability::extract("<html><body></body></html>");
    // Just ensure it returns without panicking; content can be empty.
    assert!(text.len() < 10000);

    let meta = metadata::extract("<html><body></body></html>");
    assert!(meta.title.is_none());
    assert_eq!(meta.word_count, 0);

    // Completely empty string
    let text2 = readability::extract("");
    assert!(text2.len() < 10000);

    let meta2 = metadata::extract("");
    assert!(meta2.title.is_none());
}

#[test]
fn test_truncation_at_word_boundary() {
    let input = "The quick brown fox jumps over the lazy dog";
    // Limit to 18 chars — should cut at a word boundary before or at 18
    let out = text::format_raw(input, Some(18));
    assert!(
        out.contains("[truncated at"),
        "should show truncation marker, got: {}",
        out
    );
    // The truncated portion should not end mid-word
    let first_line = out.lines().next().unwrap();
    assert!(
        first_line.ends_with(' ') || first_line == "The quick brown" || !first_line.contains("fo"),
        "should truncate at a word boundary, got: {}",
        first_line
    );
    assert!(
        out.contains(&format!("full content {} chars", input.len())),
        "should report full content length, got: {}",
        out
    );
}

#[test]
fn test_readability_fallback_on_empty_extraction() {
    // HTML where content selectors would yield empty but body has text
    let html = r#"<html><body><div class="weird-layout">
        <span>Some important content here</span>
    </div></body></html>"#;
    let text = readability::extract(html);
    // Should get some content via fallback even if no main/article found
    assert!(
        !text.is_empty(),
        "readability should fall back to full-body extraction"
    );
    assert!(
        text.contains("important content"),
        "should contain body text, got: {}",
        text
    );
}

#[test]
fn test_markdown_preserves_headers_and_links() {
    let html = load_fixture("article_with_formatting.html");
    let md = markdown::to_markdown(&html);

    // Headers should survive
    assert!(
        md.contains("Guide to Rust"),
        "should preserve h1 heading, got: {}",
        md
    );
    assert!(
        md.contains("Code Example"),
        "should preserve h2 heading, got: {}",
        md
    );
    assert!(
        md.contains("Resources"),
        "should preserve h2 heading, got: {}",
        md
    );

    // Links should survive (text and URL)
    assert!(
        md.contains("Official docs"),
        "should preserve link text, got: {}",
        md
    );
    assert!(
        md.contains("https://doc.rust-lang.org"),
        "should preserve link URL, got: {}",
        md
    );
    assert!(
        md.contains("Crate registry"),
        "should preserve link text, got: {}",
        md
    );
    assert!(
        md.contains("https://crates.io"),
        "should preserve link URL, got: {}",
        md
    );
}
