# neuron — detailed design

Graph-first thinking-graph layer. This document is the implementation
reference: module interfaces, type shapes, behavior contracts, and test
surfaces. Decisions and their rejected alternatives live in `docs/adr/`.

## Principles

1. **Graph-first**: the in-memory `NeuronGraph` is the working truth.
   Every domain rule is a Rust function on flat data. Storage is a
   snapshot sink behind a seam and never carries domain logic.
2. **DOD**: flat typed collections, index references (`NodeIdx`),
   behavior as functions outside the data, no object graphs.
3. **Policy-separated flushing**: *when* to persist is a `FlushPolicy`
   decision evaluated on events — never welded into domain operations.
4. **Single writer**: exactly one process owns memory at a time,
   enforced by an advisory lock.
5. **Small graphs**: one graph per idea cluster, ~30 nodes practice;
   whole-graph load/flush is deliberately trivial at this size.

## Layout

```
src/
  types.rs      plain data, serde, zero behavior
  graph.rs      NeuronGraph: the domain core (no I/O)
  policy.rs     FlushPolicy: pure decisions (no side effects)
  flusher.rs    SQLite snapshot sink + cold reads (all SQL lives here)
  holder.rs     owner runtime: cache + policy execution + lock
  render.rs     mermaid / markdown / JSON interchange
  main.rs       CLI adapter (thin)
  bin/mcp.rs    MCP owner adapter (thin)
tests/          seam tests per module (see Test plan)
docs/           this file + adr/
```

## types.rs

```rust
pub enum GraphStatus { Active, Settled }              // TEXT in storage
#[repr(u8)]
pub enum NodeStatus  { Active = 0, Superseded = 1, Parked = 2 }

pub struct NodeIdx(pub u32);                          // index into nodes vec

pub struct GraphMeta { id, title, status, project: Option<String>,
                       created: i64, updated: i64 }

pub struct NewNode   { id, kind, title, content,
                       stage: Option<String>, skills: Vec<String> }

pub struct Node      { id, kind, title, content, status, stage,
                       skills: Vec<String>, reinforced: u32,
                       superseded_by: Option<String>,
                       created: i64, updated: i64 }

pub struct Edge      { from, to, label, weight: u32, created: i64 }

pub struct GraphData { meta, nodes: Vec<Node>, edges: Vec<Edge> }
// GraphData is the serialization view: flusher rows and JSON
// interchange both round-trip through it.
```

Read views (assembled by graph/flusher, rendered by adapters):

```rust
pub struct NodeBrief    { id, kind, title, reinforced }
pub struct Summary      { meta, counts: {active, superseded, parked},
                          frontier: Vec<NodeBrief>, top: Vec<NodeBrief> }
pub struct Hit          { graph_id, node_id, title, rank: f64 }
pub struct Neighborhood { center: Node,
                          out: Vec<(Edge, NodeBrief)>,
                          inc: Vec<(Edge, NodeBrief)> }
```

## graph.rs — the core

```rust
pub struct NeuronGraph {
    meta:  GraphMeta,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    topo:  StableDiGraph<NodeIdx, ()>,     // petgraph 0.8.3, topology only
    ids:   HashMap<String, NodeIdx>,       // id -> index
    dirty_ops: u32,                        // mutations since last flush
    last_touch: i64,                       // for quiet-period policy
}
```

Invariants (hold after every public call):
- `ids` maps exactly the node ids present in `nodes`.
- `topo` node/edge sets mirror `nodes`/`edges` (edge weight lives in
  the flat vec; topo carries connectivity only).
- Every mutation increments `dirty_ops` and stamps `last_touch` and
  `meta.updated`; `mark_clean()` zeroes `dirty_ops` (called by holder
  after a successful flush).

Interface:

