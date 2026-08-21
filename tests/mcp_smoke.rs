//! Smoke: the real neuron-mcp binary driven by a real MCP client over
//! stdio. Thin-adapter logic lives below the seam; this proves the wire.

use rmcp::model::CallToolRequestParams;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};
use tokio::process::Command;

fn args_of(value: Value) -> Map<String, Value> {
    value.as_object().expect("object args").clone()
}

#[tokio::test]
async fn tools_list_and_write_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("neurons.db");

    let client = ()
        .serve(
            TokioChildProcess::new(Command::new(env!("CARGO_BIN_EXE_neuron-mcp")).configure(
                |cmd| {
                    cmd.env("NEURON_DB", &db);
                },
            ))
            .unwrap(),
        )
        .await
        .expect("client connects to spawned server");

    let tools = client.list_all_tools().await.expect("tools list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "summary", "show", "search", "path", "list", "new_graph", "add_node", "link",
        "reinforce", "supersede", "set_stage", "park", "unpark", "settle", "reopen",
        "consolidate",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}: {names:?}");
    }

    let call = |name: &'static str, args: Value| {
        let params = CallToolRequestParams::new(name).with_arguments(args_of(args));
        client.call_tool(params)
    };

    call("new_graph", json!({"graph": "smoke", "title": "Smoke test"}))
        .await
        .expect("new_graph");
    call(
        "add_node",
        json!({"graph": "smoke", "id": "a", "kind": "idea", "title": "First thought"}),
    )
    .await
    .expect("add_node");
    call(
        "add_node",
        json!({"graph": "smoke", "id": "b", "kind": "question", "title": "Second thought"}),
    )
    .await
    .expect("add_node b");
    call("link", json!({"graph": "smoke", "from": "a", "to": "b", "label": "raised"}))
        .await
        .expect("link");

    let summary = call("summary", json!({"graph": "smoke"})).await.expect("summary");
    let text = serde_json::to_string(&summary).unwrap();
    assert!(text.contains("First thought"), "summary carries the thought: {text}");

    let missing = call("reinforce", json!({"graph": "smoke", "id": "ghost"})).await;
    match missing {
        Ok(result) => assert_eq!(
            result.is_error,
            Some(true),
            "ghost reinforce must surface as a tool error"
        ),
        Err(_) => {}
    }

    client.cancel().await.ok();
}
