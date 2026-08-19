# ADR-0003: Outbox delta flush; the owner is named Cortex

Date: 2026-08-20
Status: accepted (refines ADR-0001's flush shape)

## Context

Design review by the owner challenged two points: whole-graph
delete-and-reinsert flushing ("why reinsert everything?"), and the
module name Holder ("too generic").

## Decision

1. **Outbox delta flush.** Every mutation applies to state AND records
   into a per-component-kind outbox (keyed maps: last-wins dedup).
   Flush drains the outbox — `take_delta()` — into a `GraphDelta` of
   per-kind row batches; the Flusher executes bulk upserts per kind in
   one IMMEDIATE transaction, plus bulk-delete slots that stay empty
   while the domain has no deletion. O(changed rows); stable nids; FTS
   triggers fire only for rows actually written. The only whole-graph
   write left is `import` of a not-yet-existing graph id.
2. **Cortex.** The owner runtime (hot-graph cache, policy execution,
   advisory lock) is named Cortex — the product's own metaphor: where
   active neurons live and fire; flushing sends them to long-term
   storage.

## Rationale

The domain never deletes rows (supersede marks, reinforce increments,
add inserts), so delete-and-reinsert bought zero correctness and cost
FTS churn, nid instability, and write amplification. The outbox makes
the Flusher graph-ignorant: it moves a delta's rows, nothing more.

## Consequences

- NeuronGraph carries the outbox and exposes `take_delta`; the policy
  reads `outbox.ops` as its dirty counter.
- Flusher interface: `flush(graph_id, &GraphDelta)`; `import` refuses
  existing ids so no replace path exists anywhere.
- If deletion ever enters the domain, it is an outbox entry and a bulk
  delete — no flush redesign.
