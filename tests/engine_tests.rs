use exfetch::bridge::connection::ConnectionManager;
use exfetch::engine::command::{BrowserActionType, Command};
use exfetch::engine::policy::{PolicyEngine, PolicyVerdict};
use exfetch::engine::router::{Route, Router};

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
    assert!(matches!(
        policy.check_rate_limit(),
        PolicyVerdict::Block(_)
    ));
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
