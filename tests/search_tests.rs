use exfetch::search::engine::parse_ddg_html;

#[test]
fn test_parse_ddg_results() {
    let html = include_str!("fixtures/ddg_results.html");
    let results = parse_ddg_html(html);

    assert!(!results.is_empty(), "should parse at least one result");
    assert_eq!(results.len(), 3, "fixture contains 3 results");

    // First result
    assert!(results[0].title.contains("Rust Programming Language"));
    assert_eq!(results[0].url, "https://www.rust-lang.org/");
    assert!(results[0].snippet.contains("reliable and efficient"));

    // Second result
    assert!(results[1].title.contains("Rust Book"));
    assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
    assert!(results[1].snippet.contains("official guide"));

    // Third result
    assert!(results[2].title.contains("Wikipedia"));
    assert!(results[2].url.contains("wikipedia.org"));
    assert!(results[2].snippet.contains("multi-paradigm"));
}

#[test]
fn test_parse_ddg_empty_html() {
    let html = "<html><body><div>No results</div></body></html>";
    let results = parse_ddg_html(html);
    assert!(results.is_empty());
}

#[test]
fn test_parse_ddg_results_have_required_fields() {
    let html = include_str!("fixtures/ddg_results.html");
    let results = parse_ddg_html(html);

    for result in &results {
        assert!(!result.title.is_empty(), "title should not be empty");
        assert!(!result.url.is_empty(), "url should not be empty");
        assert!(
            result.url.starts_with("http"),
            "url should start with http: got {}",
            result.url
        );
    }
}
