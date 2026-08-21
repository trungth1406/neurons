// QA group B: long-term memory (EngramStore) plus the master cross-seam
// property QA-39. Blind scenario suite from the QA-1 spec.

#[path = "qa_dsl.rs"]
mod qa_dsl;

use neuron::{EngramStore, GraphData, GraphMeta, GraphStatus, NeuronGraph, NewNode, Trace};
use qa_dsl::*;

// QA-17: what was consolidated is exactly what comes back - every field,
// every counter, byte for byte, even for a mind full of odd characters.
// Contract: DESIGN test plan (load/consolidate lossless roundtrip).
#[test]
fn qa_17_what_was_consolidated_is_exactly_what_comes_back() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let mut mind = a_practice_size_cluster();

    consolidate_whole(&mut store, &mut mind);
    let engram = recall_engram(&mut store, "cluster");

    assert_eq!(engram, mind.to_data(), "the engram is the mind, byte for byte");
    assert_eq!(engram.meta.updated, mind.meta().updated);
}

// QA-18: you cannot adopt a snapshot over an existing memory.
// Contract: DESIGN engram.rs (import refuses existing ids - no replace path);
// ADR-0003.
#[test]
fn qa_18_you_cannot_adopt_a_snapshot_over_an_existing_memory() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let mut mind = fresh_mind("kept");
    capture(&mut mind, "original-thought", 10);
    consolidate_whole(&mut store, &mut mind);
    let before = recall_engram(&mut store, "kept");

    let mut usurper = fresh_mind("kept");
    capture(&mut usurper, "usurping-thought", 20);
    assert!(store.import(&usurper.to_data()).is_err());

    assert_eq!(recall_engram(&mut store, "kept"), before, "the original is untouched");
    assert!(store.exists("kept").unwrap());

    // characterization (AMB-12): founding the same memory twice is refused too.
    assert!(store.create(&graph_meta("kept")).is_err());
}

// QA-19: recalling a memory that never formed.
// Contract: DESIGN graph.rs notes (error shapes); engram.rs (exists).
#[test]
fn qa_19_recalling_a_memory_that_never_formed() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);

    assert!(store.recall("ghost").is_err());
    assert!(!store.exists("ghost").unwrap());

    store.create(&graph_meta("just-founded")).unwrap();
    assert!(store.exists("just-founded").unwrap());
    let empty = recall_engram(&mut store, "just-founded");
    assert_eq!(empty.meta, graph_meta("just-founded"));
    assert!(empty.nodes.is_empty() && empty.edges.is_empty());
}

// QA-20: a consolidated thought is findable by its words - even a corrected
// one. Corrected beliefs are kept, not erased.
// Contract: DESIGN engram.rs (search: FTS5 over title and content); graph.rs
// (supersede never deletes).
#[test]
fn qa_20_a_consolidated_thought_is_findable_by_its_words_even_corrected() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let mut mind = fresh_mind("sea");
    mind.add_node(
        NewNode {
            id: "first-guess".into(),
            kind: "idea".into(),
            title: "zebra crossing".into(),
            content: "the quick brown fox".into(),
            stage: None,
            skills: vec![],
        },
        10,
    )
    .unwrap();
    mind.add_node(
        NewNode {
            id: "better-guess".into(),
            kind: "idea".into(),
            title: "zebra stripes theory".into(),
            content: "zebra zebra zebra".into(),
            stage: None,
            skills: vec![],
        },
        11,
    )
    .unwrap();
    correct(&mut mind, "first-guess", "better-guess", 12);
    consolidate_whole(&mut store, &mut mind);

    let by_title = store.search("crossing", 10).unwrap();
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].graph_id, "sea");
    assert_eq!(by_title[0].node_id, "first-guess");
    assert_eq!(by_title[0].title, "zebra crossing");

    let by_content = store.search("fox", 10).unwrap();
    assert_eq!(by_content[0].node_id, "first-guess", "content words count too");

    let hits = store.search("zebra", 10).unwrap();
    assert!(
        hits.iter().any(|h| h.node_id == "first-guess"),
        "a corrected belief still surfaces when you ask for its words"
    );
    // characterization (AMB-09): rank is bm25-negative; better matches rank
    // more negative and come first.
    assert_eq!(hits[0].node_id, "better-guess");
    assert!(hits.windows(2).all(|w| w[0].rank <= w[1].rank));
    assert!(hits.iter().all(|h| h.rank < 0.0));

    assert_eq!(store.search("zebra", 1).unwrap().len(), 1, "the limit is honored");
    assert_eq!(store.search("zebra", 0).unwrap().len(), 0);
}

