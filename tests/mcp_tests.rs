use exfetch::mcp::tools::get_tool_definitions;
use exfetch::mcp::types::JsonRpcResponse;

#[test]
fn test_tool_definitions_valid() {
    let tools = get_tool_definitions();
    assert_eq!(tools.len(), 4, "expected 4 tool definitions");

    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"fetch_page"));
    assert!(names.contains(&"search_web"));
    assert!(names.contains(&"browser_action"));
    assert!(names.contains(&"connection_status"));

    for tool in &tools {
        // Every tool must have a non-empty name and description
        assert!(!tool.name.is_empty(), "tool name must not be empty");
        assert!(
            !tool.description.is_empty(),
            "tool '{}' must have a description",
            tool.name
        );

        // inputSchema must be an object with a "type" field
        assert_eq!(
            tool.input_schema.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "tool '{}' inputSchema must have type: object",
            tool.name
        );

        // inputSchema must have a "properties" field that is an object
        assert!(
            tool.input_schema.get("properties").map_or(false, |v| v.is_object()),
            "tool '{}' inputSchema must have a properties object",
            tool.name
        );
    }

    // fetch_page and search_web must have required fields
    let fetch_tool = tools.iter().find(|t| t.name == "fetch_page").unwrap();
    let required = fetch_tool
        .input_schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("fetch_page must have required array");
    assert!(required.iter().any(|v| v.as_str() == Some("url")));

    let search_tool = tools.iter().find(|t| t.name == "search_web").unwrap();
    let required = search_tool
        .input_schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("search_web must have required array");
    assert!(required.iter().any(|v| v.as_str() == Some("query")));

    let action_tool = tools.iter().find(|t| t.name == "browser_action").unwrap();
    let required = action_tool
        .input_schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("browser_action must have required array");
    assert!(required.iter().any(|v| v.as_str() == Some("action")));
}

#[test]
fn test_json_rpc_response_serialization() {
    // Test success response
    let success = JsonRpcResponse::success(
        serde_json::json!(1),
        serde_json::json!({"status": "ok"}),
    );
    let json = serde_json::to_value(&success).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
    assert!(json.get("result").is_some());
    assert!(json.get("error").is_none(), "success response must not have error field");

    // Test error response
    let error = JsonRpcResponse::error(
        serde_json::json!(2),
        -32601,
        "Method not found",
    );
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 2);
    assert!(json.get("result").is_none(), "error response must not have result field");
    let err = json.get("error").expect("error response must have error field");
    assert_eq!(err["code"], -32601);
    assert_eq!(err["message"], "Method not found");
}

#[test]
fn test_json_rpc_response_null_id() {
    let resp = JsonRpcResponse::error(serde_json::Value::Null, -32700, "Parse error");
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json["id"].is_null());
    assert_eq!(json["error"]["code"], -32700);
}
