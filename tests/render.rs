//! Render seam: pure views over GraphData fixtures. No I/O, no server —
//! the fixtures state exactly what the text must and must not contain.

use neuron::{
    export_md, mermaid, Edge, GraphData, GraphMeta, GraphStatus, Node, NodeStatus,
};

fn meta(id: &str) -> GraphMeta {
    GraphMeta {
        id: id.into(),
        title: format!("graph {id}"),
        status: GraphStatus::Active,
        project: None,
        created: 100,
        updated: 100,
    }
}

fn node(id: &str, title: &str, status: NodeStatus) -> Node {
    Node {
        id: id.into(),
        kind: "idea".into(),
        title: title.into(),
        content: String::new(),
        status,
        stage: None,
        skills: Vec::new(),
        reinforced: 1,
        superseded_by: None,
        created: 100,
        updated: 100,
    }
}

fn edge(from: &str, to: &str, label: &str, weight: u32) -> Edge {
    Edge { from: from.into(), to: to.into(), label: label.into(), weight, created: 100 }
}

/// a -> b -> c -> d chain plus a parked e; c superseded by b.
fn fixture() -> GraphData {
    let mut c = node("c", "Gamma", NodeStatus::Superseded);
    c.superseded_by = Some("b".into());
    let mut b = node("b", "Beta", NodeStatus::Active);
    b.stage = Some("grilled".into());
    b.reinforced = 3;
    GraphData {
        meta: meta("g"),
        nodes: vec![
            node("a", "Alpha", NodeStatus::Active),
            b,
            c,
            node("d", "Delta", NodeStatus::Active),
            node("e", "Epsilon", NodeStatus::Parked),
        ],
        edges: vec![
            edge("a", "b", "raised", 3),
            edge("b", "c", "answered-by", 1),
            edge("c", "d", "spawns", 1),
        ],
    }
}

#[test]
fn whole_graph_names_every_node_and_edge() {
    let chart = mermaid(&fixture(), None, 0).unwrap();
    assert!(chart.starts_with("flowchart TD\n"), "header: {chart}");
    for id in ["a", "b", "c", "d", "e"] {
        assert!(chart.contains(&format!("    {id}[")), "node {id} present: {chart}");
    }
    for title in ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"] {
        assert!(chart.contains(title), "title {title} present: {chart}");
    }
    for label in ["answered-by", "spawns"] {
        assert!(chart.contains(&format!("|\"{label}\"|")), "label {label}: {chart}");
    }
}

#[test]
fn focus_with_depth_excludes_out_of_radius_nodes() {
    let chart = mermaid(&fixture(), Some("b"), 1).unwrap();
    for id in ["a", "b", "c"] {
        assert!(chart.contains(&format!("    {id}[")), "{id} within radius: {chart}");
    }
    assert!(!chart.contains("Delta"), "d is two hops out: {chart}");
    assert!(!chart.contains("Epsilon"), "e is disconnected: {chart}");
    assert!(!chart.contains("spawns"), "edge c->d leaves the view with d: {chart}");
    assert!(chart.contains("raised"), "edge a->b stays: {chart}");
}

#[test]
fn focus_walks_both_directions() {
    let chart = mermaid(&fixture(), Some("c"), 1).unwrap();
    assert!(chart.contains("Beta"), "incoming b->c reached: {chart}");
    assert!(chart.contains("Delta"), "outgoing c->d reached: {chart}");
    assert!(!chart.contains("Alpha"), "a is two hops upstream: {chart}");
}

#[test]
fn focus_depth_zero_is_the_focus_alone() {
    let chart = mermaid(&fixture(), Some("a"), 0).unwrap();
    assert!(chart.contains("Alpha"));
    assert!(!chart.contains("Beta"));
    assert!(!chart.contains("-->"), "no edge survives a one-node view: {chart}");
}

#[test]
fn unknown_focus_is_refused() {
    let err = mermaid(&fixture(), Some("ghost"), 2).unwrap_err();
    assert!(err.to_string().contains("ghost"), "refusal names the id: {err}");
}

#[test]
fn superseded_and_parked_are_styled_distinctly() {
    let chart = mermaid(&fixture(), None, 0).unwrap();
    assert!(chart.contains("c[\"Gamma\"]:::superseded"), "superseded class: {chart}");
    assert!(chart.contains("e[\"Epsilon\"]:::parked"), "parked class: {chart}");
    assert!(chart.contains("classDef superseded"), "superseded classDef: {chart}");
    assert!(chart.contains("classDef parked"), "parked classDef: {chart}");
    assert!(chart.contains("stroke-dasharray"), "superseded dashes: {chart}");
    assert!(!chart.contains("a[\"Alpha\"]:::"), "active carries no class: {chart}");
}

#[test]
fn all_active_graph_emits_no_classdefs() {
    let data = GraphData {
        meta: meta("g"),
        nodes: vec![node("a", "Alpha", NodeStatus::Active)],
        edges: vec![],
    };
    let chart = mermaid(&data, None, 0).unwrap();
    assert!(!chart.contains("classDef"), "no unused styling: {chart}");
}

