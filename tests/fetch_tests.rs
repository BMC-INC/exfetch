use std::time::Duration;

use exfetch::fetch::http::{fetch_url, FetchError};

#[tokio::test]
async fn test_fetch_success_returns_body() {
    let resp = fetch_url("https://httpbin.org/html", Duration::from_secs(15), "exfetch-test/0.1")
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
    let result =
        fetch_url("https://httpbin.org/status/404", Duration::from_secs(15), "exfetch-test/0.1")
            .await;

    assert!(result.is_err(), "404 should return an error");
    match result.unwrap_err() {
        FetchError::HttpError { status, .. } => assert_eq!(status, 404),
        other => panic!("expected HttpError, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fetch_auto_prepends_https() {
    let resp = fetch_url("httpbin.org/html", Duration::from_secs(15), "exfetch-test/0.1")
        .await
        .expect("fetch with auto-prepended https should succeed");

    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("Herman Melville"));
}

#[tokio::test]
async fn test_fetch_timeout() {
    let result =
        fetch_url("https://httpbin.org/delay/5", Duration::from_secs(1), "exfetch-test/0.1")
            .await;

    assert!(result.is_err(), "should time out");
    match result.unwrap_err() {
        FetchError::Timeout { .. } => {} // expected
        other => panic!("expected Timeout, got: {:?}", other),
    }
}
