// QA group D: the cortex - focus, pressure, sweeps, lock. Every assertion
// observes the public side effect the contracts promise: what became visible
// in long-term memory, and when. Each scenario gets its own MemoryHome
// because the cortex lock guards the memory's home directory.

#[path = "qa_dsl.rs"]
mod qa_dsl;

use neuron::{ConsolidationPolicy, Cortex, NodeStatus, Op};
use qa_dsl::*;

// QA-29: one mind owns the memory at a time.
// Contract: DESIGN cortex.rs lock protocol (flock, exclusive, non-blocking,
// held for the cortex lifetime); Principles 4.
#[test]
fn qa_29_one_mind_owns_the_memory_at_a_time() {
    let home = a_quiet_place();
    let first = Cortex::open(&home.db, ConsolidationPolicy::default()).unwrap();

    assert!(
        Cortex::open(&home.db, ConsolidationPolicy::default()).is_err(),
        "a second mind is refused while the first lives"
    );

    // characterization: the lock guards the memory's home directory, so even
    // a different database in the same home is refused.
    assert!(Cortex::open(&home.dir.path().join("other.db"), ConsolidationPolicy::default()).is_err());

    drop(first);
    assert!(
        Cortex::open(&home.db, ConsolidationPolicy::default()).is_ok(),
        "the moment the first is gone, ownership passes"
    );
}

// QA-30: turning focus consolidates the thought you leave behind - thinking
// too small to consolidate on its own reaches long-term memory purely
// because focus left it.
// Contract: ADR-0002 event 5 (FocusSwitch); DESIGN engram.rs staleness
// contract (cold reads see consolidated state only).
#[test]
fn qa_30_turning_focus_consolidates_the_thought_you_leave_behind() {
    let home = a_quiet_place();
    let mut cortex = awake_cortex(&home, lazy_policy());
    cortex.create_graph(&graph_meta("left")).unwrap();
    cortex.create_graph(&graph_meta("entered")).unwrap();

    cortex.apply("left", add_about_op("l1", "leftword"), 10).unwrap();
    cortex.apply("left", add_about_op("l2", "leftword"), 11).unwrap();
    cortex.apply("left", add_about_op("l3", "leftword"), 12).unwrap();
    assert_eq!(cold_hits(&home, "leftword"), 0, "three small thoughts: not consolidated yet");

    cortex.apply("entered", add_about_op("e1", "enteredword"), 20).unwrap();

    assert_eq!(cold_recall(&home, "left").nodes.len(), 3, "leaving focus consolidated them");
    assert_eq!(cold_hits(&home, "leftword"), 3);
    assert_eq!(cold_hits(&home, "enteredword"), 0, "the newly focused mind is the stale one now");

    // characterization (AMB-22): a mere glance at another graph turns focus
    // too, and consolidates the mind left behind.
    cortex.apply("entered", add_about_op("e2", "enteredword"), 21).unwrap();
    let _ = cortex.read("left", 30, |g| g.nodes().len()).unwrap();
    assert_eq!(cold_hits(&home, "enteredword"), 2);
}

// QA-31: a correction never lingers in working memory - lifecycle verbs
// consolidate immediately under even the laziest policy, while plain
// captures wait.
// Contract: ADR-0002 event 2 (hardwired); ADR-0006 (per-verb stimulus);
// DESIGN cortex.rs (classify Lifecycle vs Mutated by verb).
#[test]
fn qa_31_a_correction_never_lingers_in_working_memory() {
    let home = a_quiet_place();
    let mut cortex = awake_cortex(&home, ConsolidationPolicy::exit_only());
    cortex.create_graph(&graph_meta("beliefs")).unwrap();

    cortex.apply("beliefs", add_op("env-vars"), 10).unwrap();
    cortex.apply("beliefs", add_op("config-files"), 11).unwrap();
    assert_eq!(cold_recall(&home, "beliefs").nodes.len(), 0, "captures wait under exit_only");

    cortex
        .apply("beliefs", Op::Supersede { old: "env-vars".into(), by: "config-files".into() }, 12)
        .unwrap();

    let cold = cold_recall(&home, "beliefs");
    assert_eq!(cold.nodes.len(), 2, "the correction carried everything with it");
    let corrected = the_thought(&cold, "env-vars");
    assert_eq!(corrected.status, NodeStatus::Superseded);
    assert_eq!(corrected.superseded_by.as_deref(), Some("config-files"));

    cortex.apply("beliefs", add_op("later-idea"), 13).unwrap();
    assert_eq!(cold_recall(&home, "beliefs").nodes.len(), 2, "a plain capture waits again");

    // Settling is just as precious.
    cortex.apply("beliefs", Op::Settle, 14).unwrap();
    assert_eq!(
        cold_recall(&home, "beliefs").meta.status,
        neuron::GraphStatus::Settled,
        "the settling is cold-visible at once"
    );
}

