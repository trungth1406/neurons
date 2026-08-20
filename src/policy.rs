//! When the brain commits working memory to long-term storage.
//! Pure decisions: no clocks, no I/O — everything is a parameter.

/// What just happened, as the policy hears it (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stimulus {
    /// Explicit consolidate verb or MCP tool.
    OnDemand,
    /// settle / reopen / supersede landed.
    Lifecycle,
    /// Any other mutation landed.
    Mutated,
    /// The sweeper interval fired.
    Tick,
    /// The cortex turned to a different graph.
    FocusSwitch,
    /// The owner process is terminating.
    Shutdown,
    /// More graphs loaded than the cortex wants hot.
    MemoryPressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Ignore,
    Consolidate,
    ConsolidateAndRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsolidationPolicy {
    pub dirty_threshold: u32,
    pub quiet_secs: i64,
    pub max_loaded: usize,
}

impl Default for ConsolidationPolicy {
    fn default() -> Self {
        ConsolidationPolicy {
            dirty_threshold: 10,
            quiet_secs: 60,
            max_loaded: 8,
        }
    }
}

impl ConsolidationPolicy {
    /// CLI direct mode: threshold and quiet-period rows never fire; the
    /// hardwired rows (OnDemand, Lifecycle, FocusSwitch, Shutdown) still
    /// consolidate, so a single invocation writes at natural boundaries
    /// and always on exit.
    pub fn exit_only() -> Self {
        ConsolidationPolicy {
            dirty_threshold: u32::MAX,
            quiet_secs: i64::MAX,
            max_loaded: usize::MAX,
        }
    }

    /// The ADR-0002 decision table, one row per stimulus.
    pub fn evaluate(
        &self,
        stimulus: Stimulus,
        dirty: u32,
        idle_secs: i64,
        loaded: usize,
    ) -> Response {
        use {Response::*, Stimulus::*};
        match stimulus {
            OnDemand | Lifecycle => Consolidate,
            Shutdown if dirty > 0 => Consolidate,
            Mutated if dirty >= self.dirty_threshold => Consolidate,
            Tick if dirty > 0 && idle_secs >= self.quiet_secs => Consolidate,
            FocusSwitch if dirty > 0 => Consolidate,
            MemoryPressure if loaded > self.max_loaded => ConsolidateAndRelease,
            _ => Ignore,
        }
    }
}