```rust
new(meta) -> NeuronGraph
from_data(GraphData) -> Result<NeuronGraph>   // rebuild topo + ids
to_data(&self) -> GraphData

add_node(&mut self, NewNode, now) -> Result<()>      // dup id = Err
link(&mut self, from, to, label, now) -> Result<()>  // missing = Err;
                                                     // repeat = weight+1
reinforce(&mut self, id, now) -> Result<()>          // reinforced+1
supersede(&mut self, old, by, now) -> Result<()>     // status=Superseded,
                                                     // superseded_by=by;
                                                     // never deletes
set_stage(&mut self, id, stage, now) -> Result<()>
settle(&mut self, now) / reopen(&mut self, now)      // meta.status flip

summary(&self, limit) -> Summary            // frontier = newest active,
                                            // top = most reinforced
neighborhood(&self, id, depth) -> Result<Neighborhood>  // BFS both
                                            // directions via topo
path(&self, from, to) -> Option<Vec<String>>  // shortest, via topo
dirty(&self) -> u32,  touched(&self) -> i64
```

Notes: `now: i64` is always a parameter — the core never reads the
clock (determinism, testability). All errors are "does not exist" /
"already exists" shaped; anyhow at the edges.

## policy.rs — pure decisions

```rust
pub enum FlushEvent {
    OnDemand,             // explicit flush verb / MCP tool
    Lifecycle,            // settle | reopen | supersede just applied
    Mutated,              // any other mutation applied
    Tick,                 // sweeper interval fired
    FocusSwitch,          // holder touched a different graph
    Shutdown,             // owner terminating
    MemoryPressure,       // holder over max_loaded
}

pub struct FlushPolicy {
    dirty_threshold: u32,   // default 10
    quiet_secs: i64,        // default 60
    max_loaded: usize,      // default 8
}

pub enum Decision { Nothing, Flush, FlushAndEvict }

pub fn evaluate(policy, event, dirty: u32, idle_secs: i64,
                loaded: usize) -> Decision
```

Decision table (row = event):

| event          | condition                    | decision       |
|----------------|------------------------------|----------------|
| OnDemand       | always                       | Flush          |
| Lifecycle      | always (hardwired)           | Flush          |
| Mutated        | dirty >= dirty_threshold     | Flush          |
| Tick           | dirty > 0 && idle >= quiet   | Flush          |
| FocusSwitch    | dirty > 0                    | Flush          |
| Shutdown       | dirty > 0 (hardwired)        | Flush          |
| MemoryPressure | loaded > max_loaded          | FlushAndEvict  |

Loss window = min(dirty_threshold ops, quiet_secs, next lifecycle).

## flusher.rs — snapshot sink and cold reads

All SQL in the crate lives here. No domain logic: rows in, rows out.

```rust
pub struct Flusher { conn: Connection }

open(path) -> Result<Flusher>     // WAL, busy_timeout=5000, FK on,
                                  // hand-rolled user_version migration;
                                  // refuses schemas newer than binary
load(&mut self, id) -> Result<GraphData>
flush(&mut self, &GraphData) -> Result<()>
    // ONE IMMEDIATE txn: upsert graphs row; DELETE graph's nodes+edges;
    // reinsert all rows. Whole-graph replace — trivial at ~30 nodes,
    // and FTS triggers keep the index in sync through the delete/insert.
create(&mut self, &GraphMeta) -> Result<()>     // new graph row
list(&mut self, status?, project?) -> Result<Vec<GraphMeta>>
search(&mut self, q, limit) -> Result<Vec<Hit>>  // FTS5 MATCH, rank
exists(&mut self, id) -> Result<bool>
```

Staleness contract: `list`/`search` see *flushed* state only; bounded
by the policy's loss window. Callers surface this as "as of last
flush" semantics; MCP search of the focused hot graph merges in-memory
matches for that graph only (cheap linear scan at ~30 nodes).

Schema: v1 DDL exactly as researched — graphs; nodes with INTEGER
PRIMARY KEY `nid` + UNIQUE(graph_id, id) + `content_encoding` reserved
column; edges WITHOUT ROWID with PK(graph_id, from_id, to_id, label) +
`edges_in` index; FTS5 external-content over nodes(title, content)
with insert/update/delete sync triggers. Four indexes total.

## holder.rs — the owner runtime

