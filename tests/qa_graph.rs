// QA group A: the graph of thinking (pure core, no database).
// Blind scenario suite from the QA-1 spec; contracts cited per test.

#[path = "qa_dsl.rs"]
mod qa_dsl;

use neuron::{GraphStatus, NeuronGraph, NodeStatus, Op};
use qa_dsl::*;

// QA-01: an idea can only be captured once, and a refusal changes nothing.
// Contract: DESIGN graph.rs (add_node dup id = Err); Invariants (a refusal is
// not a mutation, so no dirt).
#[test]
fn qa_01_an_idea_can_only_be_captured_once() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "heap-not-stack", 10);
    let before = mind.to_data();
    let dirt_before = mind.dirty();

    assert!(mind.add_node(idea("heap-not-stack"), 11).is_err());

    assert_eq!(mind.to_data(), before, "a refused capture must change nothing");
    assert_eq!(mind.dirty(), dirt_before, "a refused capture is not thinking");
}

// QA-02: a re-confirmed link grows stronger; a differently-worded link between
// the same two thoughts is its own connection.
// Contract: DESIGN graph.rs (repeat = weight+1); edge identity (from,to,label).
// characterization (AMB-01): a first link starts at weight 1.
#[test]
fn qa_02_a_reconfirmed_link_grows_stronger_not_duplicated() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "async", 10);
    capture(&mut mind, "backpressure", 11);

    connect(&mut mind, "async", "causes", "backpressure", 12);
    let first = mind.edges().iter().find(|e| e.label == "causes").unwrap();
    assert_eq!(first.weight, 1); // characterization (AMB-01)

    reconfirm(&mut mind, "async", "causes", "backpressure", 4, 13);
    connect(&mut mind, "async", "mitigates", "backpressure", 14);

    let causes = mind.edges().iter().find(|e| e.label == "causes").unwrap();
    let mitigates = mind.edges().iter().find(|e| e.label == "mitigates").unwrap();
    assert_eq!(mind.edges().len(), 2, "one connection per (from, to, label)");
    assert_eq!(causes.weight, 5, "five confirmations strong");
    assert_eq!(mitigates.weight, 1, "the other reading lives its own life");
}

// QA-03: a connection needs both thoughts to exist; no half-made link lingers.
// Contract: DESIGN graph.rs (link missing = Err; Invariants).
#[test]
fn qa_03_a_connection_needs_both_thoughts_to_exist() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "premise", 10);
    let dirt_before = mind.dirty();

    assert!(mind.link("premise", "never-captured", "therefore", 11).is_err());
    assert!(mind.link("never-captured", "premise", "therefore", 12).is_err());

    assert!(mind.edges().is_empty(), "no half-made connection");
    assert_eq!(mind.dirty(), dirt_before);
    assert_eq!(
        mind.path("premise", "premise").unwrap(),
        Some(vec!["premise".to_string()]),
        "the mind still answers as if nothing happened"
    );
}

// QA-04: reinforcing a belief counts every confirmation, one by one.
// Contract: DESIGN graph.rs (reinforce reinforced+1; outbox ops counts
// mutations).
// characterization (AMB-01-adjacent): capturing an idea counts as its first
// reinforcement, so a fresh thought reads 1.
#[test]
fn qa_04_reinforcing_a_belief_counts_every_confirmation() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "belief", 10);
    assert_eq!(the_thought_in(&mind, "belief").reinforced, 1); // characterization

    let dirt_before = mind.dirty();
    reinforce_belief(&mut mind, "belief", 4, 11);

    assert_eq!(the_thought_in(&mind, "belief").reinforced, 5);
    assert_eq!(mind.dirty() - dirt_before, 4, "four separate acts of thinking");
}

// QA-05: a corrected belief survives with a forwarding address, and a chain of
// corrections still reads end to end.
// Contract: DESIGN graph.rs (supersede: status=Superseded, superseded_by=by;
// never deletes).
#[test]
fn qa_05_a_corrected_belief_survives_with_a_forwarding_address() {
    let mind = a_lineage_of_corrections();

    assert_eq!(mind.nodes().len(), 4, "nothing was deleted by correcting");
    assert_eq!(mind.edges().len(), 1, "its connections still hold");

    let v1 = the_thought_in(&mind, "v1");
    assert_eq!(v1.status, NodeStatus::Superseded);
    assert_eq!(v1.superseded_by.as_deref(), Some("v2"));
    let v2 = the_thought_in(&mind, "v2");
    assert_eq!(v2.superseded_by.as_deref(), Some("v3"));
    let v3 = the_thought_in(&mind, "v3");
    assert_eq!(v3.status, NodeStatus::Active, "the head of the lineage lives");
    assert_eq!(v3.superseded_by, None);

    let counts = mind.summary(10).counts;
    assert_eq!((counts.active, counts.superseded), (2, 2));
}