// QA-21: hostile words neither break the memory nor poison it.
// Contract: DESIGN engram.rs (all SQL lives here - binding discipline);
// AMB-09/AMB-16 characterization for query language and NUL.
#[test]
fn qa_21_hostile_words_neither_break_the_memory_nor_poison_it() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);

    let mut mind = fresh_mind("../up"); // a path-hostile memory id is just data
    let big = "x".repeat(1024 * 1024);
    let hostile = [
        ("", "empty id"),
        ("   ", "whitespace id"),
        ("a\nb\tc\"d", "control characters"),
        ("'; DROP TABLE nodes;--", "sql-shaped id"),
        ("rowid", "keyword id"),
        ("\u{1F4A1}", "astral emoji id"),
        ("\u{6982}\u{5FF5}", "cjk id"),
    ];
    for (i, (id, why)) in hostile.iter().enumerate() {
        mind.add_node(
            NewNode {
                id: (*id).into(),
                kind: format!("kind {why}"),
                title: format!("title {why}"),
                content: format!("content {why} '; DELETE FROM edges;--"),
                stage: None,
                skills: vec![String::new(), "dup".into(), "dup".into()],
            },
            10 + i as i64,
        )
        .unwrap_or_else(|e| panic!("capture {why}: {e}"));
    }
    mind.add_node(
        NewNode {
            id: "big".into(),
            kind: "idea".into(),
            title: "a megabyte of thought".into(),
            content: big.clone(),
            stage: None,
            skills: vec![],
        },
        20,
    )
    .unwrap();
    mind.add_node(
        NewNode {
            id: "nul".into(),
            kind: "idea".into(),
            title: "nul carrier".into(),
            content: "a\u{0}b".into(),
            stage: None,
            skills: vec![],
        },
        21,
    )
    .unwrap();
    mind.link("rowid", "\u{1F4A1}", "label,with(meta)chars", 30).unwrap();
    mind.link("rowid", "\u{1F4A1}", "label,with)other(chars", 31).unwrap();

    consolidate_whole(&mut store, &mut mind);
    let engram = recall_engram(&mut store, "../up");
    assert_eq!(engram, mind.to_data(), "every hostile value round-trips byte-identical");
    assert_eq!(the_thought(&engram, "big").content, big);
    assert_eq!(the_thought(&engram, "nul").content, "a\u{0}b"); // characterization (AMB-16)

    // characterization (AMB-09): the query is raw FTS5 syntax - garbage is an
    // Err, never a panic, and never poisons the store.
    for garbage in ["\"", "*", "", "megabyte AND", "NEAR(", "(((", "^"] {
        let outcome = store.search(garbage, 10);
        drop(outcome); // Err or Ok - the only rule is: no panic, no poison
    }
    assert_eq!(store.search("megabyte", 10).unwrap().len(), 1, "the store still answers");
}

// QA-22: the library lists what it has, sliced how you ask.
// Contract: DESIGN engram.rs (list(status?, project?)).
#[test]
fn qa_22_the_library_lists_what_it_has_sliced_how_you_ask() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    store
        .create(&GraphMeta { project: Some("p1".into()), ..graph_meta("active-p1") })
        .unwrap();
    store
        .create(&GraphMeta {
            status: GraphStatus::Settled,
            project: Some("p1".into()),
            ..graph_meta("settled-p1")
        })
        .unwrap();
    store.create(&graph_meta("active-free")).unwrap();

    let ids = |v: Vec<GraphMeta>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();

    // characterization (AMB-13): the library lists in founding order.
    assert_eq!(
        ids(store.list(None, None).unwrap()),
        vec!["active-p1", "settled-p1", "active-free"]
    );
    assert_eq!(
        ids(store.list(Some(GraphStatus::Active), None).unwrap()),
        vec!["active-p1", "active-free"]
    );
    assert_eq!(ids(store.list(Some(GraphStatus::Settled), None).unwrap()), vec!["settled-p1"]);
    // characterization (AMB-13): a project filter excludes memories that
    // belong to no project.
    assert_eq!(ids(store.list(None, Some("p1")).unwrap()), vec!["active-p1", "settled-p1"]);
    assert_eq!(
        ids(store.list(Some(GraphStatus::Settled), Some("p1")).unwrap()),
        vec!["settled-p1"]
    );
    assert!(store.list(None, Some("no-such-project")).unwrap().is_empty());
}

