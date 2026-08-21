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

fn outcome_of(r: &rmcp::model::CallToolResult) -> Value {
    match &r.structured_content {
        Some(v) => v.clone(),
        None => {
            let text = r.content.first().and_then(|c| c.as_text()).expect("text block");
            serde_json::from_str(&text.text).expect("outcome json")
        }
    }
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
        "summary", "show", "search", "path", "list", "new_graph", "add_node", "add_nodes",
        "link", "reinforce", "supersede", "set_stage", "park", "unpark", "settle",
        "reopen", "consolidate",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}: {names:?}");
    }

    let call = |name: &'static str, args: Value| {
        let params = CallToolRequestParams::new(name).with_arguments(args_of(args));
        client.call_tool(params)
    };

    let created = call("new_graph", json!({"graph": "smoke", "title": "Smoke test"}))
        .await
        .expect("new_graph");
    assert!(
        !matches!(created.structured_content, Some(Value::String(_))),
        "write results must not carry string structuredContent (defect #22)"
    );
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
    if let Ok(result) = missing {
        assert_eq!(
            result.is_error,
            Some(true),
            "ghost reinforce must surface as a tool error"
        );
    }

    let ok_text = |r: rmcp::model::CallToolResult, tool: &str| {
        assert_ne!(r.is_error, Some(true), "{tool} must succeed");
        serde_json::to_string(&r).unwrap()
    };

    let r = call("reinforce", json!({"graph": "smoke", "id": "a"})).await.unwrap();
    ok_text(r, "reinforce");
    let r = call("set_stage", json!({"graph": "smoke", "id": "a", "stage": "grilled"}))
        .await
        .unwrap();
    ok_text(r, "set_stage");

    let r = call("show", json!({"graph": "smoke", "node": "a"})).await.unwrap();
    let shown = ok_text(r, "show");
    assert!(shown.contains("Second thought"), "show walks a -> b: {shown}");

    let r = call("path", json!({"graph": "smoke", "from": "a", "to": "b"})).await.unwrap();
    let p = ok_text(r, "path");
    assert!(p.contains("\"a\"") && p.contains("\"b\""), "path a->b found: {p}");

    let r = call(
        "add_node",
        json!({"graph": "smoke", "id": "c", "kind": "idea", "title": "Replacement thought"}),
    )
    .await
    .unwrap();
    ok_text(r, "add_node c");
    let r = call("supersede", json!({"graph": "smoke", "old": "b", "by": "c"})).await.unwrap();
    ok_text(r, "supersede");

    let r = call("park", json!({"graph": "smoke", "id": "c"})).await.unwrap();
    ok_text(r, "park");
    let r = call("unpark", json!({"graph": "smoke", "id": "c"})).await.unwrap();
    ok_text(r, "unpark");

    let r = call("consolidate", json!({})).await.unwrap();
    ok_text(r, "consolidate");
    let r = call("search", json!({"query": "thought"})).await.unwrap();
    let hits = ok_text(r, "search");
    assert!(hits.contains("smoke"), "consolidated thoughts findable: {hits}");

    let r = call("list", json!({"status": "active"})).await.unwrap();
    assert!(
        matches!(&r.structured_content, Some(Value::Object(_)) | None),
        "read results must carry object structuredContent (defect #25)"
    );
    let listed = ok_text(r, "list");
    assert!(listed.contains("Smoke test"), "list sees the graph: {listed}");
    let r = call("list", json!({"status": "bogus"})).await;
    if let Ok(result) = r {
        assert_eq!(result.is_error, Some(true), "bogus status filter is a tool error");
    }

    let r = call("settle", json!({"graph": "smoke"})).await.unwrap();
    ok_text(r, "settle");
    let r = call("reopen", json!({"graph": "smoke"})).await.unwrap();
    ok_text(r, "reopen");

    let r = call(
        "add_nodes",
        json!({
            "graph": "smoke",
            "nodes": [
                {"id": "d", "kind": "idea", "title": "Batch root"},
                {"id": "e", "kind": "idea", "title": "Batch branch"},
                {"id": "f", "kind": "idea", "title": "Batch leaf"},
            ],
            "links": [
                {"from": "d", "to": "e", "label": "spawns"},
                {"from": "e", "to": "f", "label": "spawns"},
            ],
        }),
    )
    .await
    .unwrap();
    assert_ne!(r.is_error, Some(true), "add_nodes must succeed");
    assert!(
        matches!(&r.structured_content, Some(Value::Object(_)) | None),
        "add_nodes carries object structuredContent"
    );
    let batch = outcome_of(&r);
    assert_eq!(batch["applied_nodes"], json!(3), "three nodes applied: {batch}");
    assert_eq!(batch["applied_links"], json!(2), "two links applied: {batch}");
    assert_eq!(batch["failed"], Value::Null, "clean batch reports no failure: {batch}");
    let r = call("summary", json!({"graph": "smoke", "limit": 10})).await.unwrap();
    let after = ok_text(r, "summary after batch");
    assert!(after.contains("Batch root"), "batch nodes land in the graph: {after}");

    let r = call(
        "add_nodes",
        json!({
            "graph": "smoke",
            "nodes": [{"id": "g", "kind": "idea", "title": "Partial survivor"}],
            "links": [
                {"from": "g", "to": "ghost", "label": "haunts"},
                {"from": "g", "to": "d", "label": "never applied"},
            ],
        }),
    )
    .await
    .unwrap();
    assert_ne!(r.is_error, Some(true), "partial batch is a report, not a tool error");
    assert!(
        matches!(&r.structured_content, Some(Value::Object(_)) | None),
        "partial add_nodes carries object structuredContent"
    );
    let batch = outcome_of(&r);
    assert_eq!(batch["applied_nodes"], json!(1), "the valid node applied: {batch}");
    assert_eq!(batch["applied_links"], json!(0), "stopped at the bad edge: {batch}");
    assert_eq!(batch["failed"]["kind"], json!("link"), "failure names its kind: {batch}");
    assert_eq!(batch["failed"]["edge"]["to"], json!("ghost"), "failure names the edge: {batch}");
    let r = call("summary", json!({"graph": "smoke", "limit": 10})).await.unwrap();
    let after = ok_text(r, "summary after partial batch");
    assert!(after.contains("Partial survivor"), "the valid prefix landed: {after}");

    client.cancel().await.ok();
}