// QA-06: correcting with a phantom, correcting twice, correcting with itself.
// Contract: SILENT (AMB-04) - all three recorded as observed.
#[test]
fn qa_06_correcting_with_a_phantom_replacement_is_refused() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "a", 10);
    capture(&mut mind, "b", 11);
    capture(&mut mind, "c", 12);

    // characterization (AMB-04): a phantom replacement is refused.
    assert!(mind.supersede("a", "ghost", 13).is_err());
    assert_eq!(the_thought_in(&mind, "a").superseded_by, None);

    // characterization (AMB-04): re-correcting is allowed; the latest wins.
    correct(&mut mind, "a", "b", 14);
    correct(&mut mind, "a", "c", 15);
    assert_eq!(the_thought_in(&mind, "a").superseded_by.as_deref(), Some("c"));

    // characterization (AMB-04): a thought cannot correct itself.
    assert!(mind.supersede("b", "b", 16).is_err());
}

// QA-07: circular reasoning does not trap the mind; a self-referential thought
// is a legal (if odd) shape.
// Contract: DESIGN graph.rs (neighborhood BFS; path via topo).
#[test]
fn qa_07_circular_reasoning_does_not_trap_the_mind() {
    let mut mind = circular_reasoning(&["a", "b", "c", "d"]);

    // Each side walks its own direction, so a cycle's connections may appear
    // once per side - but never twice within one side.
    let around = mind.neighborhood("a", 10).unwrap();
    for side in [&around.out, &around.inc] {
        let mut seen = std::collections::HashSet::new();
        for (e, _) in side {
            assert!(
                seen.insert((e.from.clone(), e.to.clone(), e.label.clone())),
                "each connection reported once per side"
            );
        }
    }
    assert_eq!(
        mind.path("b", "a").unwrap(),
        Some(vec!["b".into(), "c".into(), "d".into(), "a".into()]),
        "the way back exists by going around"
    );

    // characterization (AMB-03): a self-loop is legal and shows up as both an
    // implication and a support of the same thought.
    mind.link("a", "a", "defines-itself", 50).unwrap();
    let around = mind.neighborhood("a", 1).unwrap();
    assert!(around.out.iter().any(|(e, _)| e.to == "a" && e.from == "a"));
    assert!(around.inc.iter().any(|(e, _)| e.to == "a" && e.from == "a"));
}

// QA-08: a thought knows both its implications and its support; each side of
// the neighborhood walks its own direction.
// Contract: DESIGN graph.rs (neighborhood BFS both directions, depth).
#[test]
fn qa_08_a_thought_knows_its_implications_and_its_support() {
    let mind = competing_explanations("observation", &["h1", "h2", "h3", "h4"]);
    let around = mind.neighborhood("observation", 1).unwrap();
    assert_eq!(around.center.id, "observation");
    assert_eq!(around.out.len(), 0);
    let supporters: Vec<&str> = around.inc.iter().map(|(_, b)| b.id.as_str()).collect();
    for h in ["h1", "h2", "h3", "h4"] {
        assert!(supporters.contains(&h), "{h} supports the observation");
    }

    let mind = one_idea_many_implications("claim", &["i1", "i2", "i3", "i4"]);
    let around = mind.neighborhood("claim", 1).unwrap();
    assert_eq!(around.inc.len(), 0);
    assert_eq!(around.out.len(), 4);

    assert!(mind.neighborhood("never-captured", 1).is_err());

    // Depth follows each direction onward: on a -> x -> y the second step is
    // visible at depth 2.
    let mut chain = a_chain_of_reasoning(&["a", "x", "y"]);
    let around = chain.neighborhood("a", 2).unwrap();
    assert!(around.out.iter().any(|(e, _)| e.from == "x" && e.to == "y"));

    // characterization (AMB-05): each side walks only its own direction, so a
    // sibling explanation (b -> x <- a) stays invisible from a at any depth.
    capture(&mut chain, "b", 90);
    connect(&mut chain, "b", "explains", "x", 91);
    let around = chain.neighborhood("a", 3).unwrap();
    assert!(!around.out.iter().any(|(_, b)| b.id == "b"));
    assert!(!around.inc.iter().any(|(_, b)| b.id == "b"));

    // characterization (AMB-05): depth 0 means just the thought itself.
    let around = chain.neighborhood("a", 0).unwrap();
    assert!(around.out.is_empty() && around.inc.is_empty());
}