// QA-23: consolidation moves only what changed, and the newest state wins.
// Contract: DESIGN engram.rs (delta only, O(changed rows), FTS triggers fire
// only for rows actually written); ADR-0003.
#[test]
fn qa_23_consolidation_moves_only_what_changed_and_newest_wins() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let mut mind = fresh_mind("delta");
    for i in 0..20 {
        capture(&mut mind, &format!("n{i:02}"), 10 + i);
    }
    consolidate_whole(&mut store, &mut mind);
    let first = recall_engram(&mut store, "delta");

    mind.add_node(idea_about("n20", "freshword"), 40).unwrap();
    mind.reinforce("n05", 41).unwrap();
    let trace = mind.take_trace();
    assert_eq!(trace.nodes.len(), 2, "the trace carries only the touched thoughts");
    store.consolidate("delta", &trace).unwrap();

    let second = recall_engram(&mut store, "delta");
    assert_eq!(second, mind.to_data());
    assert_eq!(the_thought(&second, "n05").reinforced, 2);
    for i in 0..20 {
        if i == 5 {
            continue;
        }
        let id = format!("n{i:02}");
        assert_eq!(the_thought(&second, &id), the_thought(&first, &id), "{id} untouched");
    }
    assert_eq!(store.search("freshword", 10).unwrap().len(), 1, "the new words are findable");

    // The newest state wins on re-consolidation of the same thought.
    reinforce_belief(&mut mind, "n20", 2, 50);
    store.consolidate("delta", &mind.take_trace()).unwrap();
    assert_eq!(the_thought(&recall_engram(&mut store, "delta"), "n20").reinforced, 3);

    // A trace without meta leaves the graphs row alone.
    let updated_before = recall_engram(&mut store, "delta").meta.updated;
    let mut meta_less = Trace::default();
    let mut row = the_thought(&recall_engram(&mut store, "delta"), "n20").clone();
    row.reinforced = 9;
    meta_less.nodes.push(row);
    store.consolidate("delta", &meta_less).unwrap();
    let after = recall_engram(&mut store, "delta");
    assert_eq!(the_thought(&after, "n20").reinforced, 9);
    assert_eq!(after.meta.updated, updated_before);
}

// QA-24: an empty trace is a harmless consolidation - the shape an on-demand
// consolidation of a clean mind produces.
// Contract: DESIGN policy.rs (OnDemand always) + engram.rs; the composition
// requires this to be safe.
#[test]
fn qa_24_an_empty_trace_is_a_harmless_consolidation() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let mut mind = fresh_mind("calm");
    capture(&mut mind, "a", 10);
    consolidate_whole(&mut store, &mut mind);
    let before = recall_engram(&mut store, "calm");

    store.consolidate("calm", &Trace::default()).unwrap();
    assert_eq!(recall_engram(&mut store, "calm"), before);

    // characterization (AMB-14): thoughts for a memory that was never founded
    // are refused outright (foreign keys), never stored as orphans.
    let mut orphan = Trace::default();
    orphan.nodes.push(the_thought(&before, "a").clone());
    assert!(store.consolidate("never-founded", &orphan).is_err());
    assert!(!store.exists("never-founded").unwrap());
}

// QA-25: a memory from the future is refused.
// Contract: ADR-0005 (a database whose history contains a migration this
// binary does not embed refuses to open).
#[test]
fn qa_25_a_memory_from_the_future_is_refused() {
    let home = a_quiet_place();
    {
        let _ = long_term_memory(&home); // forms the memory at today's schema
    }
    {
        let conn = rusqlite::Connection::open(&home.db).unwrap();
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
             VALUES (999, 'from_the_future', '2099-01-01T00:00:00Z', '12345')",
            [],
        )
        .unwrap();
    }
    assert!(EngramStore::open(&home.db).is_err(), "an older binary must refuse");
}