// QA-32: quiet thoughts settle into long-term memory on the sweep - at the
// quiet boundary exactly, and only when there is dirt.
// Contract: ADR-0002 event 4 (QuietPeriod, default 60); DESIGN cortex.rs
// (tick: Tick per dirty graph); policy table (idle >= quiet).
#[test]
fn qa_32_quiet_thoughts_settle_into_long_term_memory_on_the_sweep() {
    let home = a_quiet_place();
    let policy = ConsolidationPolicy { dirty_threshold: 1_000_000, quiet_secs: 60, max_loaded: 8 };
    let mut cortex = awake_cortex(&home, policy);
    cortex.create_graph(&graph_meta("quiet")).unwrap();

    cortex.apply("quiet", add_about_op("q1", "quietword"), 100).unwrap();
    cortex.apply("quiet", add_about_op("q2", "quietword"), 101).unwrap();

    cortex.tick(160).unwrap();
    assert_eq!(cold_hits(&home, "quietword"), 0, "59 idle seconds is not yet quiet");

    cortex.tick(161).unwrap();
    assert_eq!(cold_hits(&home, "quietword"), 2, "at 60 idle seconds the thoughts settle");

    cortex.tick(1_000_000).unwrap();
    assert_eq!(cold_hits(&home, "quietword"), 2, "a clean mind is left alone by later sweeps");
}

// QA-33: the crowded cortex lets the coldest thought sleep - and loses
// nothing. The released mind's work is cold-visible, and picking it up again
// recalls it whole.
// Contract: ADR-0002 event 7 (consolidate and release least-recently-
// touched); DESIGN cortex.rs (MemoryPressure eviction, load-if-absent).
#[test]
fn qa_33_the_crowded_cortex_lets_the_coldest_thought_sleep_losing_nothing() {
    let home = a_quiet_place();
    let tight = ConsolidationPolicy { dirty_threshold: 1_000_000, quiet_secs: 1_000_000, max_loaded: 1 };
    let mut cortex = awake_cortex(&home, tight);
    cortex.create_graph(&graph_meta("ga")).unwrap();
    cortex.create_graph(&graph_meta("gb")).unwrap();

    cortex.apply("ga", add_about_op("a-thought", "worda"), 10).unwrap();
    assert!(cortex.loaded() <= 1, "the cap holds");

    cortex.apply("gb", add_about_op("b-thought", "wordb"), 20).unwrap();
    assert!(cortex.loaded() <= 1, "the coldest mind was let go");
    assert_eq!(cold_hits(&home, "worda"), 1, "nothing of the sleeping mind was lost");

    cortex.apply("ga", Op::Reinforce { id: "a-thought".into() }, 30).unwrap();
    let reinforced = cortex.read("ga", 31, |g| the_thought_in(g, "a-thought").reinforced).unwrap();
    assert_eq!(reinforced, 2, "the released mind came back whole and kept thinking");

    cortex.consolidate_all(40).unwrap();
    assert_eq!(the_thought(&cold_recall(&home, "ga"), "a-thought").reinforced, 2);
    assert_eq!(cold_hits(&home, "wordb"), 1);
}