// QA-09: the line of reasoning between two thoughts.
// Contract: DESIGN graph.rs (path: shortest, Option, via topo).
#[test]
fn qa_09_the_line_of_reasoning_between_two_thoughts() {
    let ids = ["premise", "s1", "s2", "s3", "s4", "conclusion"];
    let chain = a_chain_of_reasoning(&ids);
    assert_eq!(
        chain.path("premise", "conclusion").unwrap(),
        Some(ids.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        "the whole chain, in order"
    );

    // characterization (AMB-02): the route follows the direction thinking
    // flows - there is no route backwards along a one-way chain.
    assert_eq!(chain.path("conclusion", "premise").unwrap(), None);

    // characterization (AMB-02): the route from a thought to itself is just
    // the thought.
    assert_eq!(chain.path("premise", "premise").unwrap(), Some(vec!["premise".into()]));

    let diamond = two_routes_to_the_same_conclusion();
    let route = diamond.path("a", "d").unwrap().expect("a route exists");
    assert_eq!(route.len(), 3, "the shortest route, whichever branch");
    assert_eq!((route[0].as_str(), route[2].as_str()), ("a", "d"));

    let musings = disconnected_musings();
    assert_eq!(musings.path("p1", "m2").unwrap(), None, "no route between clusters");

    assert!(chain.path("ghost", "conclusion").is_err(), "unknown thoughts are refused");
}

// QA-10: the mind's overview - freshest thinking up front, strongest living
// beliefs on top, honest counts.
// Contract: DESIGN graph.rs (summary: frontier = newest active, top = most
// reinforced; counts).
#[test]
fn qa_10_the_minds_overview_freshest_up_front_strongest_on_top() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "old-truth", 10);
    reinforce_belief(&mut mind, "old-truth", 9, 11);
    capture(&mut mind, "new-truth", 20);
    reinforce_belief(&mut mind, "new-truth", 4, 21);
    correct(&mut mind, "old-truth", "new-truth", 30);
    capture(&mut mind, "fresh-a", 40);
    capture(&mut mind, "fresh-b", 50);

    let overview = mind.summary(10);
    assert_eq!((overview.counts.active, overview.counts.superseded, overview.counts.parked), (3, 1, 0));

    let frontier: Vec<&str> = overview.frontier.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(frontier[0], "fresh-b", "newest first");
    assert_eq!(frontier[1], "fresh-a");
    assert!(!frontier.contains(&"old-truth"), "corrected beliefs left the frontier");

    let top: Vec<&str> = overview.top.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(top[0], "new-truth", "the strongest living belief leads");
    // characterization (AMB-06): the overview celebrates only living beliefs -
    // a heavily reinforced but corrected thought is absent from top.
    assert!(!top.contains(&"old-truth"));

    assert_eq!(mind.summary(1).frontier.len(), 1, "the limit is honored");
    assert_eq!(mind.summary(0).frontier.len(), 0);
    assert_eq!(mind.summary(0).top.len(), 0);
}

// QA-11: a train of thought settles, and can wake again; the settling alone is
// what the day's trace carries.
// Contract: DESIGN graph.rs (settle/reopen: meta.status flip; every mutation
// records into the outbox).
#[test]
fn qa_11_a_train_of_thought_settles_and_can_wake_again() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "a", 10);
    let _ = mind.take_trace(); // the day so far is already consolidated

    mind.settle(20);
    assert_eq!(mind.meta().status, GraphStatus::Settled);
    let trace = mind.take_trace();
    let settled_meta = trace.meta.expect("the settling itself is the trace");
    assert_eq!(settled_meta.status, GraphStatus::Settled);
    assert!(trace.nodes.is_empty() && trace.edges.is_empty());

    mind.reopen(30);
    assert_eq!(mind.meta().status, GraphStatus::Active);

    // characterization (AMB-04): settling an already-settled mind still counts
    // as an act of thinking (dirt accrues), and the status simply stays.
    mind.settle(40);
    let dirt = mind.dirty();
    mind.settle(41);
    assert_eq!(mind.meta().status, GraphStatus::Settled);
    assert_eq!(mind.dirty(), dirt + 1);

    // characterization (AMB-04): a settled mind still accepts new ideas.
    assert!(mind.add_node(idea("late-idea"), 50).is_ok());
}

