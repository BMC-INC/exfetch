use exfetch::extract::{metadata, readability};
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
