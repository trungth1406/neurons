# ADR-0006: Domain verbs become a closed Op enum, not traits

Date: 2026-08-20
Status: accepted (implementation lands with T3, where the first
consumer exists)

## Context

The verb set will grow, and more per-verb handling is coming (stimulus
classification, MCP dispatch, future replay). The question is the
dispatch shape: a Verb trait with implementors, or an enum.

## Decision

A closed `Op` enum owned by the graph module, one variant per domain
verb, applied through a single `NeuronGraph::apply(op, now)` door that
routes to the existing named methods. Per-verb knowledge hangs off the
enum (`op.stimulus()` classifies Lifecycle vs Mutated for the
consolidation policy). Serde on the enum is the wire format for MCP
tools and any future replay.

Traits rejected: the verb set is closed and crate-owned — a trait
buys open-world extensibility nobody needs, and loses exhaustive
matching (the compiler forcing every match site to handle a new verb
is the feature), free serialization, and static dispatch.

Topology growth is likewise not a trait: new topological questions are
functions (mostly petgraph algorithms) over the one StableDiGraph
plane.

## Consequences

- New verb = new variant; every match breaks until handled. That
  breakage is the design working.
- T5 MCP write tools become decode -> apply.
- Named methods remain the ergonomic API; apply is the uniform door.
