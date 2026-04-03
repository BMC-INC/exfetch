use std::time::Duration;

use exfetch::fetch::http::{fetch_url, fetch_url_with_options, FetchError};
use exfetch::fetch::pdf::{extract_text, PdfError};

#[tokio::test]
async fn test_fetch_success_returns_body() {
    let resp = fetch_url(
        "https://httpbin.org/html",
        Duration::from_secs(15),
        "exfetch-test/0.1",
    )
    .await
    .expect("fetch should succeed");

    assert_eq!(resp.status, 200);
    assert!(
        resp.body.contains("Herman Melville"),
        "body should contain 'Herman Melville'"
    );
}

#[tokio::test]
async fn test_fetch_404_returns_error() {
    let result = fetch_url(
        "https://httpbin.org/status/404",
        Duration::from_secs(15),
        "exfetch-test/0.1",
    )
    .await;

    assert!(result.is_err(), "404 should return an error");
    match result.unwrap_err() {
        FetchError::HttpError { status, .. } => assert_eq!(status, 404),
        other => panic!("expected HttpError, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fetch_auto_prepends_https() {
    let resp = fetch_url(
        "httpbin.org/html",
        Duration::from_secs(15),
        "exfetch-test/0.1",
    )
    .await
    .expect("fetch with auto-prepended https should succeed");

    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("Herman Melville"));
}

#[tokio::test]
async fn test_fetch_timeout() {
    let result = fetch_url(
        "https://httpbin.org/delay/5",
        Duration::from_secs(1),
        "exfetch-test/0.1",
    )
    .await;

    assert!(result.is_err(), "should time out");
    match result.unwrap_err() {
        FetchError::Timeout { .. } => {} // expected
        other => panic!("expected Timeout, got: {:?}", other),
    }
}

// ─── SSRF protection tests ──────────────────────────────────────────

#[tokio::test]
async fn test_ssrf_blocks_localhost() {
    let result = fetch_url_with_options(
        "http://127.0.0.1:8080",
        Duration::from_secs(5),
        "exfetch-test/0.1",
        false,
    )
    .await;

    assert!(
        result.is_err(),
        "localhost should be blocked by SSRF protection"
    );
    match result.unwrap_err() {
        FetchError::SsrfBlocked { .. } => {} // expected
        other => panic!("expected SsrfBlocked, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_ssrf_blocks_private_ip() {
    let result = fetch_url_with_options(
        "http://192.168.1.1",
        Duration::from_secs(5),
        "exfetch-test/0.1",
        false,
    )
    .await;

    assert!(
        result.is_err(),
        "private IP should be blocked by SSRF protection"
    );
    match result.unwrap_err() {
        FetchError::SsrfBlocked { .. } => {} // expected
        other => panic!("expected SsrfBlocked, got: {:?}", other),
    }
}

// ─── PDF extraction tests ───────────────────────────────────────────

#[test]
fn test_pdf_extraction() {
    use lopdf::dictionary;
    use lopdf::{Document, Object, Stream};

    let mut doc = Document::with_version("1.4");

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let content = b"BT /F1 12 Tf 100 700 Td (Hello from exfetch PDF test) Tj ET";
    let content_stream = Stream::new(dictionary! {}, content.to_vec());
    let content_id = doc.add_object(content_stream);

    let resources = dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    };

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "Resources" => resources,
    });

    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    });

    if let Ok(page) = doc.get_object_mut(page_id) {
        if let Object::Dictionary(ref mut dict) = page {
            dict.set("Parent", pages_id);
        }
    }

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });

    doc.trailer.set("Root", catalog_id);

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("failed to save test PDF");

    let text = extract_text(&buf).expect("should extract text from valid PDF");
    assert!(!text.is_empty(), "extracted text should not be empty");
    assert!(
        text.contains("Hello from exfetch PDF test"),
        "expected test text in extracted content, got: {:?}",
        text
    );
}

#[test]
fn test_pdf_invalid_bytes() {
    let garbage = b"not a valid pdf file at all";
    let result = extract_text(garbage);
    assert!(result.is_err(), "garbage bytes should produce an error");
    match result.unwrap_err() {
        PdfError::ParseError(_) => {} // expected
        other => panic!("expected ParseError, got: {:?}", other),
    }
}