// QA-12: the journal remembers where things stand, not the play-by-play.
// Contract: DESIGN graph.rs (outbox keyed maps = last-wins dedup; take_trace
// copies current state; drain); types.rs Trace::is_empty; AMB-21 ruling
// (every mutation dirties the graphs row, so the trace carries meta).
#[test]
fn qa_12_the_journal_remembers_where_things_stand_not_the_play_by_play() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "belief", 10);
    capture(&mut mind, "evidence", 11);
    connect(&mut mind, "belief", "rests-on", "evidence", 12);
    let _ = mind.take_trace();
    assert_eq!(mind.dirty(), 0, "drained clean");

    reinforce_belief(&mut mind, "belief", 5, 20);
    reconfirm(&mut mind, "belief", "rests-on", "evidence", 3, 21);
    assert_eq!(mind.dirty(), 8, "eight separate acts of thinking");

    let trace = mind.take_trace();
    assert!(trace.meta.is_some(), "every mutation keeps the graphs row honest");
    assert_eq!(trace.nodes.len(), 1, "one row for the one touched thought");
    assert_eq!(trace.nodes[0].reinforced, 6, "the present state, not the play-by-play");
    assert_eq!(trace.edges.len(), 1);
    assert_eq!(trace.edges[0].weight, 4);
    assert!(trace.deleted_nodes.is_empty() && trace.deleted_edges.is_empty());

    assert_eq!(mind.dirty(), 0);
    assert!(mind.take_trace().is_empty(), "a second drain carries nothing");
}

// QA-13: the uniform door and the named path think identically.
// Contract: ADR-0006 (apply routes to the named methods).
#[test]
fn qa_13_the_uniform_door_and_the_named_path_think_identically() {
    let mut named = fresh_mind("m");
    named.add_node(idea("a"), 10).unwrap();
    named.add_node(idea("b"), 11).unwrap();
    named.link("a", "b", "therefore", 12).unwrap();
    named.reinforce("b", 13).unwrap();
    named.set_stage("b", "confirmed", 14).unwrap();
    named.supersede("a", "b", 15).unwrap();
    named.settle(16);
    named.reopen(17);

    let mut door = fresh_mind("m");
    door.apply(Op::AddNode(idea("a")), 10).unwrap();
    door.apply(Op::AddNode(idea("b")), 11).unwrap();
    door.apply(
        Op::Link { from: "a".into(), to: "b".into(), label: "therefore".into() },
        12,
    )
    .unwrap();
    door.apply(Op::Reinforce { id: "b".into() }, 13).unwrap();
    door.apply(Op::SetStage { id: "b".into(), stage: "confirmed".into() }, 14).unwrap();
    door.apply(Op::Supersede { old: "a".into(), by: "b".into() }, 15).unwrap();
    door.apply(Op::Settle, 16).unwrap();
    door.apply(Op::Reopen, 17).unwrap();

    assert_eq!(named.to_data(), door.to_data());
    assert_eq!(named.dirty(), door.dirty());
    assert_eq!(
        named.add_node(idea("a"), 20).is_err(),
        door.apply(Op::AddNode(idea("a")), 20).is_err(),
        "refusals agree too"
    );
}

// QA-14: a snapshot of the mind rebuilds the same mind - same data, same
// routes, same neighborhoods, same overview.
// Contract: DESIGN graph.rs (from_data rebuilds topo + ids); Principles 2.
#[test]
fn qa_14_a_snapshot_of_the_mind_rebuilds_the_same_mind() {
    let minds = [
        a_chain_of_reasoning(&["p", "q", "r", "s"]),
        competing_explanations("obs", &["h1", "h2", "h3"]),
        circular_reasoning(&["a", "b", "c"]),
        two_routes_to_the_same_conclusion(),
        disconnected_musings(),
        a_lineage_of_corrections(),
        a_practice_size_cluster(),
    ];
    for original in minds {
        let recollected = NeuronGraph::from_data(original.to_data()).expect("recollect");
        assert_eq!(original.to_data(), recollected.to_data());
        assert_eq!(original.summary(10), recollected.summary(10));
        let ids: Vec<String> = original.nodes().iter().map(|n| n.id.clone()).collect();
        if ids.len() >= 2 {
            assert_eq!(
                original.path(&ids[0], &ids[1]).unwrap(),
                recollected.path(&ids[0], &ids[1]).unwrap()
            );
            // characterization (AMB-05-adjacent): a recollected mind gives the
            // same neighborhood as a set; the listing order may differ from
            // the original's (no ordering promise exists - presentation
            // ordering belongs to the adapters per DESIGN's budget note).
            assert_eq!(
                sorted_connections(original.neighborhood(&ids[0], 2).unwrap()),
                sorted_connections(recollected.neighborhood(&ids[0], 2).unwrap())
            );
        }
    }
}

