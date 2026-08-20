use neuron::{EngramStore, GraphMeta, GraphStatus, NeuronGraph, NewNode, NodeStatus};

fn meta(id: &str) -> GraphMeta {
    GraphMeta {
        id: id.into(),
        title: format!("graph {id}"),
        status: GraphStatus::Active,
        project: Some("neurons".into()),
        created: 100,
        updated: 100,
    }
}

fn draft(id: &str, title: &str, content: &str) -> NewNode {
    NewNode {
        id: id.into(),
        kind: "idea".into(),
        title: title.into(),
        content: content.into(),
        stage: None,
        skills: vec!["grilling".into()],
    }
}

fn open_temp() -> (tempfile::TempDir, EngramStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EngramStore::open(&dir.path().join("neurons.db")).expect("open");
    (dir, store)
}

fn worked_graph() -> NeuronGraph {
    let mut g = NeuronGraph::new(meta("g1"));
    g.add_node(draft("a", "Token ceremony", "private repo forces tokens"), 110)
        .unwrap();
    g.add_node(draft("b", "Public repo", "dissolves the ceremony"), 120)
        .unwrap();
    g.link("b", "a", "contradicts", 130).unwrap();
    g.supersede("a", "b", 140).unwrap();
    g
}

#[test]
fn consolidate_then_recall_roundtrips() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    let recalled = store.recall("g1").unwrap();
    assert_eq!(recalled, g.to_data());
    let rebuilt = NeuronGraph::from_data(recalled).unwrap();
    assert_eq!(rebuilt.dirty(), 0);
}

#[test]
fn incremental_consolidation_touches_only_trace_rows() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    g.reinforce("b", 200).unwrap();
    let trace = g.take_trace();
    assert_eq!(trace.nodes.len(), 1, "only b in the trace");
    store.consolidate("g1", &trace).unwrap();

    let recalled = store.recall("g1").unwrap();
    let a = recalled.nodes.iter().find(|n| n.id == "a").unwrap();
    let b = recalled.nodes.iter().find(|n| n.id == "b").unwrap();
    assert_eq!(a.updated, 140, "untouched row kept its stamp");
    assert_eq!((b.reinforced, b.updated), (2, 200));
    assert_eq!(recalled.meta.updated, 200, "meta rode along");
}

#[test]
fn empty_trace_is_a_no_op() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();
    assert_eq!(store.recall("g1").unwrap(), g.to_data());
}

#[test]
fn search_spans_graphs_and_stays_in_sync() {
    let (_dir, mut store) = open_temp();
    let mut g1 = worked_graph();
    store.create(g1.meta()).unwrap();
    store.consolidate("g1", &g1.take_trace()).unwrap();

    let mut g2 = NeuronGraph::new(meta("g2"));
    g2.add_node(draft("x", "Marketplace clone", "ssh credential ceremony"), 300)
        .unwrap();
    store.create(g2.meta()).unwrap();
    store.consolidate("g2", &g2.take_trace()).unwrap();

    let hits = store.search("ceremony", 10).unwrap();
    let graphs: Vec<&str> = hits.iter().map(|h| h.graph_id.as_str()).collect();
    assert!(graphs.contains(&"g1") && graphs.contains(&"g2"));

    g1.reinforce("a", 400).unwrap();
    store.consolidate("g1", &g1.take_trace()).unwrap();
    let again = store.search("ceremony", 10).unwrap();
    assert_eq!(
        again.iter().filter(|h| h.node_id == "a").count(),
        1,
        "upsert did not duplicate the FTS entry"
    );
    assert!(store.search("nonexistentterm", 10).unwrap().is_empty());
}

#[test]
fn list_filters_and_orders() {
    let (_dir, mut store) = open_temp();
    for (id, updated) in [("g1", 100), ("g2", 300)] {
        let mut m = meta(id);
        m.updated = updated;
        store.create(&m).unwrap();
    }
    let all = store.list(None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "g2", "ordered by updated desc");
    assert_eq!(store.list(Some(GraphStatus::Settled), None).unwrap().len(), 0);
    assert_eq!(store.list(None, Some("neurons")).unwrap().len(), 2);
    assert_eq!(store.list(None, Some("other")).unwrap().len(), 0);
}

#[test]
fn import_refuses_existing_and_roundtrips_fresh() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    let err = store.import(&g.to_data()).unwrap_err();
    assert!(err.to_string().contains("never replaces"));

    let mut moved = g.to_data();
    moved.meta.id = "g1-copy".into();
    store.import(&moved).unwrap();
    let recalled = store.recall("g1-copy").unwrap();
    assert_eq!(recalled.nodes, moved.nodes);
    assert_eq!(recalled.edges, moved.edges);
    assert!(store.exists("g1-copy").unwrap());
}

#[test]
fn recall_missing_graph_fails() {
    let (_dir, mut store) = open_temp();
    assert!(store.recall("ghost").is_err());
    assert!(!store.exists("ghost").unwrap());
}

#[test]
fn newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("neurons.db");
    drop(EngramStore::open(&path).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
             VALUES (99, 'from_the_future', '2099-01-01T00:00:00Z', '0')",
            [],
        )
        .unwrap();
    }
    let err = EngramStore::open(&path).unwrap_err();
    let msg = format!("{err:#}").to_lowercase();
    assert!(msg.contains("missing") || msg.contains("newer"), "got: {msg}");
}

#[test]
fn statuses_roundtrip_through_storage() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    g.settle(500);
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    let recalled = store.recall("g1").unwrap();
    assert_eq!(recalled.meta.status, GraphStatus::Settled);
    let a = recalled.nodes.iter().find(|n| n.id == "a").unwrap();
    assert_eq!(a.status, NodeStatus::Superseded);
    assert_eq!(a.superseded_by.as_deref(), Some("b"));
}

#[test]
fn edge_reconsolidation_replaces_weight_row_for_row() {
    let (_dir, mut store) = open_temp();
    let mut g = worked_graph();
    store.create(g.meta()).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    g.link("b", "a", "contradicts", 500).unwrap();
    g.link("b", "a", "contradicts", 510).unwrap();
    store.consolidate("g1", &g.take_trace()).unwrap();

    let recalled = store.recall("g1").unwrap();
    let e = recalled.edges.iter().find(|e| e.label == "contradicts").unwrap();
    assert_eq!(e.weight, 3, "storage mirrors memory; SQL never adds");
}

#[test]
fn multi_edge_roundtrip_is_order_canonical() {
    let (_dir, mut store) = open_temp();
    let mut g = NeuronGraph::new(meta("gz"));
    g.add_node(draft("z", "Zed", "last"), 100).unwrap();
    g.add_node(draft("a", "Ay", "first"), 110).unwrap();
    g.link("z", "a", "zeta", 120).unwrap();
    g.link("a", "z", "alpha", 130).unwrap();
    store.create(g.meta()).unwrap();
    store.consolidate("gz", &g.take_trace()).unwrap();
    assert_eq!(store.recall("gz").unwrap(), g.to_data());
}

#[test]
fn parked_status_roundtrips_via_import() {
    let (_dir, mut store) = open_temp();
    let mut data = worked_graph().to_data();
    data.meta.id = "gp".into();
    data.nodes[0].status = NodeStatus::Parked;
    store.import(&data).unwrap();
    let recalled = store.recall("gp").unwrap();
    assert_eq!(recalled.nodes[0].status, NodeStatus::Parked);
}
