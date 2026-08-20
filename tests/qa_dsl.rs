#![allow(dead_code)]
// QA DSL: the phase-1 vocabulary table as helper functions. Each helper is a
// domain phrase; the body is the public API call it stands for. Scenario
// files include this module via #[path]. Blind suite: built from
// docs/DESIGN.md, docs/adr/* and the public surface only.

use neuron::{
    ConsolidationPolicy, Cortex, EngramStore, GraphData, GraphMeta, GraphStatus, NeuronGraph,
    NewNode, Node, Op,
};
use std::path::PathBuf;

// --- thoughts and minds -----------------------------------------------------

pub fn graph_meta(id: &str) -> GraphMeta {
    GraphMeta {
        id: id.into(),
        title: format!("thinking about {id}"),
        status: GraphStatus::Active,
        project: None,
        created: 1,
        updated: 1,
    }
}

pub fn fresh_mind(id: &str) -> NeuronGraph {
    NeuronGraph::new(graph_meta(id))
}

pub fn idea(id: &str) -> NewNode {
    NewNode {
        id: id.into(),
        kind: "idea".into(),
        title: format!("thought {id}"),
        content: format!("body of {id}"),
        stage: None,
        skills: vec![],
    }
}

/// An idea carrying a distinctive searchable word in title and content.
pub fn idea_about(id: &str, word: &str) -> NewNode {
    NewNode {
        id: id.into(),
        kind: "idea".into(),
        title: format!("thought {id} about {word}"),
        content: format!("body of {id} mentioning {word}"),
        stage: None,
        skills: vec![],
    }
}

pub fn capture(mind: &mut NeuronGraph, id: &str, now: i64) {
    mind.add_node(idea(id), now).expect("capture");
}

pub fn connect(mind: &mut NeuronGraph, from: &str, label: &str, to: &str, now: i64) {
    mind.link(from, to, label, now).expect("connect");
}

/// The discussion re-confirms a link k more times.
pub fn reconfirm(mind: &mut NeuronGraph, from: &str, label: &str, to: &str, k: usize, now: i64) {
    for _ in 0..k {
        mind.link(from, to, label, now).expect("reconfirm");
    }
}

pub fn reinforce_belief(mind: &mut NeuronGraph, id: &str, k: usize, now: i64) {
    for _ in 0..k {
        mind.reinforce(id, now).expect("reinforce");
    }
}

pub fn correct(mind: &mut NeuronGraph, old: &str, by: &str, now: i64) {
    mind.supersede(old, by, now).expect("correct");
}

pub fn the_thought<'a>(data: &'a GraphData, id: &str) -> &'a Node {
    data.nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap_or_else(|| panic!("no thought {id}"))
}

pub fn the_thought_in<'a>(mind: &'a NeuronGraph, id: &str) -> &'a Node {
    mind.nodes()
        .iter()
        .find(|n| n.id == id)
        .unwrap_or_else(|| panic!("no thought {id}"))
}

pub fn add_op(id: &str) -> Op {
    Op::AddNode(idea(id))
}

pub fn add_about_op(id: &str, word: &str) -> Op {
    Op::AddNode(idea_about(id, word))
}

// --- topology catalog (phase-1 section 2) -----------------------------------

/// T1: premise -> step -> ... -> conclusion.
pub fn a_chain_of_reasoning(ids: &[&str]) -> NeuronGraph {
    let mut mind = fresh_mind("chain");
    let mut now = 10;
    for id in ids {
        capture(&mut mind, id, now);
        now += 1;
    }
    for pair in ids.windows(2) {
        connect(&mut mind, pair[0], "therefore", pair[1], now);
        now += 1;
    }
    mind
}

/// T2: several hypotheses all explaining one observation (fan-in).
pub fn competing_explanations(observation: &str, hypotheses: &[&str]) -> NeuronGraph {
    let mut mind = fresh_mind("competing");
    let mut now = 10;
    capture(&mut mind, observation, now);
    for h in hypotheses {
        now += 1;
        capture(&mut mind, h, now);
        connect(&mut mind, h, "explains", observation, now);
    }
    mind
}

/// T3: one claim with many consequences (fan-out).
pub fn one_idea_many_implications(claim: &str, implications: &[&str]) -> NeuronGraph {
    let mut mind = fresh_mind("implications");
    let mut now = 10;
    capture(&mut mind, claim, now);
    for i in implications {
        now += 1;
        capture(&mut mind, i, now);
        connect(&mut mind, claim, "implies", i, now);
    }
    mind
}

/// T4: thoughts arguing in a circle.
pub fn circular_reasoning(ids: &[&str]) -> NeuronGraph {
    let mut mind = fresh_mind("circle");
    let mut now = 10;
    for id in ids {
        capture(&mut mind, id, now);
        now += 1;
    }
    for pair in ids.windows(2) {
        connect(&mut mind, pair[0], "justifies", pair[1], now);
        now += 1;
    }
    connect(&mut mind, ids[ids.len() - 1], "justifies", ids[0], now);
    mind
}

/// T11: a -> b -> d and a -> c -> d, two equal routes to one conclusion.
pub fn two_routes_to_the_same_conclusion() -> NeuronGraph {
    let mut mind = fresh_mind("diamond");
    for (i, id) in ["a", "b", "c", "d"].iter().enumerate() {
        capture(&mut mind, id, 10 + i as i64);
    }
    connect(&mut mind, "a", "route", "b", 20);
    connect(&mut mind, "b", "route", "d", 21);
    connect(&mut mind, "a", "route", "c", 22);
    connect(&mut mind, "c", "route", "d", 23);
    mind
}

