use std::path::PathBuf;

use serde::Deserialize;

/// Top-level configuration loaded from the platform configuration directory.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub defaults: Defaults,
    pub policy: PolicyConfig,
    pub tabs: TabConfig,
}

/// Default settings for fetch and search operations.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Defaults {
    /// Request timeout in seconds.
    pub timeout: u64,
    /// Default User-Agent header.
    pub user_agent: String,
    /// Default search engine ("duckduckgo" or "searxng").
    pub search_engine: String,
    /// SearXNG instance URL (when search_engine is "searxng").
    pub searxng_url: Option<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            timeout: 15,
            user_agent: "exfetch/0.1".to_string(),
            search_engine: "duckduckgo".to_string(),
            searxng_url: None,
        }
    }
}

/// Security and rate-limiting policy configuration.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyConfig {
    /// Domains that are blocked from being fetched.
    pub blocked_domains: Vec<String>,
    /// Browser actions that are allowed (e.g. "navigate", "read_dom").
    pub allowed_actions: Vec<String>,
    /// Whether cookie reading is permitted.
    pub allow_cookie_read: bool,
    /// Domains for which cookie access is allowed.
    pub cookie_allowed_domains: Vec<String>,
    /// Whether all requests should be logged.
    pub log_all: bool,
    /// Maximum requests allowed per minute.
    pub max_requests_per_minute: u32,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            blocked_domains: Vec::new(),
            allowed_actions: vec![
                "click".to_string(),
                "type_text".to_string(),
                "read_dom".to_string(),
                "screenshot".to_string(),
                "navigate".to_string(),
                "list_tabs".to_string(),
                "switch_tab".to_string(),
            ],
            allow_cookie_read: false,
            cookie_allowed_domains: Vec::new(),
            log_all: true,
            max_requests_per_minute: 60,
        }
    }
}

/// Tab/session reuse configuration.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TabConfig {
    /// Strategy for reusing tabs: "exact", "domain", or "none".
    pub reuse_strategy: String,
    /// Whether new tabs open in the background.
    pub open_in_background: bool,
}

impl Default for TabConfig {
    fn default() -> Self {
        Self {
            reuse_strategy: "exact".to_string(),
            open_in_background: true,
        }
    }
}

/// Return the platform-appropriate exfetch config directory.
///
/// Uses the platform config directory (Application Support on macOS), unless
/// `EXFETCH_CONFIG_DIR` is set.
pub fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("EXFETCH_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("exfetch")
}

/// Load settings, using defaults only when no configuration file exists.
/// Invalid or unreadable settings are errors so policy is never silently reset.
pub fn load() -> anyhow::Result<Config> {
    let path = config_dir().join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).map_err(|error| {
            anyhow::anyhow!("Invalid configuration {}: {}", path.display(), error)
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(anyhow::anyhow!(
            "Cannot read configuration {}: {}",
            path.display(),
            error
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.defaults.timeout, 15);
        assert_eq!(config.defaults.search_engine, "duckduckgo");
        assert_eq!(config.policy.max_requests_per_minute, 60);
        assert!(!config.policy.allow_cookie_read);
        assert!(config.policy.log_all);
        assert_eq!(config.tabs.reuse_strategy, "exact");
        assert!(config.tabs.open_in_background);
    }

    #[test]
    fn test_parse_partial_config() {
        let toml_str = r#"
[defaults]
timeout = 30

[policy]
max_requests_per_minute = 120
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.defaults.timeout, 30);
        assert_eq!(config.policy.max_requests_per_minute, 120);
        // Other fields should use defaults
        assert_eq!(config.defaults.search_engine, "duckduckgo");
        assert!(config.tabs.open_in_background);
    }

    #[test]
    fn test_unknown_config_fields_are_rejected() {
        for contents in [
            "polciy = {}",
            "[defaults]\ntimout = 30",
            "[policy]\nblocked_domans = ['blocked.example']",
            "[policy]\nallowed_domains = ['allowed.example']",
            "[tabs]\nreuse_stategy = 'none'",
        ] {
            let error = toml::from_str::<Config>(contents).unwrap_err();
            assert!(error.to_string().contains("unknown field"), "{error}");
        }
    }

    #[test]
    fn test_config_dir_is_not_empty() {
        let dir = config_dir();
        assert!(dir.to_str().unwrap().contains("exfetch"));
    }
}
