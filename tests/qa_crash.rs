// QA group E: the crash model and cross-process concurrency.
// QA-37 spawns this very test binary as a child (env-gated helper) and kills
// the mind mid-thought; QA-38 reads the library while the owner is thinking.

#[path = "qa_dsl.rs"]
mod qa_dsl;

use neuron::{ConsolidationPolicy, Cortex, EngramStore, NeuronGraph, NodeStatus, Op};
use qa_dsl::*;
use std::path::Path;
use std::process::{Command, Stdio};

// The child half of QA-37. Runs only when spawned with QA37_DB set; under a
// normal `cargo test` run it is an instant no-op pass. It consolidates a
// correction (hardwired lifecycle), then captures four more thoughts under an
// exit-only policy - thoughts that never reach long-term memory - and dies
// without ceremony: abort skips Drop, so no shutdown consolidation runs.
#[test]
fn crash_child_the_mind_dies_mid_thought() {
    let Ok(db) = std::env::var("QA37_DB") else {
        return;
    };
    let mut cortex = Cortex::open(Path::new(&db), ConsolidationPolicy::exit_only()).unwrap();
    cortex.create_graph(&graph_meta("crash")).unwrap();
    cortex.apply("crash", add_op("keep-a"), 10).unwrap();
    cortex.apply("crash", add_op("keep-b"), 11).unwrap();
    cortex
        .apply("crash", Op::Supersede { old: "keep-a".into(), by: "keep-b".into() }, 12)
        .unwrap();
    for (i, id) in ["lost-1", "lost-2", "lost-3", "lost-4"].iter().enumerate() {
        cortex.apply("crash", add_op(id), 13 + i as i64).unwrap();
    }
    std::process::abort();
}

// QA-37: an unclean death loses at most the loss window - and never the
// memory itself. Everything up to the last hardwired consolidation survives;
// the tail is the bargain; the database opens clean and the kernel released
// the dead owner's lock.
// Contract: DESIGN concurrency and crash model (crash loses at most the
// policy loss window); ADR-0002 (loss window: up to the next lifecycle);
// cortex.rs (kernel releases the lock on death - no stale locks).
#[test]
fn qa_37_an_unclean_death_loses_at_most_the_loss_window() {
    let home = a_quiet_place();
    let exe = std::env::current_exe().unwrap();
    let status = Command::new(exe)
        .arg("crash_child_the_mind_dies_mid_thought")
        .arg("--exact")
        .env("QA37_DB", &home.db)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success(), "the child died mid-thought");

    let mut store = EngramStore::open(&home.db).expect("the memory itself survived intact");
    let cold = store.recall("crash").unwrap();
    let mut ids: Vec<&str> = cold.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["keep-a", "keep-b"], "everything up to the correction survived");
    let corrected = the_thought(&cold, "keep-a");
    assert_eq!(corrected.status, NodeStatus::Superseded);
    assert_eq!(corrected.superseded_by.as_deref(), Some("keep-b"));
    assert!(
        !cold.nodes.iter().any(|n| n.id.starts_with("lost-")),
        "the tail after the last consolidation is the loss window - exactly it"
    );
    drop(store);

    assert!(
        Cortex::open(&home.db, ConsolidationPolicy::default()).is_ok(),
        "the kernel released the dead owner's lock; a new mind takes over at once"
    );
}

// QA-38: a reader can browse the library while the owner is thinking. Every
// answer is a consistent consolidated snapshot: no failure within the busy
// timeout, and never a torn trace - an edge whose endpoint thought is
// missing, or a snapshot the mind cannot rebuild.
// Contract: DESIGN concurrency and crash model (WAL: concurrent snapshot
// readers are safe); engram.rs (busy_timeout; consolidate is ONE IMMEDIATE
// transaction).
#[test]
fn qa_38_a_reader_can_browse_while_the_owner_is_thinking() {
    let home = a_quiet_place();
    {
        let mut cortex = awake_cortex(&home, ConsolidationPolicy::default());
        cortex.create_graph(&graph_meta("stream")).unwrap();
        cortex.apply("stream", add_op("n000"), 1).unwrap();
        cortex.consolidate(Some("stream"), 2).unwrap();
    }

    let db = home.db.clone();
    let writer = std::thread::spawn(move || {
        let mut cortex = Cortex::open(&db, ConsolidationPolicy::default()).unwrap();
        for i in 1..=120i64 {
            let id = format!("n{i:03}");
            let prev = format!("n{:03}", i - 1);
            cortex.apply("stream", add_op(&id), 10 + i).unwrap();
            cortex
                .apply(
                    "stream",
                    Op::Link { from: id.clone(), to: prev.clone(), label: "follows".into() },
                    10 + i,
                )
                .unwrap();
            if i % 12 == 0 {
                cortex
                    .apply("stream", Op::Supersede { old: prev, by: id }, 10 + i)
                    .unwrap();
            }
        }
        cortex.consolidate_all(1_000).unwrap();
    });

    let mut reader = EngramStore::open(&home.db).unwrap();
    let mut browses = 0u32;
    while !writer.is_finished() {
        let snapshot = reader.recall("stream").expect("recall while the owner thinks");
        for edge in &snapshot.edges {
            assert!(
                snapshot.nodes.iter().any(|n| n.id == edge.from)
                    && snapshot.nodes.iter().any(|n| n.id == edge.to),
                "a consolidated snapshot is never torn"
            );
        }
        NeuronGraph::from_data(snapshot).expect("every snapshot is re-thinkable");
        assert!(!reader.list(None, None).unwrap().is_empty());
        let _ = reader.search("body", 5).expect("search while the owner thinks");
        browses += 1;
    }
    writer.join().unwrap();
    assert!(browses > 0, "the reader really browsed mid-thought");

    let final_state = reader.recall("stream").unwrap();
    assert_eq!(final_state.nodes.len(), 121, "the whole stream arrived");
    assert_eq!(final_state.edges.len(), 120);
}
