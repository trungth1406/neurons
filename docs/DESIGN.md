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
3. **Policy-separated consolidating**: *when* to persist is a `ConsolidationPolicy`
   decision evaluated on events — never welded into domain operations.
4. **Single writer**: exactly one process owns memory at a time,
   enforced by an advisory lock.
5. **Small graphs**: one graph per idea cluster, ~30 nodes practice;
   whole-graph load/consolidate is deliberately trivial at this size.

## Layout

```
src/
  types.rs      plain data, serde, zero behavior
  graph.rs      NeuronGraph: the domain core (no I/O)
  policy.rs     ConsolidationPolicy: pure decisions (no side effects)
  engram.rs    SQLite snapshot sink + cold reads (all SQL lives here)
  cortex.rs     owner runtime: cache + policy execution + lock
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
// GraphData is the serialization view: consolidateer rows and JSON
// interchange both round-trip through it.
```

Read views (assembled by graph/consolidateer, rendered by adapters):

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
    trace: TraceJournal,                        // parallel mutation journal
    last_touch: i64,                       // for quiet-period policy
}

// The trace journal: every mutation applies to state AND records its row in
// the journal for its component kind. Keyed maps = last-wins dedup.
struct TraceJournal {
    meta:  bool,                              // graphs row dirty
    nodes: HashMap<String, ()>,               // node ids touched
    edges: HashMap<(String, String, String), ()>,  // edge keys touched
    ops:   u32,                               // total mutations (policy)
}

// Drained at consolidate into the EngramStore's input — per-kind row batches:
pub struct Trace {
    pub meta:  Option<GraphMeta>,
    pub nodes: Vec<Node>,                  // current rows for touched ids
    pub edges: Vec<Edge>,
    pub deleted_nodes: Vec<String>,        // empty in MVP: domain never
    pub deleted_edges: Vec<(String, String, String)>,  // deletes
}
```

Invariants (hold after every public call):
- `ids` maps exactly the node ids present in `nodes`.
- `topo` node/edge sets mirror `nodes`/`edges` (edge weight lives in
  the flat vec; topo carries connectivity only).
- Every mutation records into the trace journal and stamps `last_touch` and
  `meta.updated`; a successful consolidate drains the trace journal via `take_trace` (cortex-driven
  after a successful consolidate).

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
take_trace(&mut self) -> Trace   // drain trace journal; rows copied from
                                      // current state (state is truth)
dirty(&self) -> u32,  touched(&self) -> i64
```

Notes: `now: i64` is always a parameter — the core never reads the
clock (determinism, testability). All errors are "does not exist" /
"already exists" shaped; anyhow at the edges.

## policy.rs — pure decisions

```rust
pub enum Stimulus {
    OnDemand,             // explicit consolidate verb / MCP tool
    Lifecycle,            // settle | reopen | supersede just applied
    Mutated,              // any other mutation applied
    Tick,                 // sweeper interval fired
    FocusSwitch,          // cortex touched a different graph
    Shutdown,             // owner terminating
    MemoryPressure,       // cortex over max_loaded
}

pub struct ConsolidationPolicy {
    dirty_threshold: u32,   // default 10
    quiet_secs: i64,        // default 60
    max_loaded: usize,      // default 8
}

pub enum Response { Ignore, Consolidate, ConsolidateAndRelease }

pub fn evaluate(policy, event, dirty: u32, idle_secs: i64,
                loaded: usize) -> Response
```

Decision table (row = event):

| event          | condition                    | decision       |
|----------------|------------------------------|----------------|
| OnDemand       | always                       | Consolidate    |
| Lifecycle      | always (hardwired)           | Consolidate    |
| Mutated        | dirty >= dirty_threshold     | Consolidate    |
| Tick           | dirty > 0 && idle >= quiet   | Consolidate    |
| FocusSwitch    | dirty > 0                    | Consolidate    |
| Shutdown       | dirty > 0 (hardwired)        | Consolidate    |
| MemoryPressure | loaded > max_loaded          | ConsolidateAndRelease  |

Loss window = min(dirty_threshold ops, quiet_secs, next lifecycle).

## engram.rs — snapshot sink and cold reads

All SQL in the crate lives here. No domain logic: rows in, rows out.

```rust
pub struct EngramStore { conn: Connection }

open(path) -> Result<EngramStore>     // WAL, busy_timeout=5000, FK on,
                                  // hand-rolled user_version migration;
                                  // refuses schemas newer than binary
recall(&mut self, id) -> Result<GraphData>
consolidate(&mut self, graph_id, &Trace) -> Result<()>
    // ONE IMMEDIATE txn, per-kind bulk statements over the delta only:
    //   upsert graphs row        if delta.meta
    //   bulk upsert delta.nodes  (prepared stmt loop; FTS triggers fire
    //   bulk upsert delta.edges   only for rows actually written)
    //   bulk delete deleted_*    (empty in MVP; the slot exists)
    // O(changed rows), stable nids, no whole-graph rewrite. The EngramStore
    // knows nothing about graphs — it moves a delta's rows.
import(&mut self, &GraphData) -> Result<()>
    // the ONE whole-graph write: bulk insert of a graph whose id does
    // not exist yet (import refuses existing ids — no replace path)
create(&mut self, &GraphMeta) -> Result<()>     // new graph row
list(&mut self, status?, project?) -> Result<Vec<GraphMeta>>
search(&mut self, q, limit) -> Result<Vec<Hit>>  // FTS5 MATCH, rank
exists(&mut self, id) -> Result<bool>
```

