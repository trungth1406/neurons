use neuron::{GraphMeta, GraphStatus, NeuronGraph, NewNode, NodeStatus};

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

fn draft(id: &str, title: &str) -> NewNode {
    NewNode {
        id: id.into(),
        kind: "idea".into(),
        title: title.into(),
        content: format!("content of {title}"),
        stage: None,
        skills: vec!["grilling".into()],
    }
}

fn seeded() -> NeuronGraph {
    let mut g = NeuronGraph::new(meta("g"));
    g.add_node(draft("a", "Alpha"), 110).unwrap();
    g.add_node(draft("b", "Beta"), 120).unwrap();
    g.add_node(draft("c", "Gamma"), 130).unwrap();
    g.link("a", "b", "raised", 140).unwrap();
    g.link("b", "c", "answered-by", 150).unwrap();
    g
}

#[test]
fn add_and_link_shape_the_graph() {
    let g = seeded();
    assert_eq!(g.nodes().len(), 3);
    assert_eq!(g.edges().len(), 2);
    let a = &g.nodes()[0];
    assert_eq!((a.id.as_str(), a.reinforced, a.status), ("a", 1, NodeStatus::Active));
    assert_eq!(g.meta().updated, 150);
}

#[test]
fn duplicate_node_and_missing_link_fail() {
    let mut g = seeded();
    assert!(g.add_node(draft("a", "again"), 160).is_err());
    let err = g.link("a", "ghost", "haunts", 160).unwrap_err();
    assert!(err.to_string().contains("ghost"));
}

#[test]
fn repeated_link_reinforces_weight_not_count() {
    let mut g = seeded();
    g.link("a", "b", "raised", 160).unwrap();
    g.link("a", "b", "contradicts", 170).unwrap();
    assert_eq!(g.edges().len(), 3);
    let raised = g.edges().iter().find(|e| e.label == "raised").unwrap();
    assert_eq!(raised.weight, 2);
}

#[test]
fn supersede_marks_and_survives() {
    let mut g = seeded();
    g.supersede("a", "b", 200).unwrap();
    let a = g.nodes().iter().find(|n| n.id == "a").unwrap();
    assert_eq!(a.status, NodeStatus::Superseded);
    assert_eq!(a.superseded_by.as_deref(), Some("b"));
    assert_eq!(g.nodes().len(), 3, "supersede never deletes");
    assert!(g.supersede("a", "a", 210).is_err(), "self-supersede refused");
    assert!(g.supersede("b", "ghost", 210).is_err());
}

#[test]
fn trace_dedups_by_identity_and_carries_current_rows() {
    let mut g = seeded();
    g.take_trace();
    assert_eq!(g.dirty(), 0);

    g.reinforce("a", 200).unwrap();
    g.reinforce("a", 210).unwrap();
    g.link("a", "b", "raised", 220).unwrap();
    g.supersede("c", "b", 230).unwrap();
    assert_eq!(g.dirty(), 4);

    let trace = g.take_trace();
    assert_eq!(trace.nodes.len(), 2, "a touched twice = one entry; plus c");
    let a = trace.nodes.iter().find(|n| n.id == "a").unwrap();
    assert_eq!(a.reinforced, 3, "row is current state, not op history");
    assert_eq!(trace.edges.len(), 1);
    assert_eq!(trace.edges[0].weight, 2);
    assert!(trace.meta.is_some(), "meta.updated changed");
    assert!(trace.deleted_nodes.is_empty() && trace.deleted_edges.is_empty());

    assert_eq!(g.dirty(), 0);
    assert!(g.take_trace().is_empty(), "drained journal stays empty");
}

#[test]
fn neighborhood_respects_depth_and_direction() {
    let g = seeded();
    let n1 = g.neighborhood("a", 1).unwrap();
    assert_eq!(n1.out.len(), 1);
    assert_eq!(n1.out[0].1.id, "b");
    assert!(n1.inc.is_empty());

    let n2 = g.neighborhood("a", 2).unwrap();
    assert_eq!(n2.out.len(), 2, "depth 2 reaches b and c");

    let nb = g.neighborhood("b", 1).unwrap();
    assert_eq!(nb.out.len(), 1);
    assert_eq!(nb.inc.len(), 1);
    assert_eq!(nb.inc[0].1.id, "a");
}