#[test]
fn reinforced_edges_show_their_weight() {
    let chart = mermaid(&fixture(), None, 0).unwrap();
    assert!(chart.contains("|\"raised x3\"|"), "weight 3 rendered: {chart}");
    assert!(chart.contains("|\"answered-by\"|"), "weight 1 stays bare: {chart}");
    assert!(!chart.contains("x1"), "weight 1 never printed: {chart}");
}

#[test]
fn hostile_titles_are_escaped() {
    let data = GraphData {
        meta: meta("g"),
        nodes: vec![node("h", "say \"hi\" [sic] `code` {x} <b>", NodeStatus::Active)],
        edges: vec![],
    };
    let chart = mermaid(&data, None, 0).unwrap();
    let line = chart.lines().find(|l| l.contains("h[")).unwrap();
    assert_eq!(
        line,
        "    h[\"say #quot;hi#quot; #91;sic#93; #96;code#96; #123;x#125; #lt;b#gt;\"]",
        "every hostile character becomes an entity code"
    );
    assert!(!line.contains("[sic]"), "raw brackets gone: {line}");
    assert!(!line.contains('`'), "raw backticks gone: {line}");
}

#[test]
fn export_md_carries_sections_counts_and_forwarding() {
    let md = export_md(&fixture(), None, 0).unwrap();
    assert!(md.contains("```mermaid\nflowchart TD"), "markdown embeds the diagram");
    assert!(md.starts_with("# graph g\n"), "title header: {md}");
    assert!(md.contains("3 active, 1 parked, 1 superseded"), "counts line: {md}");
    assert!(md.contains("## Active"), "active section: {md}");
    assert!(md.contains("## Parked"), "parked section: {md}");
    assert!(md.contains("## Superseded"), "superseded section: {md}");
    assert!(
        md.contains("- Beta (idea, stage grilled, reinforced 3)"),
        "bullet carries kind, stage, reinforced: {md}"
    );
    assert!(
        md.contains("- Gamma (idea, reinforced 1) -> superseded by b"),
        "superseded bullet names its forwarding address: {md}"
    );
    assert!(md.contains("## Edges"), "edges section: {md}");
    assert!(md.contains("- a -raised x3-> b"), "weighted edge line: {md}");
    assert!(md.contains("- b -answered-by-> c"), "plain edge line: {md}");
}

#[test]
fn export_md_skips_empty_sections() {
    let data = GraphData {
        meta: meta("g"),
        nodes: vec![node("a", "Alpha", NodeStatus::Active)],
        edges: vec![],
    };
    let md = export_md(&data, None, 0).unwrap();
    assert!(md.contains("## Active"));
    assert!(!md.contains("## Parked"), "empty section absent: {md}");
    assert!(!md.contains("## Superseded"), "empty section absent: {md}");
    assert!(!md.contains("## Edges"), "edgeless graph has no edges section: {md}");
}

// ---------------------------------------------------------------------
// Interactive HTML export: one document, one radius authority.
// ---------------------------------------------------------------------

use neuron::export_html;
use neuron::render::{adjacency, radius_from_adjacency, within_radius};
use serde_json::json;

/// a -> b -> c -> a cycle with a c -> d -> e tail hanging off it.
fn cycle_fixture() -> GraphData {
    GraphData {
        meta: meta("cyc"),
        nodes: vec![
            node("a", "A", NodeStatus::Active),
            node("b", "B", NodeStatus::Active),
            node("c", "C", NodeStatus::Active),
            node("d", "D", NodeStatus::Active),
            node("e", "E", NodeStatus::Active),
        ],
        edges: vec![
            edge("a", "b", "next", 1),
            edge("b", "c", "next", 1),
            edge("c", "a", "closes", 1),
            edge("c", "d", "tail", 1),
            edge("d", "e", "tail", 1),
        ],
    }
}

/// Parallel edges both ways, a self-loop, a disconnected pair, and an
/// isolated node — the adversarial shapes for radius membership.
fn tangle_fixture() -> GraphData {
    GraphData {
        meta: meta("tan"),
        nodes: vec![
            node("a", "A", NodeStatus::Active),
            node("b", "B", NodeStatus::Active),
            node("c", "C", NodeStatus::Active),
            node("x", "X", NodeStatus::Active),
            node("y", "Y", NodeStatus::Active),
            node("z", "Z", NodeStatus::Active),
        ],
        edges: vec![
            edge("a", "b", "raised", 1),
            edge("a", "b", "again", 2),
            edge("b", "a", "returns", 1),
            edge("b", "b", "loops", 1),
            edge("b", "c", "onward", 1),
            edge("x", "y", "apart", 1),
        ],
    }
}

