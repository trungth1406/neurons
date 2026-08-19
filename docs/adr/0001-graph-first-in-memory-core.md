# ADR-0001: Graph-first in-memory core

Date: 2026-08-20
Status: accepted

## Context

The first implementation attempt made SQLite the center: every domain
operation was a SQL statement (reinforcement as `ON CONFLICT DO UPDATE
weight = weight + 1`), petgraph a read-side afterthought. The owner
rejected it: domain logic had leaked into SQL strings and Rust was a
pass-through — "just SQL with Rust in the middle."

## Decision

The in-memory `NeuronGraph` (flat vecs + petgraph StableGraph topology
+ id map) is the working truth. All domain rules are Rust functions on
it, I/O-free and clock-free (`now` is a parameter). Persistence is a
separate seam (`Flusher`) that snapshots whole graphs to SQLite, and
*when* to persist is a `FlushPolicy` decision evaluated on events —
never welded into the domain operations.

A long-lived owner process (the MCP server; the CLI in single-invocation
direct mode) holds graphs in memory behind an advisory file lock:
exactly one writer at a time.

## Consequences

- The entire domain is testable without a database.
- Whole-graph flush replaces row-level SQL; trivial at the ~30-node
  practice size, and FTS stays in sync through its triggers.
- Cold reads (list, search) see flushed state; staleness is bounded by
  the flush policy's loss window (ADR-0002).
- The MCP server gains write tools and becomes the owner, revising the
  earlier reads-only split; the CLI keeps admin/read verbs and a
  direct mode that is the same code with a flush-on-exit policy.
- A crash loses at most the policy loss window of unflushed thought.

## Rejected

- SQL-centric store (first attempt): domain in strings, shallow Rust.
- petgraph's own serde as the persistence format: exposes internals,
  marries the file format to the dependency's major version.
- A Flusher trait: one adapter exists; a trait would be a hypothetical
  seam. The module boundary is the seam.