// QA-26: two ids differing only in case - or only in unicode normalization -
// are two different thoughts, in the mind and in the memory.
// Contract: owner ruling on AMB-08 - ids and edge keys are opaque byte
// strings; no folding, no normalization. This is a CONTRACT test.
#[test]
fn qa_26_two_ids_differing_only_in_case_are_two_thoughts_everywhere() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);

    let mut mind = fresh_mind("ids");
    capture(&mut mind, "Idea", 10);
    capture(&mut mind, "idea", 11);
    capture(&mut mind, "caf\u{e9}", 12); // NFC: e-acute
    capture(&mut mind, "cafe\u{301}", 13); // NFD: e + combining acute
    assert_eq!(mind.nodes().len(), 4);

    consolidate_whole(&mut store, &mut mind);
    let engram = recall_engram(&mut store, "ids");
    assert_eq!(engram.nodes.len(), 4, "all four survive as distinct thoughts");
    for id in ["Idea", "idea", "caf\u{e9}", "cafe\u{301}"] {
        assert_eq!(the_thought(&engram, id).id, id);
    }

    // Graph ids are byte-exact too.
    store.create(&graph_meta("G")).unwrap();
    store.create(&graph_meta("g")).unwrap();
    assert!(store.exists("G").unwrap() && store.exists("g").unwrap());
    assert_eq!(recall_engram(&mut store, "G").meta.id, "G");
    assert_eq!(recall_engram(&mut store, "g").meta.id, "g");

    // characterization (AMB-09): word-search folds case; identity never does.
    assert!(!store.search("IDEA", 10).unwrap().is_empty());
}

// QA-15 (memory half) - DEFECT: the memory adopts a snapshot the mind cannot
// re-think. import() accepts GraphData whose edge names a thought that is not
// there; recall() then hands back a snapshot NeuronGraph::from_data refuses,
// and Cortex::apply on that graph fails with "node does not exist". The
// engram is unrecallable in the only sense that matters.
// Contract violated: ADR-0004 ("recall brings an engram back"), DESIGN
// Principles 1 (storage is a snapshot sink for what the mind holds), DESIGN
// test plan (lossless roundtrip). Expected: import refuses what from_data
// refuses. Observed: import accepts it. Filed as a defect issue.
#[test]
#[ignore = "DEFECT(QA-15): import accepts a dangling-edge snapshot that recall/from_data cannot rebuild - see gh issue"]
fn qa_15_defect_the_memory_must_refuse_a_snapshot_the_mind_cannot_rethink() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let corpse = GraphData {
        meta: graph_meta("corpse"),
        nodes: vec![],
        edges: vec![neuron::Edge {
            from: "nope".into(),
            to: "also-nope".into(),
            label: "r".into(),
            weight: 1,
            created: 1,
        }],
    };
    // Expected per contract: the memory refuses what the mind cannot rebuild.
    assert!(
        store.import(&corpse).is_err(),
        "import must refuse a snapshot from_data refuses"
    );
}

// QA-39: the mind and the memory never disagree - the master cross-seam
// property over the whole topology catalog, including meta.updated.
// Contract: DESIGN graph.rs invariants (every mutation stamps meta.updated),
// AMB-21 ruling (every mutation dirties the graphs row), test plan (lossless
// roundtrip), Principles 1.
#[test]
fn qa_39_the_mind_and_the_memory_never_disagree() {
    let home = a_quiet_place();
    let mut store = long_term_memory(&home);
    let minds = [
        a_chain_of_reasoning(&["p", "q", "r", "s", "t", "u"]),
        competing_explanations("obs", &["h1", "h2", "h3", "h4"]),
        one_idea_many_implications("claim", &["i1", "i2", "i3", "i4"]),
        circular_reasoning(&["a", "b", "c", "d"]),
        two_routes_to_the_same_conclusion(),
        disconnected_musings(),
        a_lineage_of_corrections(),
        a_practice_size_cluster(),
    ];
    for mut mind in minds {
        let id = mind.meta().id.clone();
        consolidate_whole(&mut store, &mut mind);
        let engram = recall_engram(&mut store, &id);

        assert_eq!(engram, mind.to_data(), "{id}: the engram is the mind");
        assert_eq!(engram.meta.updated, mind.meta().updated, "{id}: the graphs row kept up");

        let recollected = NeuronGraph::from_data(engram).expect("recall must be re-thinkable");
        assert_eq!(recollected.summary(10), mind.summary(10), "{id}: same overview");
        let ids: Vec<String> = mind.nodes().iter().map(|n| n.id.clone()).collect();
        if ids.len() >= 2 {
            assert_eq!(
                recollected.path(&ids[0], &ids[1]).unwrap(),
                mind.path(&ids[0], &ids[1]).unwrap(),
                "{id}: same routes"
            );
        }
    }
}