```rust
pub struct Holder {
    graphs: HashMap<String, NeuronGraph>,
    flusher: Flusher,
    policy: FlushPolicy,
    focus: Option<String>,
    lock: HolderLock,          // advisory file lock
}

open(db_path, policy) -> Result<Holder>   // acquires holder.lock (flock);
                                          // Err if another owner is alive
with_graph<T>(&mut self, id, now,
              op: impl FnOnce(&mut NeuronGraph) -> Result<T>) -> Result<T>
    // 1. focus != id  -> emit FocusSwitch for previous focus
    // 2. load-if-absent (from flusher); MemoryPressure eviction first
    // 3. run op; classify event (Lifecycle vs Mutated) by verb
    // 4. evaluate policy; execute Flush / FlushAndEvict; mark_clean
read_graph<T>(&mut self, id, ...)          // same, no dirty event
tick(&mut self, now)                       // sweeper: Tick per dirty graph
flush_all(&mut self, now)                  // Shutdown path
create_graph / list / search               // delegate to flusher (+ hot
                                           // merge for focused graph)
```

Lock protocol: `~/.claude/neurons/holder.lock`, `flock(LOCK_EX|NB)` held
for the owner's lifetime (kernel releases on death — no stale locks).
CLI write verbs try the same lock non-blocking: failure means an owner
is alive -> refuse with "owner running; use the MCP tools".

Sweeper: the MCP adapter runs `tick` every 15s (tokio interval). The
CLI direct mode never needs it (flush-on-exit).

## Adapters

neuron-mcp (the owner, long-lived, single writer while alive):

| tool      | params                     | maps to                  |
|-----------|----------------------------|--------------------------|
| summary   | graph                      | holder.read summary      |
| show      | graph, node, depth, budget | holder.read neighborhood |
| search    | query, limit               | holder.search            |
| path      | graph, from, to            | holder.read path         |
| list      | status?, project?          | holder.list              |
| add       | graph, node fields         | holder.with_graph add    |
| link      | graph, from, to, label     | holder.with_graph link   |
| reinforce | graph, id                  | holder.with_graph        |
| supersede | graph, old, by             | holder.with_graph (Lifecycle) |
| stage     | graph, id, stage           | holder.with_graph        |
| settle    | graph  (also reopen)       | holder.with_graph (Lifecycle) |
| flush     | graph?                     | OnDemand -> holder       |

Budget: every read tool truncates by importance (weight desc,
reinforced desc) to the requested budget; defaults keep summary ~150
tokens.

neuron CLI:
- Always available (reads flushed snapshots): `list`, `mermaid`,
  `export`, `import`, `new`.
- Direct mode (only when no owner lock): full write verbs through the
  same Holder with `FlushPolicy::immediate()` (flush-on-exit
  degenerate policy). Same code path, different policy — the mode
  difference IS a policy value.

## Concurrency and crash model

- One owner process at a time (flock). MCP owner running -> CLI writes
  refuse. No owner -> CLI direct mode is the owner for one invocation.
- SQLite WAL: concurrent snapshot readers (CLI reads while owner runs)
  are safe; writes are single-writer by construction above the DB.
- Crash of the owner loses at most the policy loss window (<=10 ops or
  <=60s idle work or up-to-lifecycle). Fsync durability inherits WAL
  defaults; flush transactions are IMMEDIATE.

## Test plan

| module  | seam                | proves                                  |
|---------|---------------------|-----------------------------------------|
| graph   | pure API            | every domain rule with NO database:     |
|         |                     | link weight, supersede survival, BFS    |
|         |                     | depth, path, summary shape, invariants  |
| policy  | pure fn             | full decision table as data             |
| flusher | tempdir DB          | load/flush lossless roundtrip; FTS sync |
|         |                     | through whole-graph replace; refusal of |
|         |                     | newer schema                            |
| holder  | tempdir + fake now  | op streams produce the expected flush   |
|         |                     | calls; eviction order; lock exclusion   |
| render  | golden files        | deterministic mermaid; JSON roundtrip   |
| bins    | none (thin)         | logic lives below the seam              |

## Deferred (reserved landings)

Content compression (`content_encoding` column exists), skills join
table (additive migration), as-of/history, cross-graph links, HTML
render, vault auto-export.
