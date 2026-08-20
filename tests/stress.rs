//! Stress: memory bounds and speed under abuse. Run explicitly:
//!   cargo test --test stress --release -- --ignored --nocapture

use std::time::Instant;

use neuron::{ConsolidationPolicy, Cortex, GraphMeta, GraphStatus, NewNode, Op};

fn meta(id: &str) -> GraphMeta {
    GraphMeta {
        id: id.into(),
        title: format!("graph {id}"),
        status: GraphStatus::Active,
        project: None,
        created: 1,
        updated: 1,
    }
}

fn add(id: String) -> Op {
    Op::AddNode(NewNode {
        kind: "idea".into(),
        title: format!("node {id} about consolidation pressure"),
        content: format!("stress content for {id}: tokens, ceremony, engrams"),
        stage: None,
        skills: vec![],
        id,
    })
}

fn max_rss_mb() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage writes into the zeroed struct we own.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let raw = usage.ru_maxrss as f64;
    if cfg!(target_os = "macos") {
        raw / (1024.0 * 1024.0)
    } else {
        raw / 1024.0
    }
}

#[test]
#[ignore]
fn stress_memory_and_speed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("neurons.db");
    println!("start rss: {:.1} MB", max_rss_mb());

    // S1 — write burst, one graph: 5k nodes + 5k links through the door,
    // default policy so consolidation fires every 10 ops.
    let mut cortex = Cortex::open(&db, ConsolidationPolicy::default()).unwrap();
    cortex.create_graph(&meta("burst")).unwrap();
    let t = Instant::now();
    let mut clock = 10;
    for i in 0..5_000 {
        cortex.apply("burst", add(format!("n{i}")), clock).unwrap();
        clock += 1;
    }
    for i in 1..5_000 {
        let op = Op::Link {
            from: format!("n{i}"),
            to: format!("n{}", i / 2),
            label: "relates".into(),
        };
        cortex.apply("burst", op, clock).unwrap();
        clock += 1;
    }
    let burst = t.elapsed();
    println!(
        "S1 write burst: 10k ops in {:.2?} ({:.0} ops/sec), ~1000 consolidations",
        burst,
        10_000.0 / burst.as_secs_f64()
    );
    assert!(burst.as_secs() < 60, "write path pathologically slow");

    // The burst was 9,999 ops: 9 links short of the last threshold multiple.
    // Dropping WITHOUT a shutdown sweep must lose exactly that tail — the
    // ADR-0002 loss window, demonstrated live.
    drop(cortex);
    let mut peek = Cortex::open(&db, ConsolidationPolicy::default()).unwrap();
    let lost_tail = peek
        .read("burst", clock, |g| g.path("n4999", "n0").unwrap())
        .unwrap();
    assert!(lost_tail.is_none(), "unclean drop should have lost the tail links");
    println!("S1b loss window: unclean drop lost the final 9 links (by design)");
    drop(peek);

    // S2 — churn across 300 graphs with max_loaded 8: every touch of a cold
    // graph forces recall + pressure release. The memory model under abuse.
    let policy = ConsolidationPolicy { max_loaded: 8, ..Default::default() };
    let mut cortex = Cortex::open(&db, policy).unwrap();
    let t = Instant::now();
    for i in 0..300 {
        let id = format!("churn{i}");
        cortex.create_graph(&meta(&id)).unwrap();
        for n in 0..3 {
            cortex.apply(&id, add(format!("n{n}")), clock).unwrap();
            clock += 1;
        }
        assert!(cortex.loaded() <= 8, "cache exceeded max_loaded");
    }
    let created = t.elapsed();
    let t = Instant::now();
    for i in 0..300 {
        let id = format!("churn{i}");
        cortex.apply(&id, Op::Reinforce { id: "n0".into() }, clock).unwrap();
        clock += 1;
        assert!(cortex.loaded() <= 8, "cache exceeded max_loaded on recall");
    }
    let recycled = t.elapsed();
    cortex.apply("burst", Op::Link { from: "n4999".into(), to: "n2499".into(), label: "relates".into() }, clock).unwrap();
    clock += 1;
    cortex.consolidate(Some("burst"), clock).unwrap();
    println!(
        "S2 churn: 300 graphs created+worked in {:.2?}; 300 cold recalls+evictions in {:.2?} ({:.1} ms/cycle); loaded() never exceeded 8",
        created, recycled,
        recycled.as_millis() as f64 / 300.0
    );
    assert!(recycled.as_secs() < 30, "recall/evict cycle pathologically slow");

    // S3 — read latency on the abuse-scale graph (5k nodes, ~166x the
    // 30-node design practice) and on a design-size graph.
    let t = Instant::now();
    let summary = cortex.summary("burst", 5, clock).unwrap();
    let cold_summary = t.elapsed();
    assert_eq!(summary.counts.active, 5_000);
    let t = Instant::now();
    let hood = cortex
        .read("burst", clock, |g| g.neighborhood("n100", 2).unwrap())
        .unwrap();
    let hood_time = t.elapsed();
    let t = Instant::now();
    let path = cortex
        .read("burst", clock, |g| g.path("n4999", "n0").unwrap())
        .unwrap();
    let path_time = t.elapsed();
    assert!(path.is_some(), "chain path must exist after clean consolidation");
    let t = Instant::now();
    let hits = cortex.search("ceremony", 10).unwrap();
    let search_time = t.elapsed();
    println!(
        "S3 abuse-scale reads (5k nodes): summary(recall) {:.2?}, neighborhood(d2) {:.2?} ({} edges), path {:.2?} (len {:?}), search {:.2?} ({} hits)",
        cold_summary, hood_time, hood.out.len() + hood.inc.len(),
        path_time, path.as_ref().map(|p| p.len()), search_time, hits.len()
    );
    assert!(cold_summary.as_secs() < 5 && hood_time.as_secs() < 5 && path_time.as_secs() < 5);

    let t = Instant::now();
    let _ = cortex.summary("churn299", 5, clock).unwrap();
    println!("S3 design-scale read (3 nodes): summary {:.2?}", t.elapsed());

    // S4 — shutdown sweep and cold reopen of everything.
    let t = Instant::now();
    cortex.consolidate_all(clock).unwrap();
    println!("S4 shutdown sweep (301 graphs): {:.2?}", t.elapsed());
    drop(cortex);
    let t = Instant::now();
    let mut reopened = Cortex::open(&db, ConsolidationPolicy::default()).unwrap();
    let s = reopened.summary("burst", 5, clock).unwrap();
    println!("S4 cold reopen + 5k-node recall: {:.2?}", t.elapsed());
    assert_eq!(s.counts.active, 5_000);

    let db_size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    println!(
        "end rss: {:.1} MB; db file: {:.1} MB",
        max_rss_mb(),
        db_size as f64 / (1024.0 * 1024.0)
    );
}
