# ADR-0002: Flush policy events

Date: 2026-08-20
Status: accepted

## Context

With the graph living in memory (ADR-0001), persistence timing becomes
a policy question: which events ask the Flusher to write (and possibly
evict to free memory), and what loss window is acceptable for thought
captured but not yet flushed.

## Decision

Seven trigger events, evaluated by a pure `FlushPolicy`:

1. OnDemand — explicit flush verb / MCP tool. Always flushes.
2. Lifecycle — settle, reopen, supersede. Hardwired immediate flush:
   corrections and lifecycle boundaries are too precious to lose.
3. DirtyThreshold — N mutations since last flush (default 10).
4. QuietPeriod — dirty and idle for T seconds (default 60), detected
   by a sweeper tick.
5. FocusSwitch — the owner starts touching a different graph; the one
   leaving focus flushes if dirty.
6. Shutdown — owner terminating flushes all dirty graphs. Hardwired.
7. MemoryPressure — more than K graphs loaded (default 8): flush and
   evict least-recently-touched.

Defaults are knobs on `FlushPolicy`; Lifecycle and Shutdown are not
configurable. Loss window on owner crash: at most min(N ops, T seconds,
time to next lifecycle event).

## Consequences

- The CLI direct mode is the same machinery with a degenerate policy
  (flush on exit) — mode difference is a policy value, not a code path.
- Cold reads are stale by at most the loss window; the focused hot
  graph's search merges in-memory matches to compensate.
- The policy is a pure function of (event, dirty, idle, loaded) and is
  tested as a decision table.
