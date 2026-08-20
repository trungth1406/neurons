use neuron::{
    ConsolidationPolicy, Cortex, EngramStore, GraphMeta, GraphStatus, NewNode, Op,
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

fn add(id: &str, title: &str) -> Op {
    Op::AddNode(NewNode {
        id: id.into(),
        kind: "idea".into(),
        title: title.into(),
        content: format!("content of {title}"),
        stage: None,
        skills: vec![],
    })
}

fn setup(policy: ConsolidationPolicy) -> (tempfile::TempDir, Cortex) {
    let dir = tempfile::tempdir().unwrap();
    let cortex = Cortex::open(&dir.path().join("neurons.db"), policy).unwrap();
    (dir, cortex)
}

/// Observe consolidated state through an independent reader connection —
/// WAL allows concurrent readers while the cortex holds its lock.
fn consolidated_node_count(dir: &tempfile::TempDir, graph: &str) -> usize {
    let mut reader = EngramStore::open(&dir.path().join("neurons.db")).unwrap();
    reader.recall(graph).map(|d| d.nodes.len()).unwrap_or(0)
}

#[test]
fn second_cortex_is_refused_while_first_lives() {
    let (dir, cortex) = setup(ConsolidationPolicy::default());
    let err = Cortex::open(&dir.path().join("neurons.db"), ConsolidationPolicy::default())
        .map(|_| ())
        .unwrap_err();
    assert!(err.to_string().contains("another cortex"));
    drop(cortex);
    Cortex::open(&dir.path().join("neurons.db"), ConsolidationPolicy::default())
        .expect("lock released with the first cortex");
}

#[test]
fn below_threshold_stays_in_working_memory() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 110).unwrap();
    cortex.apply("g", add("b", "Beta"), 120).unwrap();
    assert_eq!(
        consolidated_node_count(&dir, "g"),
        0,
        "2 mutations < threshold 10: nothing consolidated yet"
    );
}

#[test]
fn threshold_crossing_consolidates() {
    let policy = ConsolidationPolicy { dirty_threshold: 3, ..Default::default() };
    let (dir, mut cortex) = setup(policy);
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 110).unwrap();
    cortex.apply("g", add("b", "Beta"), 120).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 0);
    cortex.apply("g", add("c", "Gamma"), 130).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 3, "third op hit the threshold");
}

#[test]
fn lifecycle_consolidates_immediately() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 110).unwrap();
    cortex.apply("g", add("b", "Beta"), 120).unwrap();
    cortex
        .apply("g", Op::Supersede { old: "a".into(), by: "b".into() }, 130)
        .unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 2, "supersede is hardwired");
}

#[test]
fn focus_switch_consolidates_the_graph_left_behind() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g1")).unwrap();
    cortex.create_graph(&meta("g2")).unwrap();
    cortex.apply("g1", add("a", "Alpha"), 110).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g1"), 0);
    cortex.apply("g2", add("x", "Xi"), 120).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g1"), 1, "leaving g1 flushed it");
}

#[test]
fn quiet_period_tick_consolidates() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 1_000).unwrap();
    cortex.tick(1_030).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 0, "only 30s idle");
    cortex.tick(1_061).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 1, "61s idle >= quiet 60");
}

#[test]
fn shutdown_consolidates_everything_dirty() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::exit_only());
    cortex.create_graph(&meta("g1")).unwrap();
    cortex.create_graph(&meta("g2")).unwrap();
    cortex.apply("g1", add("a", "Alpha"), 110).unwrap();
    cortex.apply("g2", add("x", "Xi"), 120).unwrap();
    cortex.consolidate_all(130).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g1"), 1);
    assert_eq!(consolidated_node_count(&dir, "g2"), 1);
}

#[test]
fn memory_pressure_releases_the_coldest() {
    let policy = ConsolidationPolicy { max_loaded: 2, ..Default::default() };
    let (dir, mut cortex) = setup(policy);
    for (id, at) in [("g1", 110), ("g2", 200), ("g3", 300)] {
        cortex.create_graph(&meta(id)).unwrap();
        cortex.apply(id, add("n", "Node"), at).unwrap();
    }
    assert_eq!(cortex.loaded(), 2, "pressure released down to max_loaded");
    assert_eq!(
        consolidated_node_count(&dir, "g1"),
        1,
        "the released graph reached the engram store"
    );
    let summary = cortex.summary("g1", 5, 400).unwrap();
    assert_eq!(summary.counts.active, 1, "released graph recalls cleanly");
}

#[test]
fn creation_alone_cannot_grow_the_cache_unboundedly() {
    let policy = ConsolidationPolicy { max_loaded: 2, ..Default::default() };
    let (_dir, mut cortex) = setup(policy);
    for id in ["g1", "g2", "g3", "g4", "g5"] {
        cortex.create_graph(&meta(id)).unwrap();
    }
    assert!(cortex.loaded() <= 2, "creates are subject to pressure too");
}

#[test]
fn ghost_graph_neither_steals_focus_nor_consolidates_the_previous() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 110).unwrap();
    assert!(cortex.apply("ghost", add("x", "Xi"), 120).is_err());
    assert_eq!(
        consolidated_node_count(&dir, "g"),
        0,
        "failed apply must not fire FocusSwitch for the previous graph"
    );
    assert!(cortex.consolidate(Some("ghost"), 130).is_err());
}

#[test]
fn hot_thought_survives_cold_saturation() {
    let policy = ConsolidationPolicy { dirty_threshold: 2, ..Default::default() };
    let (_dir, mut cortex) = setup(policy);
    cortex.create_graph(&meta("cold")).unwrap();
    for i in 0..6 {
        cortex
            .apply("cold", add(&format!("c{i}"), &format!("saturation topic {i}")), 100 + i)
            .unwrap();
    }
    cortex.create_graph(&meta("hot")).unwrap();
    cortex
        .apply("hot", add("fresh", "saturation topic fresh"), 200)
        .unwrap();
    let hits = cortex.search("saturation", 6).unwrap();
    assert!(
        hits.iter().any(|h| h.graph_id == "hot" && h.node_id == "fresh"),
        "fresh hot thought must not be starved out by cold results"
    );
    assert_eq!(hits.len(), 6, "limit still respected");
}

#[test]
fn on_demand_consolidates_now() {
    let (dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex.apply("g", add("a", "Alpha"), 110).unwrap();
    cortex.consolidate(Some("g"), 120).unwrap();
    assert_eq!(consolidated_node_count(&dir, "g"), 1);
}

#[test]
fn search_sees_hot_unconsolidated_thought() {
    let (_dir, mut cortex) = setup(ConsolidationPolicy::default());
    cortex.create_graph(&meta("g")).unwrap();
    cortex
        .apply("g", add("a", "Ephemeral brainstorm about tokens"), 110)
        .unwrap();
    let hits = cortex.search("ephemeral", 10).unwrap();
    assert_eq!(hits.len(), 1, "hot focused graph is searchable before consolidation");
    assert_eq!(hits[0].node_id, "a");
}

#[test]
fn ops_roundtrip_as_wire_format() {
    let op = Op::Supersede { old: "n5".into(), by: "n13".into() };
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(json, r#"{"op":"supersede","old":"n5","by":"n13"}"#);
    assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
}
