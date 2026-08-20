use neuron::{ConsolidationPolicy, Response, Stimulus};

use Response::{Consolidate, ConsolidateAndRelease, Ignore};
use Stimulus::*;

#[test]
fn the_decision_table() {
    let p = ConsolidationPolicy::default(); // threshold 10, quiet 60, max 8
    let rows: &[(Stimulus, u32, i64, usize, Response)] = &[
        (OnDemand, 0, 0, 1, Consolidate),
        (OnDemand, 5, 0, 1, Consolidate),
        (Lifecycle, 0, 0, 1, Consolidate),
        (Lifecycle, 1, 0, 1, Consolidate),
        (Mutated, 9, 0, 1, Ignore),
        (Mutated, 10, 0, 1, Consolidate),
        (Mutated, 11, 0, 1, Consolidate),
        (Tick, 0, 999, 1, Ignore),
        (Tick, 3, 59, 1, Ignore),
        (Tick, 3, 60, 1, Consolidate),
        (FocusSwitch, 0, 0, 1, Ignore),
        (FocusSwitch, 1, 0, 1, Consolidate),
        (Shutdown, 0, 0, 1, Ignore),
        (Shutdown, 1, 0, 1, Consolidate),
        (MemoryPressure, 0, 0, 8, Ignore),
        (MemoryPressure, 0, 0, 9, ConsolidateAndRelease),
    ];
    for &(stimulus, dirty, idle, loaded, expected) in rows {
        assert_eq!(
            p.evaluate(stimulus, dirty, idle, loaded),
            expected,
            "row: {stimulus:?} dirty={dirty} idle={idle} loaded={loaded}"
        );
    }
}

#[test]
fn exit_only_ignores_everything_but_the_hardwired() {
    let p = ConsolidationPolicy::exit_only();
    assert_eq!(p.evaluate(Mutated, 1_000_000, 0, 1), Ignore);
    assert_eq!(p.evaluate(Tick, 5, i64::MAX - 1, 1), Ignore);
    assert_eq!(p.evaluate(FocusSwitch, 5, 0, 1), Consolidate);
    assert_eq!(p.evaluate(Shutdown, 1, 0, 1), Consolidate);
    assert_eq!(p.evaluate(Lifecycle, 0, 0, 1), Consolidate);
}
