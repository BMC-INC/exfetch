use super::types::ToolDefinition;

/// Return the list of all MCP tool definitions exposed by exfetch.
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "fetch_page".to_string(),
            description: "Fetch a web page and extract its content in the specified format."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "markdown", "html", "json"],
                        "default": "markdown",
                        "description": "Output format for the fetched content"
                    },
                    "use_browser": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether to use browser-based fetching if available"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum content length in characters"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector to extract specific elements"
                    }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "search_web".to_string(),
            description: "Search the web and optionally fetch top result content.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "num_results": {
                        "type": "integer",
                        "default": 5,
                        "description": "Number of search results to return"
                    },
                    "fetch_results": {
                        "type": "boolean",
                        "default": false,
                        "description": "Whether to also fetch content from top results"
                    },
                    "fetch_count": {
                        "type": "integer",
                        "default": 3,
                        "description": "Number of top results to fetch content from (when fetch_results is true)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "markdown", "html", "json"],
                        "default": "markdown",
                        "description": "Output format for fetched content"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "browser_action".to_string(),
            description: "Perform a browser action via the connected Chrome extension.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "click", "type_text", "read_dom", "screenshot",
                            "navigate", "list_tabs", "switch_tab",
                            "get_cookies", "execute_js"
                        ],
                        "description": "The browser action to perform"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for the target element"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type (for type_text action)"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to (for navigate action)"
                    },
                    "tab_id": {
                        "type": "integer",
                        "description": "Tab ID (for switch_tab action)"
                    },
                    "code": {
                        "type": "string",
                        "description": "JavaScript code to execute (for execute_js action)"
                    },
                    "full_page": {
                        "type": "boolean",
                        "description": "Whether to capture the full page (for screenshot action)"
                    }
                },
                "required": ["action"]
            }),
        },
        ToolDefinition {
            name: "connection_status".to_string(),
            description: "Check whether a browser extension is connected to exfetch.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}