/// Center plus both connection lists in a canonical order, for
/// order-insensitive neighborhood comparison.
fn sorted_connections(n: neuron::Neighborhood) -> (neuron::Node, Vec<String>, Vec<String>) {
    let key = |list: &[(neuron::Edge, neuron::NodeBrief)]| {
        let mut v: Vec<String> = list
            .iter()
            .map(|(e, b)| format!("{}|{}|{}|{}|{}", e.from, e.to, e.label, e.weight, b.id))
            .collect();
        v.sort();
        v
    };
    let out = key(&n.out);
    let inc = key(&n.inc);
    (n.center, out, inc)
}

// QA-15 (mind half): the mind refuses a snapshot it cannot think in - a
// connection to a missing thought, or two thoughts with one id.
// Contract: DESIGN graph.rs (from_data -> Result; error shapes).
#[test]
fn qa_15_the_mind_refuses_a_snapshot_with_a_dangling_connection() {
    let mut sane = fresh_mind("m");
    capture(&mut sane, "a", 10);

    let mut dangling = sane.to_data();
    dangling.edges.push(neuron::Edge {
        from: "a".into(),
        to: "missing".into(),
        label: "r".into(),
        weight: 1,
        created: 1,
    });
    assert!(NeuronGraph::from_data(dangling).is_err());

    let mut duplicated = sane.to_data();
    let twin = duplicated.nodes[0].clone();
    duplicated.nodes.push(twin);
    assert!(NeuronGraph::from_data(duplicated).is_err());
}

// QA-16: marking where a thought stands.
// Contract: DESIGN graph.rs (set_stage; missing = Err). Un-staging back to
// None is not expressible through the door (stage is &str) - AMB-04.
#[test]
fn qa_16_marking_where_a_thought_stands() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "idea", 10);
    assert_eq!(the_thought_in(&mind, "idea").stage, None);

    mind.set_stage("idea", "hypothesis", 11).unwrap();
    assert_eq!(the_thought_in(&mind, "idea").stage.as_deref(), Some("hypothesis"));

    mind.set_stage("idea", "confirmed", 12).unwrap();
    assert_eq!(the_thought_in(&mind, "idea").stage.as_deref(), Some("confirmed"));

    assert!(mind.set_stage("ghost", "hypothesis", 13).is_err());
}

// QA-41: parking a thought - the third status the vocabulary promises.
// Pending ticket #13 (T9): no public verb parks a thought yet; NodeStatus::
// Parked and counts.parked exist but are unreachable through the door. When
// Op::Park/Unpark land, this scenario asserts: parking is a Mutation, parked
// thoughts leave frontier and top, counts.parked tracks them, unparking
// restores them.
// Fulfilled by ticket #13: a thought set aside is not wrong, not gone -
// it leaves the overview, keeps its connections, and wakes on demand.
#[test]
fn qa_41_a_parked_thought_rests_outside_the_overview() {
    let mut mind = fresh_mind("m");
    capture(&mut mind, "later-maybe", 10);
    capture(&mut mind, "now", 11);
    mind.link("later-maybe", "now", "relates", 12).unwrap();

    mind.apply(Op::Park { id: "later-maybe".into() }, 20).unwrap();
    let overview = mind.summary(5);
    assert_eq!(overview.counts.parked, 1);
    assert!(
        !overview.frontier.iter().any(|b| b.id == "later-maybe"),
        "a parked thought rests outside the overview"
    );
    assert_eq!(mind.edges().len(), 1, "its connections survive the rest");

    mind.apply(Op::Unpark { id: "later-maybe".into() }, 30).unwrap();
    assert_eq!(mind.summary(5).counts.parked, 0, "and it wakes on demand");
}