/// The equivalence proof behind the page's client-side scoping: the
/// embedded adjacency + transcribed BFS must compute exactly the
/// membership within_radius (the one authority) computes, for every
/// focus and every slider depth, over cycles, parallel edges,
/// self-loops, and disconnected parts.
#[test]
fn page_radius_algorithm_matches_within_radius_everywhere() {
    for data in [fixture(), cycle_fixture(), tangle_fixture()] {
        let adj = adjacency(&data);
        for focus in data.nodes.iter().map(|n| n.id.as_str()) {
            for depth in 0..=6 {
                let engine = within_radius(&data, focus, depth).unwrap();
                let page = radius_from_adjacency(&adj, focus, depth);
                assert_eq!(
                    engine, page,
                    "graph {:?} focus {focus:?} depth {depth} diverged",
                    data.meta.id
                );
            }
        }
    }
}

#[test]
fn adjacency_covers_every_node_exactly() {
    let data = tangle_fixture();
    let adj = adjacency(&data);
    let ids: Vec<&str> = data.nodes.iter().map(|n| n.id.as_str()).collect();
    let keys: Vec<&str> = adj.keys().map(String::as_str).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "every node keyed, none invented");
    assert_eq!(adj["z"], Vec::<String>::new(), "isolated node has no neighbors");
    assert_eq!(
        adj["a"],
        vec!["b".to_string(), "b".into(), "b".into()],
        "parallel and reverse edges keep their duplicate entries in scan order"
    );
}

fn embedded_payload(html: &str) -> serde_json::Value {
    let marker = "<script id=\"neuron-payload\" type=\"application/json\">";
    let start = html.find(marker).expect("payload script present") + marker.len();
    let end = html[start..].find("</script>").expect("payload closes") + start;
    serde_json::from_str(&html[start..end]).expect("payload is valid JSON")
}

#[test]
fn export_html_is_one_self_contained_document() {
    let html = export_html(&fixture(), None, 0).unwrap();
    assert!(html.starts_with("<!doctype html>"), "document header: {}", &html[..40]);
    assert!(html.contains("<title>graph g</title>"), "titled after the graph");
    assert!(html.contains("forceSimulation"), "vendored d3-force inlined");
    assert!(html.contains("d3-force@3.0.0"), "vendor provenance named");
    assert!(html.contains("@media (prefers-color-scheme: dark)"), "dark theme designed");
    assert!(html.contains("@media (prefers-color-scheme: light)"), "light theme designed");
    assert!(!html.contains("@@NEURON:"), "no unfilled template markers");
    assert!(
        html.contains("render::radius_from_adjacency"),
        "page JS names the algorithm it transcribes"
    );
}

#[test]
fn export_html_embeds_canonical_data_and_initial_scope() {
    let html = export_html(&fixture(), Some("b"), 1).unwrap();
    let payload = embedded_payload(&html);
    assert_eq!(payload["data"]["meta"]["id"], json!("g"));
    assert_eq!(
        payload["data"]["nodes"].as_array().unwrap().len(),
        5,
        "the whole graph ships even when focused: rescoping is client-side"
    );
    assert_eq!(payload["initial"]["focus"], json!("b"));
    assert_eq!(payload["initial"]["depth"], json!(1));
    assert_eq!(payload["adjacency"]["a"], json!(["b"]));
    assert_eq!(payload["adjacency"]["e"], json!([]), "disconnected node embedded too");
}

#[test]
fn export_html_refuses_unknown_focus() {
    let err = export_html(&fixture(), Some("ghost"), 2).unwrap_err();
    assert!(err.to_string().contains("ghost"), "refusal names the id: {err}");
}

#[test]
fn export_html_keeps_status_and_weight_parity_with_mermaid() {
    let html = export_html(&fixture(), None, 0).unwrap();
    assert!(html.contains(".node.superseded circle.body"), "superseded class styled");
    assert!(html.contains(".node.parked circle.body"), "parked class styled");
    assert!(html.contains("stroke-dasharray: 5 5"), "superseded dashes match mermaid");
    assert!(html.contains("stroke-dasharray: 2 3"), "parked dashes match mermaid");
    assert!(html.contains("' x' + weight"), "edge labels carry weight x-counts");
}

#[test]
fn hostile_titles_cannot_break_out_of_the_page() {
    let mut data = fixture();
    data.meta.title = "sneaky </script><script>alert(1)".into();
    data.nodes[0].title = "also </script> here".into();
    let html = export_html(&data, None, 0).unwrap();
    assert!(
        !html.contains("</script><script>alert"),
        "no raw script breakout anywhere in the page"
    );
    assert!(
        html.contains("<title>sneaky &lt;/script&gt;&lt;script&gt;alert(1)</title>"),
        "head title is entity-escaped"
    );
    let payload = embedded_payload(&html);
    assert_eq!(
        payload["data"]["meta"]["title"],
        json!("sneaky </script><script>alert(1)"),
        "the title round-trips intact through the escaped payload"
    );
    assert_eq!(payload["data"]["nodes"][0]["title"], json!("also </script> here"));
}
