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
    pub cookie_allowed_domains: Vec<String>,
    /// Maximum requests per minute (rate limiting).
    pub max_requests_per_minute: u32,
    pub log_all: bool,
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
            cookie_allowed_domains: Vec::new(),
            max_requests_per_minute: 60,
            log_all: true,
            request_count: AtomicU32::new(0),
            last_reset: Mutex::new(Instant::now()),
        }
    }

    pub fn from_config(config: &crate::config::PolicyConfig) -> Self {
        Self {
            blocked_domains: config
                .blocked_domains
                .iter()
                .map(|domain| domain.to_lowercase())
                .collect(),
            allowed_actions: config.allowed_actions.iter().cloned().collect(),
            allow_cookie_read: config.allow_cookie_read,
            cookie_allowed_domains: config
                .cookie_allowed_domains
                .iter()
                .map(|domain| domain.to_lowercase())
                .collect(),
            max_requests_per_minute: config.max_requests_per_minute,
            log_all: config.log_all,
            ..Self::new()
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

    pub fn check_cookie_url(&self, url: &str) -> PolicyVerdict {
        if !self.allow_cookie_read {
            return PolicyVerdict::Block("cookie reading is disabled by policy".into());
        }
        let Some(domain) = extract_domain(url) else {
            return PolicyVerdict::Block("cookie reading requires an explicit URL".into());
        };
        if !self
            .cookie_allowed_domains
            .iter()
            .any(|pattern| domain_matches(&domain, pattern))
        {
            return PolicyVerdict::Block(format!(
                "cookie domain {} is not in cookie_allowed_domains",
                domain
            ));
        }
        self.check_domain(url)
    }

    pub fn has_domain_restrictions(&self) -> bool {
        !self.blocked_domains.is_empty() || self.allowed_domains.is_some()
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
        let count = self.request_count.fetch_add(1, Ordering::Relaxed) + 1;
        if self.log_all {
            tracing::info!(request_count = count, "request accepted under local policy");
        }
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
        .and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_lowercase()))
}

/// Check if a domain matches a pattern. Supports wildcard prefix "*.".
fn domain_matches(domain: &str, pattern: &str) -> bool {
    let domain = domain.trim_end_matches('.').to_lowercase();
    let pattern = pattern.trim_end_matches('.').to_lowercase();
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
    fn cookie_configuration_requires_a_matching_allowlist() {
        let mut config = crate::config::PolicyConfig {
            allow_cookie_read: true,
            cookie_allowed_domains: vec!["*.example.com".into()],
            ..Default::default()
        };
        let policy = PolicyEngine::from_config(&config);
        assert_eq!(
            policy.check_cookie_url("https://sub.example.com"),
            PolicyVerdict::Allow
        );
        assert!(matches!(
            policy.check_cookie_url("https://other.example"),
            PolicyVerdict::Block(_)
        ));
        config.cookie_allowed_domains.clear();
        assert!(matches!(
            PolicyEngine::from_config(&config).check_cookie_url("https://example.com"),
            PolicyVerdict::Block(_)
        ));
    }

    #[test]
    fn terminal_dns_dots_do_not_bypass_domain_policy() {
        let mut policy = PolicyEngine::new();
        policy.blocked_domains = vec!["blocked.example".into()];
        assert!(matches!(
            policy.check_domain("https://blocked.example./"),
            PolicyVerdict::Block(_)
        ));
        assert!(domain_matches("sub.example.com.", "*.EXAMPLE.COM."));
        policy.allow_cookie_read = true;
        policy.cookie_allowed_domains = vec!["allowed.example.".into()];
        assert_eq!(
            policy.check_cookie_url("https://allowed.example/"),
            PolicyVerdict::Allow
        );
    }

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
