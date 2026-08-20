// QA group C: the consolidation policy - pure stimulus -> response decisions.
// Blind scenario suite from the QA-1 spec; the phase-1 policy edge table
// (rows P1-P25) executed as data.

use neuron::{ConsolidationPolicy, Response, Stimulus};

const DEFAULTS: ConsolidationPolicy = ConsolidationPolicy {
    dirty_threshold: 10,
    quiet_secs: 60,
    max_loaded: 8,
};

fn knobs(dirty_threshold: u32, quiet_secs: i64, max_loaded: usize) -> ConsolidationPolicy {
    ConsolidationPolicy { dirty_threshold, quiet_secs, max_loaded }
}

// QA-27: the full stimulus-response table, every boundary, exactly as the
// decision table promises.
// Contract: DESIGN policy.rs decision table; ADR-0002 (events 1-7, defaults).
#[test]
fn qa_27_the_full_stimulus_response_table_every_boundary() {
    use Response::{Consolidate, ConsolidateAndRelease, Ignore};
    use Stimulus::{FocusSwitch, Lifecycle, MemoryPressure, Mutated, OnDemand, Shutdown, Tick};

    assert_eq!(ConsolidationPolicy::default(), DEFAULTS, "the documented defaults");

    #[rustfmt::skip]
    let table: &[(&str, ConsolidationPolicy, Stimulus, u32, i64, usize, Response)] = &[
        // P1/P2: an explicit ask always consolidates, even a clean mind.
        ("P1 on-demand clean",      DEFAULTS, OnDemand, 0, 0, 0, Consolidate),
        ("P2 on-demand knobs-max",  knobs(u32::MAX, i64::MAX, usize::MAX), OnDemand, u32::MAX, 0, 0, Consolidate),
        // P3/P4: corrections are too precious to lose - hardwired.
        ("P3 lifecycle clean",      DEFAULTS, Lifecycle, 0, 0, 0, Consolidate),
        ("P4 lifecycle knobs-max",  knobs(u32::MAX, i64::MAX, usize::MAX), Lifecycle, 0, 0, 0, Consolidate),
        // P5-P8: the dirty threshold at and around its boundary.
        ("P5 below threshold",      DEFAULTS, Mutated, 9, 0, 0, Ignore),
        ("P6 at threshold",         DEFAULTS, Mutated, 10, 0, 0, Consolidate),
        ("P7 threshold unreachable", knobs(u32::MAX, 60, 8), Mutated, 5, 0, 0, Ignore),
        ("P8 every-thought policy", knobs(1, 60, 8), Mutated, 1, 0, 0, Consolidate),
        // P10-P13, P15: the quiet period needs dirt, and fires at equality.
        ("P10 idle but clean",      DEFAULTS, Tick, 0, i64::MAX, 0, Ignore),
        ("P11 below quiet",         DEFAULTS, Tick, 1, 59, 0, Ignore),
        ("P12 at quiet",            DEFAULTS, Tick, 1, 60, 0, Consolidate),
        ("P13 zero quiet",          knobs(10, 0, 8), Tick, 1, 0, 0, Consolidate),
        ("P15 quiet at max",        knobs(10, i64::MAX, 8), Tick, 1, i64::MAX, 0, Consolidate),
        // P16/P17: leaving focus consolidates any dirt, and only dirt.
        ("P16 focus switch clean",  DEFAULTS, FocusSwitch, 0, 0, 0, Ignore),
        ("P17 focus switch dirty",  DEFAULTS, FocusSwitch, 1, 0, 0, Consolidate),
        // P18/P19: shutdown consolidates dirt - hardwired - and skips clean.
        ("P18 shutdown clean",      DEFAULTS, Shutdown, 0, 0, 0, Ignore),
        ("P19 shutdown knobs-max",  knobs(u32::MAX, i64::MAX, usize::MAX), Shutdown, 1, 0, 0, Consolidate),
        // P20-P25: pressure is strictly more-than-capacity.
        ("P20 at capacity",         DEFAULTS, MemoryPressure, 0, 0, 8, Ignore),
        ("P21 over capacity",       DEFAULTS, MemoryPressure, 0, 0, 9, ConsolidateAndRelease),
        ("P23 nothing may stay",    knobs(10, 60, 0), MemoryPressure, 0, 0, 1, ConsolidateAndRelease),
        ("P24 empty cortex",        knobs(10, 60, 0), MemoryPressure, 0, 0, 0, Ignore),
        ("P25 never release",       knobs(10, 60, usize::MAX), MemoryPressure, 0, 0, usize::MAX, Ignore),
    ];
    for (name, policy, stimulus, dirty, idle, loaded, expected) in table {
        assert_eq!(
            policy.evaluate(*stimulus, *dirty, *idle, *loaded),
            *expected,
            "{name}"
        );
    }

    // characterization (AMB-17) - the table's fine print, recorded:
    // P9: threshold zero makes 0 >= 0 true, so even a clean Mutated consolidates.
    assert_eq!(knobs(0, 60, 8).evaluate(Mutated, 0, 0, 0), Consolidate);
    // P14: clock skew - a negative idle never satisfies the quiet period.
    assert_eq!(knobs(10, 0, 8).evaluate(Tick, 1, -5, 0), Ignore);
    // P22: pressure conditions on loaded alone; dirt is irrelevant to it.
    assert_eq!(DEFAULTS.evaluate(MemoryPressure, 0, 0, 9), ConsolidateAndRelease);
}

// QA-28: degenerate knobs never bend the hardwired reflexes - and exit_only
// is exactly "all configurable triggers out of reach".
// Contract: ADR-0002 (Lifecycle and Shutdown are not configurable); DESIGN
// adapters (the CLI direct mode is a policy value, not a code path).
#[test]
fn qa_28_degenerate_knobs_never_bend_the_hardwired_reflexes() {
    use Response::{Consolidate, Ignore};
    use Stimulus::{FocusSwitch, Lifecycle, MemoryPressure, Mutated, OnDemand, Shutdown, Tick};

    let degenerates = [
        ("all zero", knobs(0, 0, 0)),
        ("all one", knobs(1, 1, 1)),
        ("all max", knobs(u32::MAX, i64::MAX, usize::MAX)),
        ("exit only", ConsolidationPolicy::exit_only()),
    ];
    for (name, policy) in degenerates {
        assert_eq!(policy.evaluate(Lifecycle, 0, 0, 0), Consolidate, "{name}: corrections consolidate");
        assert_eq!(policy.evaluate(OnDemand, 0, 0, 0), Consolidate, "{name}: an ask consolidates");
        assert_eq!(policy.evaluate(Shutdown, 1, 0, 0), Consolidate, "{name}: dirty shutdown consolidates");
        assert_eq!(policy.evaluate(Shutdown, 0, 0, 0), Ignore, "{name}: clean shutdown is a no-op");
        assert_eq!(policy.evaluate(FocusSwitch, 1, 0, 0), Consolidate, "{name}: focus turn has no knob");
    }

    // characterization (AMB-18): exit_only is every knob at its maximum -
    // thresholds, quiet, and capacity all out of reach.
    let exit_only = ConsolidationPolicy::exit_only();
    assert_eq!(exit_only, knobs(u32::MAX, i64::MAX, usize::MAX));
    assert_eq!(exit_only.evaluate(Mutated, 1_000_000, 0, 0), Ignore);
    assert_eq!(exit_only.evaluate(Tick, 1_000_000, 1_000_000_000, 0), Ignore);
    assert_eq!(exit_only.evaluate(MemoryPressure, 0, 0, usize::MAX), Ignore);
}