// QA-34: sleep consolidates every loose thought. (With focus discipline, at
// most one mind is ever dirty - the focused one; the sweep at shutdown still
// must flush it.)
// Contract: ADR-0002 event 6 (Shutdown, hardwired); DESIGN cortex.rs
// (consolidate_all: the shutdown path; consolidate: the on-demand path).
#[test]
fn qa_34_sleep_consolidates_every_loose_thought() {
    let home = a_quiet_place();
    let mut cortex = awake_cortex(&home, lazy_policy());
    cortex.create_graph(&graph_meta("day")).unwrap();

    cortex.apply("day", add_about_op("d1", "daylight"), 10).unwrap();
    cortex.apply("day", add_about_op("d2", "daylight"), 11).unwrap();
    assert_eq!(cold_hits(&home, "daylight"), 0);

    cortex.consolidate(None, 20).unwrap();
    assert_eq!(cold_hits(&home, "daylight"), 2, "an explicit ask consolidates everything");

    cortex.apply("day", add_about_op("d3", "daylight"), 30).unwrap();
    cortex.consolidate_all(40).unwrap();
    assert_eq!(cold_hits(&home, "daylight"), 3, "sleep consolidates the rest");

    cortex.consolidate_all(50).unwrap();
    assert_eq!(cold_hits(&home, "daylight"), 3, "sleeping twice is harmless");
}

// QA-35: the focused mind's freshest thought is already findable - working
// memory is merged into search for the focused graph only, and a thought is
// never counted twice once consolidated.
// Contract: DESIGN engram.rs staleness contract (hot-graph search merge);
// cortex.rs (search delegates plus hot merge).
#[test]
fn qa_35_the_focused_minds_freshest_thought_is_already_findable() {
    let home = a_quiet_place();
    let mut cortex = awake_cortex(&home, lazy_policy());
    cortex.create_graph(&graph_meta("focus")).unwrap();

    cortex.apply("focus", add_about_op("fresh", "xyzzy"), 10).unwrap();

    let hits = cortex.search("xyzzy", 10).unwrap();
    assert_eq!(hits.len(), 1, "the cortex already finds the fresh thought");
    assert_eq!(hits[0].graph_id, "focus");
    assert_eq!(hits[0].node_id, "fresh");
    assert_eq!(cold_hits(&home, "xyzzy"), 0, "a cold reader cannot see it yet");
    // characterization (AMB-20): a hot, unconsolidated match carries rank 0.
    assert_eq!(hits[0].rank, 0.0);

    cortex.consolidate(Some("focus"), 20).unwrap();
    let hits = cortex.search("xyzzy", 10).unwrap();
    assert_eq!(hits.len(), 1, "consolidated and hot never double-count");
    assert_eq!(cold_hits(&home, "xyzzy"), 1);
}

// QA-36: the tenth thought tips the scale - nine acts of thinking stay in
// working memory; the tenth consolidates them all.
// Contract: ADR-0002 event 3 (DirtyThreshold, default 10); DESIGN policy.rs
// (Mutated: dirty >= dirty_threshold); graph.rs (ops counts every mutation).
#[test]
fn qa_36_the_tenth_thought_tips_the_scale() {
    let home = a_quiet_place();
    let mut cortex = awake_cortex(&home, ConsolidationPolicy::default());
    cortex.create_graph(&graph_meta("scale")).unwrap();

    // Nine acts of thinking: five captures, two links, two reinforcements.
    for (i, id) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
        cortex.apply("scale", add_about_op(id, "scaleword"), 10 + i as i64).unwrap();
    }
    cortex
        .apply("scale", Op::Link { from: "t1".into(), to: "t2".into(), label: "therefore".into() }, 20)
        .unwrap();
    cortex
        .apply("scale", Op::Link { from: "t2".into(), to: "t3".into(), label: "therefore".into() }, 21)
        .unwrap();
    cortex.apply("scale", Op::Reinforce { id: "t4".into() }, 22).unwrap();
    cortex.apply("scale", Op::Reinforce { id: "t4".into() }, 23).unwrap();
    assert_eq!(cold_recall(&home, "scale").nodes.len(), 0, "nine acts: still working memory");

    cortex.apply("scale", Op::Reinforce { id: "t5".into() }, 24).unwrap();

    let cold = cold_recall(&home, "scale");
    assert_eq!(cold.nodes.len(), 5, "the tenth act consolidated everything");
    assert_eq!(cold.edges.len(), 2);
    assert_eq!(the_thought(&cold, "t4").reinforced, 3);
    assert_eq!(the_thought(&cold, "t5").reinforced, 2);
}
