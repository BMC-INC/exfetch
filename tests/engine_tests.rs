use exfetch::bridge::connection::ConnectionManager;
use exfetch::engine::command::{BrowserActionType, Command};
use exfetch::engine::policy::{PolicyEngine, PolicyVerdict};
use exfetch::engine::router::{Route, Router};
use exfetch::fetch::http::is_private_ip;

#[tokio::test]
async fn test_router_uses_http_when_no_extension() {
    let connections = ConnectionManager::new();
    let router = Router::new(connections);

    let cmd = Command::FetchPage {
        url: "https://example.com".to_string(),
        use_browser: false,
    };
    let route = router.determine_route(&cmd).await;
    assert_eq!(route, Route::Http);

    // Even with use_browser=true, should fall back to Http when no connections
    let cmd2 = Command::FetchPage {
        url: "https://example.com".to_string(),
        use_browser: true,
    };
    let route2 = router.determine_route(&cmd2).await;
    assert_eq!(route2, Route::Http);
}

#[test]
fn test_policy_blocks_domain() {
    let mut policy = PolicyEngine::new();
    policy.blocked_domains.push("*.evil.com".to_string());

    assert_eq!(
        policy.check_domain("https://malware.evil.com/payload"),
        PolicyVerdict::Block("domain malware.evil.com is blocked by policy".to_string())
    );
    assert_eq!(
        policy.check_domain("https://example.com"),
        PolicyVerdict::Allow
    );
}

#[test]
fn test_policy_blocks_execute_js_by_default() {
    let policy = PolicyEngine::new();

    assert_eq!(
        policy.check_action(&BrowserActionType::ExecuteJs),
        PolicyVerdict::Block("action 'execute_js' is not allowed by policy".to_string())
    );
    assert_eq!(
        policy.check_action(&BrowserActionType::Click),
        PolicyVerdict::Allow
    );
}

#[test]
fn test_policy_rate_limiting() {
    let mut policy = PolicyEngine::new();
    policy.max_requests_per_minute = 3;

    // First 3 should be allowed
    for _ in 0..3 {
        assert_eq!(policy.check_rate_limit(), PolicyVerdict::Allow);
        policy.record_request();
    }

    // 4th should be blocked
    assert!(matches!(policy.check_rate_limit(), PolicyVerdict::Block(_)));
}

#[test]
fn test_policy_blocks_cookies_by_default() {
    let policy = PolicyEngine::new();

    assert_eq!(
        policy.check_action(&BrowserActionType::GetCookies),
        PolicyVerdict::Block("cookie reading is disabled by policy".to_string())
    );

    // With cookie reading enabled, should allow
    let mut policy2 = PolicyEngine::new();
    policy2.allow_cookie_read = true;
    assert_eq!(
        policy2.check_action(&BrowserActionType::GetCookies),
        PolicyVerdict::Allow
    );
}

// ─── Router: browser action requires extension ──────────────────────

#[tokio::test]
async fn test_router_browser_action_errors_without_extension() {
    let connections = ConnectionManager::new();
    let router = Router::new(connections);

    let cmd = Command::BrowserAction {
        action: BrowserActionType::Click,
        selector: Some("button".to_string()),
        text: None,
        url: None,
        tab_id: None,
        code: None,
        full_page: false,
    };
    let route = router.determine_route(&cmd).await;
    assert!(matches!(route, Route::Error(_)));
}

#[tokio::test]
async fn test_router_search_always_http() {
    let connections = ConnectionManager::new();
    let router = Router::new(connections);

    let cmd = Command::SearchWeb {
        query: "test".to_string(),
        num_results: 5,
        fetch_results: false,
    };
    let route = router.determine_route(&cmd).await;
    assert_eq!(route, Route::Http);
}

#[tokio::test]
async fn test_router_connection_status_always_http() {
    let connections = ConnectionManager::new();
    let router = Router::new(connections);

    let cmd = Command::ConnectionStatus;
    let route = router.determine_route(&cmd).await;
    assert_eq!(route, Route::Http);
}

// ─── SSRF: is_private_ip integration tests ──────────────────────────

#[test]
fn test_is_private_ip_comprehensive() {
    // Private ranges
    assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
    assert!(is_private_ip(&"10.255.0.1".parse().unwrap()));
    assert!(is_private_ip(&"172.20.0.1".parse().unwrap()));
    assert!(is_private_ip(&"192.168.0.1".parse().unwrap()));
    assert!(is_private_ip(&"169.254.100.1".parse().unwrap()));
    assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
    assert!(is_private_ip(&"::1".parse().unwrap()));
    assert!(is_private_ip(&"fe80::1".parse().unwrap()));
    assert!(is_private_ip(&"fd12::1".parse().unwrap()));

    // Public ranges
    assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
    assert!(!is_private_ip(&"93.184.216.34".parse().unwrap()));
    assert!(!is_private_ip(&"2606:4700::1111".parse().unwrap()));
}

// ─── Policy: domain allowlist ───────────────────────────────────────

#[test]
fn test_policy_allows_domain_when_no_allowlist() {
    let policy = PolicyEngine::new();
    assert_eq!(
        policy.check_domain("https://any-domain.example.com"),
        PolicyVerdict::Allow
    );
}

#[test]
fn test_policy_blocks_domain_not_in_allowlist() {
    let mut policy = PolicyEngine::new();
    policy.allowed_domains = Some(vec!["example.com".to_string()]);

    assert_eq!(
        policy.check_domain("https://other.com"),
        PolicyVerdict::Block("domain other.com is not in the allowed domains list".to_string())
    );
    assert_eq!(
        policy.check_domain("https://example.com/page"),
        PolicyVerdict::Allow
    );
}