#[test]
fn parallel_edges_are_not_double_collected() {
    let mut g = seeded();
    g.link("a", "b", "contradicts", 160).unwrap();
    let n = g.neighborhood("a", 1).unwrap();
    assert_eq!(n.out.len(), 2, "two labels a->b = exactly two entries");
}

#[test]
fn path_follows_direction() {
    let g = seeded();
    assert_eq!(
        g.path("a", "c").unwrap(),
        Some(vec!["a".into(), "b".into(), "c".into()])
    );
    assert_eq!(g.path("c", "a").unwrap(), None, "edges are directed");
    assert!(g.path("a", "ghost").is_err());
}

#[test]
fn summary_counts_and_ranks() {
    let mut g = seeded();
    g.reinforce("b", 200).unwrap();
    g.reinforce("b", 210).unwrap();
    g.supersede("c", "b", 220).unwrap();

    let s = g.summary(2);
    assert_eq!((s.counts.active, s.counts.superseded, s.counts.parked), (2, 1, 0));
    assert_eq!(s.frontier.len(), 2);
    assert_eq!(s.frontier[0].id, "b", "frontier = latest updated active");
    assert_eq!(s.top[0].id, "b", "top = most reinforced");
    assert_eq!(s.top[0].reinforced, 3);
}

#[test]
fn settle_reopen_flip_status_and_journal_meta() {
    let mut g = seeded();
    g.take_trace();
    g.settle(300);
    assert_eq!(g.meta().status, GraphStatus::Settled);
    let t = g.take_trace();
    assert!(t.meta.is_some() && t.nodes.is_empty() && t.edges.is_empty());
    g.reopen(310);
    assert_eq!(g.meta().status, GraphStatus::Active);
}

#[test]
fn data_roundtrip_rebuilds_topology() {
    let mut g = seeded();
    g.supersede("c", "b", 200).unwrap();
    let data = g.to_data();
    let rebuilt = NeuronGraph::from_data(data.clone()).unwrap();
    assert_eq!(rebuilt.to_data(), data);
    assert_eq!(rebuilt.dirty(), 0, "recall starts with a clean journal");
    assert_eq!(
        rebuilt.path("a", "c").unwrap(),
        Some(vec!["a".into(), "b".into(), "c".into()]),
        "topology works after rebuild"
    );
}

#[test]
fn from_data_rejects_corrupt_references() {
    let mut data = seeded().to_data();
    data.edges.push(neuron::Edge {
        from: "ghost".into(),
        to: "a".into(),
        label: "haunts".into(),
        weight: 1,
        created: 0,
    });
    assert!(NeuronGraph::from_data(data).is_err());
}

#[test]
fn set_stage_updates_and_journals() {
    let mut g = seeded();
    g.take_trace();
    g.set_stage("a", "grilled", 400).unwrap();
    let a = g.nodes().iter().find(|n| n.id == "a").unwrap();
    assert_eq!(a.stage.as_deref(), Some("grilled"));
    assert_eq!(a.updated, 400);
    let t = g.take_trace();
    assert_eq!(t.nodes.len(), 1);
    assert_eq!(t.nodes[0].stage.as_deref(), Some("grilled"));
    assert!(g.set_stage("ghost", "x", 410).is_err());
}

#[test]
fn touch_stamps_last_touch_and_meta() {
    let mut g = seeded();
    assert_eq!(g.touched(), 150, "last op stamped");
    g.reinforce("a", 500).unwrap();
    assert_eq!(g.touched(), 500);
    assert_eq!(g.meta().updated, 500);
}

#[test]
fn trace_rows_follow_insertion_order() {
    let mut g = seeded();
    g.reinforce("c", 200).unwrap();
    g.reinforce("a", 210).unwrap();
    g.reinforce("b", 220).unwrap();
    let t = g.take_trace();
    let order: Vec<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(order, ["a", "b", "c"], "deterministic: insertion order, not touch order");
}