/// T7: a chain plus an unrelated pair, no route between them.
pub fn disconnected_musings() -> NeuronGraph {
    let mut mind = fresh_mind("musings");
    for (i, id) in ["p1", "p2", "p3", "m1", "m2"].iter().enumerate() {
        capture(&mut mind, id, 10 + i as i64);
    }
    connect(&mut mind, "p1", "therefore", "p2", 20);
    connect(&mut mind, "p2", "therefore", "p3", 21);
    connect(&mut mind, "m1", "reminds-of", "m2", 22);
    mind
}

/// T8: belief v1 superseded by v2 superseded by v3, plus supporting evidence.
pub fn a_lineage_of_corrections() -> NeuronGraph {
    let mut mind = fresh_mind("lineage");
    for (i, id) in ["v1", "v2", "v3", "evidence"].iter().enumerate() {
        capture(&mut mind, id, 10 + i as i64);
    }
    connect(&mut mind, "v1", "explains", "evidence", 20);
    correct(&mut mind, "v1", "v2", 21);
    correct(&mut mind, "v2", "v3", 22);
    mind
}

/// T13: a deterministic practice-size cluster (~30 thoughts) with mixed kinds,
/// statuses, stages, skills, reinforcement, hammered and parallel links, and
/// hostile-character seasoning.
pub fn a_practice_size_cluster() -> NeuronGraph {
    let mut mind = fresh_mind("cluster");
    let kinds = ["claim", "evidence", "question"];
    for i in 0..28 {
        let id = format!("n{i:02}");
        mind.add_node(
            NewNode {
                id: id.clone(),
                kind: kinds[i % 3].into(),
                title: format!("thought {id}"),
                content: format!("body of {id}"),
                stage: if i % 5 == 0 { Some("hypothesis".into()) } else { None },
                skills: if i % 7 == 0 {
                    vec!["reasoning".into(), "reasoning".into(), String::new()]
                } else {
                    vec![]
                },
            },
            100 + i as i64,
        )
        .expect("cluster node");
    }
    mind.add_node(
        NewNode {
            id: "id\u{e9}e-\u{1F4A1}".into(),
            kind: "id\u{e9}e".into(),
            title: "caf\u{e9} \u{6982}\u{5FF5} newline\ntab\tquote\"".into(),
            content: "'; DROP TABLE nodes;-- and a NUL \u{0} byte".into(),
            stage: None,
            skills: vec![],
        },
        200,
    )
    .expect("seasoned node");
    mind.add_node(idea("rowid"), 201).expect("keyword id node");
    for i in 0..10 {
        connect(
            &mut mind,
            &format!("n{i:02}"),
            "therefore",
            &format!("n{:02}", i + 1),
            210 + i,
        );
    }
    for i in 0..28 {
        let to = (i * 7 + 3) % 28;
        if to != i {
            connect(&mut mind, &format!("n{i:02}"), "relates", &format!("n{to:02}"), 240 + i as i64);
        }
    }
    reconfirm(&mut mind, "n00", "therefore", "n01", 3, 270);
    connect(&mut mind, "n00", "contradicts", "n01", 271);
    connect(&mut mind, "rowid", "relates", "id\u{e9}e-\u{1F4A1}", 272);
    reinforce_belief(&mut mind, "n03", 4, 280);
    reinforce_belief(&mut mind, "n07", 2, 281);
    correct(&mut mind, "n20", "n21", 290);
    mind.set_stage("n05", "confirmed", 291).expect("stage");
    mind
}

// --- long-term memory --------------------------------------------------------

/// A private home for one memory: tempdir plus the engram database inside it.
/// The cortex lock lives beside the database, so every scenario gets its own.
pub struct MemoryHome {
    pub dir: tempfile::TempDir,
    pub db: PathBuf,
}

pub fn a_quiet_place() -> MemoryHome {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("engram.db");
    MemoryHome { dir, db }
}

pub fn long_term_memory(home: &MemoryHome) -> EngramStore {
    EngramStore::open(&home.db).expect("open engram store")
}

/// Consolidate a mind's whole life so far: found the memory, drain the trace.
pub fn consolidate_whole(store: &mut EngramStore, mind: &mut NeuronGraph) {
    let id = mind.meta().id.clone();
    store.create(mind.meta()).expect("create graph row");
    let trace = mind.take_trace();
    store.consolidate(&id, &trace).expect("consolidate");
}

pub fn recall_engram(store: &mut EngramStore, id: &str) -> GraphData {
    store.recall(id).expect("recall")
}

// --- the cortex ---------------------------------------------------------------

/// A policy whose configurable triggers never fire (thresholds out of reach),
/// leaving only the hardwired reflexes.
pub fn lazy_policy() -> ConsolidationPolicy {
    ConsolidationPolicy {
        dirty_threshold: 1_000_000,
        quiet_secs: 1_000_000,
        max_loaded: 8,
    }
}

pub fn awake_cortex(home: &MemoryHome, policy: ConsolidationPolicy) -> Cortex {
    Cortex::open(&home.db, policy).expect("open cortex")
}

/// What a cold reader (second connection, consolidated state only) can find.
pub fn cold_hits(home: &MemoryHome, word: &str) -> usize {
    EngramStore::open(&home.db)
        .expect("cold reader")
        .search(word, 50)
        .expect("cold search")
        .len()
}

pub fn cold_recall(home: &MemoryHome, id: &str) -> GraphData {
    EngramStore::open(&home.db)
        .expect("cold reader")
        .recall(id)
        .expect("cold recall")
}