Staleness contract: `list`/`search` see *consolidated* state only; bounded
by the policy's loss window. Callers surface this as "as of last
consolidate" semantics; MCP search of the focused hot graph merges in-memory
matches for that graph only (cheap linear scan at ~30 nodes).

Schema: v1 DDL exactly as researched — graphs; nodes with INTEGER
PRIMARY KEY `nid` + UNIQUE(graph_id, id) + `content_encoding` reserved
column; edges WITHOUT ROWID with PK(graph_id, from_id, to_id, label) +
`edges_in` index; FTS5 external-content over nodes(title, content)
with insert/update/delete sync triggers. Four indexes total.

## cortex.rs — the owner runtime

```rust
pub struct Cortex {
    graphs: HashMap<String, NeuronGraph>,
    consolidateer: EngramStore,
    policy: ConsolidationPolicy,
    focus: Option<String>,
    lock: CortexLock,          // advisory file lock
}

open(db_path, policy) -> Result<Cortex>   // acquires cortex.lock (flock);
                                          // Err if another owner is alive
with_graph<T>(&mut self, id, now,
              op: impl FnOnce(&mut NeuronGraph) -> Result<T>) -> Result<T>
    // 1. focus != id  -> emit FocusSwitch for previous focus
    // 2. load-if-absent (from consolidateer); MemoryPressure eviction first
    // 3. run op; classify event (Lifecycle vs Mutated) by verb
    // 4. evaluate policy; execute Consolidate / ConsolidateAndRelease
read_graph<T>(&mut self, id, ...)          // same, no dirty event
tick(&mut self, now)                       // sweeper: Tick per dirty graph
consolidate_all(&mut self, now)                  // Shutdown path
create_graph / list / search               // delegate to consolidateer (+ hot
                                           // merge for focused graph)
```

Lock protocol: `~/.claude/neurons/cortex.lock`, `flock(LOCK_EX|NB)` held
for the cortex process lifetime (kernel releases on death — no stale locks).
CLI write verbs try the same lock non-blocking: failure means an owner
is alive -> refuse with "owner running; use the MCP tools".

Sweeper: the MCP adapter runs `tick` every 15s (tokio interval). The
CLI direct mode never needs it (consolidate-on-exit).

## Adapters

neuron-mcp (the owner, long-lived, single writer while alive):

| tool      | params                     | maps to                  |
|-----------|----------------------------|--------------------------|
| summary   | graph                      | cortex.read summary      |
| show      | graph, node, depth, budget | cortex.read neighborhood |
| search    | query, limit               | cortex.search            |
| path      | graph, from, to            | cortex.read path         |
| list      | status?, project?          | cortex.list              |
| add       | graph, node fields         | cortex.with_graph add    |
| link      | graph, from, to, label     | cortex.with_graph link   |
| reinforce | graph, id                  | cortex.with_graph        |
| supersede | graph, old, by             | cortex.with_graph (Lifecycle) |
| stage     | graph, id, stage           | cortex.with_graph        |
| settle    | graph  (also reopen)       | cortex.with_graph (Lifecycle) |
| consolidate     | graph?                     | OnDemand -> cortex       |

Budget: every read tool truncates by importance (weight desc,
reinforced desc) to the requested budget; defaults keep summary ~150
tokens.

neuron CLI:
- Always available (reads consolidated snapshots): `list`, `mermaid`,
  `export`, `import`, `new`.
- Direct mode (only when no owner lock): full write verbs through the
  same Cortex with `ConsolidationPolicy::immediate()` (consolidate-on-exit
  degenerate policy). Same code path, different policy — the mode
  difference IS a policy value.

## Concurrency and crash model

- One owner process at a time (flock). MCP owner running -> CLI writes
  refuse. No owner -> CLI direct mode is the owner for one invocation.
- SQLite WAL: concurrent snapshot readers (CLI reads while owner runs)
  are safe; writes are single-writer by construction above the DB.
- Crash of the owner loses at most the policy loss window (<=10 ops or
  <=60s idle work or up-to-lifecycle). Fsync durability inherits WAL
  defaults; consolidate transactions are IMMEDIATE.

## Test plan

| module  | seam                | proves                                  |
|---------|---------------------|-----------------------------------------|
| graph   | pure API            | every domain rule with NO database:     |
|         |                     | link weight, supersede survival, BFS    |
|         |                     | depth, path, summary shape, invariants  |
| policy  | pure fn             | full decision table as data             |
| consolidateer | tempdir DB          | load/consolidate lossless roundtrip; FTS sync |
|         |                     | through whole-graph replace; refusal of |
|         |                     | newer schema                            |
| cortex  | tempdir + fake now  | op streams produce the expected consolidate   |
|         |                     | calls; eviction order; lock exclusion   |
| render  | golden files        | deterministic mermaid; JSON roundtrip   |
| bins    | none (thin)         | logic lives below the seam              |

## Deferred (reserved landings)

Content compression (`content_encoding` column exists), skills join
table (additive migration), as-of/history, cross-graph links, HTML
render, vault auto-export.
