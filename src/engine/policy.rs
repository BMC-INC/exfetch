use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::command::BrowserActionType;

/// Result of a policy check.
#[derive(Debug, PartialEq)]
pub enum PolicyVerdict {
    /// The operation is allowed.
    Allow,
    /// The operation is blocked, with a reason.
    Block(String),
}

/// Enforces security policies for browser actions and requests.
pub struct PolicyEngine {
    /// If Some, only these domains are allowed. If None, all domains are allowed.
    pub allowed_domains: Option<Vec<String>>,
    /// Domains explicitly blocked (supports wildcards like "*.example.com").
    pub blocked_domains: Vec<String>,
    /// Actions that are permitted. Actions NOT in this set are blocked.
    pub allowed_actions: HashSet<String>,
    /// Whether cookie reading is allowed.
    pub allow_cookie_read: bool,
    /// Maximum requests per minute (rate limiting).
    pub max_requests_per_minute: u32,
    request_count: AtomicU32,
    last_reset: Mutex<Instant>,
}

impl PolicyEngine {
    /// Create a new policy engine with sensible defaults.
    ///
    /// Defaults:
    /// - All domains allowed (no allowlist)
    /// - No blocked domains
    /// - execute_js is NOT in the allowed actions set
    /// - get_cookies blocked (allow_cookie_read = false)
    /// - 60 requests per minute
    pub fn new() -> Self {
        let mut allowed_actions = HashSet::new();
        // Allow all browser actions except execute_js by default
        allowed_actions.insert("click".to_string());
        allowed_actions.insert("type_text".to_string());
        allowed_actions.insert("read_dom".to_string());
        allowed_actions.insert("screenshot".to_string());
        allowed_actions.insert("navigate".to_string());
        allowed_actions.insert("list_tabs".to_string());
        allowed_actions.insert("switch_tab".to_string());
        // NOTE: execute_js and get_cookies are intentionally NOT included

        Self {
            allowed_domains: None,
            blocked_domains: Vec::new(),
            allowed_actions,
            allow_cookie_read: false,
            max_requests_per_minute: 60,
            request_count: AtomicU32::new(0),
            last_reset: Mutex::new(Instant::now()),
        }
    }

    /// Check whether a URL's domain is allowed.
    pub fn check_domain(&self, url: &str) -> PolicyVerdict {
        let domain = extract_domain(url);
        let domain = match domain {
            Some(d) => d,
            None => return PolicyVerdict::Block(format!("cannot parse domain from URL: {}", url)),
        };

        // Check blocked list (with wildcard support)
        for pattern in &self.blocked_domains {
            if domain_matches(&domain, pattern) {
                return PolicyVerdict::Block(format!("domain {} is blocked by policy", domain));
            }
        }

        // Check allowed list (if set)
        if let Some(ref allowed) = self.allowed_domains {
            if !allowed.iter().any(|a| domain_matches(&domain, a)) {
                return PolicyVerdict::Block(format!(
                    "domain {} is not in the allowed domains list",
                    domain
                ));
            }
        }

        PolicyVerdict::Allow
    }

    /// Check whether a browser action is allowed.
    pub fn check_action(&self, action: &BrowserActionType) -> PolicyVerdict {
        // Special handling for get_cookies
        if *action == BrowserActionType::GetCookies {
            if !self.allow_cookie_read {
                return PolicyVerdict::Block("cookie reading is disabled by policy".to_string());
            }
            return PolicyVerdict::Allow;
        }

        let action_str = action.as_str();
        if self.allowed_actions.contains(action_str) {
            PolicyVerdict::Allow
        } else {
            PolicyVerdict::Block(format!("action '{}' is not allowed by policy", action_str))
        }
    }

    /// Check whether the rate limit has been exceeded.
    pub fn check_rate_limit(&self) -> PolicyVerdict {
        self.maybe_reset_counter();
        let count = self.request_count.load(Ordering::Relaxed);
        if count >= self.max_requests_per_minute {
            PolicyVerdict::Block(format!(
                "rate limit exceeded ({} requests/minute)",
                self.max_requests_per_minute
            ))
        } else {
            PolicyVerdict::Allow
        }
    }

    /// Record a request (increments the rate limiter counter).
    pub fn record_request(&self) {
        self.maybe_reset_counter();
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    fn maybe_reset_counter(&self) {
        let mut last = self.last_reset.lock().unwrap();
        if last.elapsed() >= std::time::Duration::from_secs(60) {
            self.request_count.store(0, Ordering::Relaxed);
            *last = Instant::now();
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the domain (host) from a URL string.
fn extract_domain(url: &str) -> Option<String> {
    // Handle URLs without scheme
    let normalized = if !url.contains("://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    url::Url::parse(&normalized)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
}

/// Check if a domain matches a pattern. Supports wildcard prefix "*.".
fn domain_matches(domain: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // Wildcard: matches the suffix itself and any subdomain
        domain == suffix || domain.ends_with(&format!(".{}", suffix))
    } else {
        domain == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_domain_matching() {
        assert!(domain_matches("sub.example.com", "*.example.com"));
        assert!(domain_matches("deep.sub.example.com", "*.example.com"));
        assert!(domain_matches("example.com", "*.example.com"));
        assert!(!domain_matches("notexample.com", "*.example.com"));
    }

    #[test]
    fn test_exact_domain_matching() {
        assert!(domain_matches("example.com", "example.com"));
        assert!(!domain_matches("sub.example.com", "example.com"));
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://www.example.com/path"),
            Some("www.example.com".to_string())
        );
        assert_eq!(
            extract_domain("http://example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_domain("example.com/page"),
            Some("example.com".to_string())
        );
    }
}
