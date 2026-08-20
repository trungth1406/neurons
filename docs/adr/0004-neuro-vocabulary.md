# ADR-0004: Neuro vocabulary for persistence

Date: 2026-08-20
Status: accepted (renames over ADR-0002/0003 shapes; semantics unchanged)

## Decision

The persistence machinery speaks the product's own metaphor, using the
precise neuroscience terms for what each piece actually does:

| plumbing term    | neuro term            | concept                       |
|------------------|-----------------------|-------------------------------|
| FlushEvent       | Stimulus              | what makes a neuron fire      |
| FlushPolicy      | ConsolidationPolicy   | when working memory commits   |
| Decision         | Response              | stimulus -> response          |
| Flush            | Consolidate           | memory consolidation          |
| FlushAndEvict    | ConsolidateAndRelease | consolidate, then release     |
| outbox           | Outbox (kept)         | the struct keeps its plain name |
| GraphDelta       | Trace                 | the drained outbox            |
| take_delta       | take_trace            |                               |
| Flusher          | EngramStore           | an engram: the stored memory  |
| flush(id, delta) | consolidate(id, &Trace) |                             |
| load(id)         | recall(id)            | retrieval from long-term      |

The architecture sentence: a Stimulus reaches the Cortex; the
ConsolidationPolicy answers with a Response; Consolidate drains the
graph's Trace into the EngramStore; recall brings an engram back.

EngramStore chosen over Hippocampus: more precise, easier to type.

Amendment (2026-08-20, owner): the in-graph journal struct stays named
Outbox — the plainest name for what it is. The drained result remains
Trace; consolidate/recall/EngramStore vocabulary unchanged.
