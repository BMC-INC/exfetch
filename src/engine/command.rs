use serde::{Deserialize, Serialize};

/// High-level commands that the engine can execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    /// Fetch a page by URL. Optionally request browser-based fetch.
    FetchPage {
        url: String,
        #[serde(default)]
        use_browser: bool,
    },

    /// Search the web.
    SearchWeb {
        query: String,
        #[serde(default = "default_num_results")]
        num_results: u32,
        #[serde(default)]
        fetch_results: bool,
    },

    /// Perform a browser action via the extension.
    BrowserAction {
        action: BrowserActionType,
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        tab_id: Option<u64>,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        full_page: bool,
    },

    /// Check if an extension connection is available.
    ConnectionStatus,
}

fn default_num_results() -> u32 {
    5
}

/// The type of browser action to perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionType {
    Click,
    TypeText,
    ReadDom,
    Screenshot,
    Navigate,
    ListTabs,
    SwitchTab,
    GetCookies,
    ExecuteJs,
}

impl BrowserActionType {
    /// Return the string name of this action type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::TypeText => "type_text",
            Self::ReadDom => "read_dom",
            Self::Screenshot => "screenshot",
            Self::Navigate => "navigate",
            Self::ListTabs => "list_tabs",
            Self::SwitchTab => "switch_tab",
            Self::GetCookies => "get_cookies",
            Self::ExecuteJs => "execute_js",
        }
    }
}
