//! neuron-mcp: the long-lived owner. Holds the cortex, serves reads and
//! writes over MCP stdio, sweeps quiet thoughts, consolidates on shutdown.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use neuron::{ConsolidationPolicy, Cortex, GraphMeta, GraphStatus, NewNode, Op};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "neuron-mcp", version, about = "Thinking-graph MCP server (the owner)")]
struct Args {
    /// Database path
    #[arg(long, env = "NEURON_DB")]
    db: Option<PathBuf>,
    /// Sweeper interval in seconds
    #[arg(long, env = "NEURON_TICK_SECS", default_value_t = 15)]
    tick_secs: u64,
    /// Consolidate after this many unconsolidated ops
    #[arg(long, env = "NEURON_DIRTY_THRESHOLD", default_value_t = 10)]
    dirty_threshold: u32,
    /// Consolidate a dirty graph idle this many seconds
    #[arg(long, env = "NEURON_QUIET_SECS", default_value_t = 60)]
    quiet_secs: i64,
    /// Maximum graphs held hot
    #[arg(long, env = "NEURON_MAX_LOADED", default_value_t = 8)]
    max_loaded: usize,
}

fn default_db() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".claude/neurons/neurons.db")
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

type ToolResult<T> = Result<Json<T>, String>;

fn fail<T>(err: anyhow::Error) -> ToolResult<T> {
    Err(format!("{err:#}"))
}

fn as_value<T: serde::Serialize>(value: &T) -> ToolResult<Value> {
    serde_json::to_value(value).map(Json).map_err(|e| e.to_string())
}

#[derive(Clone)]
struct NeuronMcp {
    cortex: Arc<Mutex<Cortex>>,
}

#[derive(Deserialize, JsonSchema)]
struct GraphArg {
    /// Graph id
    graph: String,
}

#[derive(Deserialize, JsonSchema)]
struct SummaryArgs {
    graph: String,
    /// Max entries per list (frontier, top)
    #[serde(default = "five")]
    limit: usize,
}
fn five() -> usize {
    5
}

#[derive(Deserialize, JsonSchema)]
struct ShowArgs {
    graph: String,
    /// Node id at the center
    node: String,
    /// Hops to walk in each direction
    #[serde(default = "one")]
    depth: usize,
    /// Max edges returned per direction
    #[serde(default = "twenty")]
    budget: usize,
}
fn one() -> usize {
    1
}
fn twenty() -> usize {
    20
}

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    /// FTS5 query over titles and content, plus the focused hot graph
    query: String,
    #[serde(default = "ten")]
    limit: usize,
}
fn ten() -> usize {
    10
}

#[derive(Deserialize, JsonSchema)]
struct PathArgs {
    graph: String,
    from: String,
    to: String,
}

#[derive(Deserialize, JsonSchema)]
struct ListArgs {
    /// Filter: "active" or "settled"
    status: Option<String>,
    /// Filter by project tag
    project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct NewGraphArgs {
    graph: String,
    title: String,
    project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AddNodeArgs {
    graph: String,
    /// Node id, unique within the graph (byte-exact identity)
    id: String,
    /// Free-form kind: idea, question, decision, knowledge, correction...
    kind: String,
    title: String,
    #[serde(default)]
    content: String,
    stage: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct LinkArgs {
    graph: String,
    from: String,
    to: String,
    /// Free-form label; repeating the same triple reinforces its weight
    label: String,
}

#[derive(Deserialize, JsonSchema)]
struct NodeSpec {
    /// Node id, unique within the graph (byte-exact identity)
    id: String,
    /// Free-form kind: idea, question, decision, knowledge, correction...
    kind: String,
    title: String,
    #[serde(default)]
    content: String,
    stage: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct LinkSpec {
    from: String,
    to: String,
    /// Free-form label; repeating the same triple reinforces its weight
    label: String,
}

#[derive(Deserialize, JsonSchema)]
struct AddNodesArgs {
    graph: String,
    /// Applied in order, before any links
    nodes: Vec<NodeSpec>,
    #[serde(default)]
    links: Vec<LinkSpec>,
}

#[derive(Deserialize, JsonSchema)]
struct NodeArg {
    graph: String,
    id: String,
}

#[derive(Deserialize, JsonSchema)]
struct SupersedeArgs {
    graph: String,
    /// The corrected belief
    old: String,
    /// What replaces it
    by: String,
}

#[derive(Deserialize, JsonSchema)]
struct StageArgs {
    graph: String,
    id: String,
    stage: String,
}

#[derive(Deserialize, JsonSchema)]
struct ConsolidateArgs {
    /// One graph, or omit for every dirty graph
    graph: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MermaidArgs {
    graph: String,
    /// Node id to center the view on; omit for the whole graph
    focus: Option<String>,
    /// Hops from the focus in each direction
    #[serde(default = "two")]
    depth: usize,
}
fn two() -> usize {
    2
}

#[derive(Deserialize, JsonSchema)]
struct ExportArgs {
    graph: String,
    /// "md" for a readable note, "json" for canonical GraphData
    format: String,
}

#[tool_router(server_handler)]
impl NeuronMcp {
    async fn apply(&self, graph: &str, op: Op) -> Result<String, String> {
        match self.cortex.lock().await.apply(graph, op, unix_now()) {
            Ok(()) => Ok("ok".into()),
            Err(e) => Err(format!("{e:#}")),
        }
    }

    #[tool(description = "Graph overview: status counts, freshest active thoughts (frontier), most reinforced (top). The re-orientation call.")]
    async fn summary(&self, Parameters(a): Parameters<SummaryArgs>) -> ToolResult<Value> {
        match self.cortex.lock().await.summary(&a.graph, a.limit, unix_now()) {
            Ok(s) => as_value(&s),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Everything around one thought: outgoing and incoming edges to the given depth, most-reinforced first, budget-capped per direction.")]
    async fn show(&self, Parameters(a): Parameters<ShowArgs>) -> ToolResult<Value> {
        let result = self
            .cortex
            .lock()
            .await
            .read(&a.graph, unix_now(), |g| g.neighborhood(&a.node, a.depth));
        match result {
            Ok(Ok(mut hood)) => {
                let rank = |pair: &(neuron::Edge, neuron::NodeBrief)| {
                    (std::cmp::Reverse(pair.0.weight), std::cmp::Reverse(pair.1.reinforced))
                };
                hood.out.sort_by_key(rank);
                hood.inc.sort_by_key(rank);
                hood.out.truncate(a.budget);
                hood.inc.truncate(a.budget);
                as_value(&hood)
            }
            Ok(Err(e)) | Err(e) => fail(e),
        }
    }

    #[tool(description = "Full-text search across all graphs (consolidated) plus the focused hot graph.")]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> ToolResult<Value> {
        match self.cortex.lock().await.search(&a.query, a.limit) {
            Ok(hits) => as_value(&json!({ "hits": hits })),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Shortest directed line of reasoning between two thoughts, as node ids.")]
    async fn path(&self, Parameters(a): Parameters<PathArgs>) -> ToolResult<Value> {
        let result = self
            .cortex
            .lock()
            .await
            .read(&a.graph, unix_now(), |g| g.path(&a.from, &a.to));
        match result {
            Ok(Ok(p)) => as_value(&json!({ "path": p })),
            Ok(Err(e)) | Err(e) => fail(e),
        }
    }

    #[tool(description = "List graphs, optionally filtered by status (active|settled) and project.")]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> ToolResult<Value> {
        let status = match a.status.as_deref() {
            None => None,
            Some("active") => Some(GraphStatus::Active),
            Some("settled") => Some(GraphStatus::Settled),
            Some(other) => return Err(format!("unknown status {other:?} (active|settled)")),
        };
        match self.cortex.lock().await.list(status, a.project.as_deref()) {
            Ok(metas) => as_value(&json!({ "graphs": metas })),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Create a new thinking graph. One graph per idea cluster; keep them neuron-sized (~30 nodes), bridge to siblings.")]
    async fn new_graph(&self, Parameters(a): Parameters<NewGraphArgs>) -> Result<String, String> {
        let now = unix_now();
        let meta = GraphMeta {
            id: a.graph,
            title: a.title,
            status: GraphStatus::Active,
            project: a.project,
            created: now,
            updated: now,
        };
        match self.cortex.lock().await.create_graph(&meta) {
            Ok(()) => Ok("ok".into()),
            Err(e) => Err(format!("{e:#}")),
        }
    }

    #[tool(description = "Capture a thought into a graph. Id is byte-exact and unique within the graph.")]
    async fn add_node(&self, Parameters(a): Parameters<AddNodeArgs>) -> Result<String, String> {
        let node = NewNode {
            id: a.id,
            kind: a.kind,
            title: a.title,
            content: a.content,
            stage: a.stage,
            skills: a.skills,
        };
        self.apply(&a.graph, Op::AddNode(node)).await
    }

    #[tool(description = "Connect two thoughts with a labeled edge. Repeating the same (from,to,label) reinforces its weight instead of duplicating.")]
    async fn link(&self, Parameters(a): Parameters<LinkArgs>) -> Result<String, String> {
        self.apply(&a.graph, Op::Link { from: a.from, to: a.to, label: a.label }).await
    }

    #[tool(description = "Batch capture: apply nodes then links in order through the Op door. Stops at the first refusal, reporting what was applied and what failed.")]
    async fn add_nodes(&self, Parameters(a): Parameters<AddNodesArgs>) -> ToolResult<Value> {
        let now = unix_now();
        let mut cortex = self.cortex.lock().await;
        let mut applied_nodes = 0;
        let mut applied_links = 0;
        let mut failed = None;
        for n in a.nodes {
            let id = n.id.clone();
            let node = NewNode {
                id: n.id,
                kind: n.kind,
                title: n.title,
                content: n.content,
                stage: n.stage,
                skills: n.skills,
            };
            match cortex.apply(&a.graph, Op::AddNode(node), now) {
                Ok(()) => applied_nodes += 1,
                Err(e) => {
                    failed = Some(json!({"kind": "node", "id": id, "error": format!("{e:#}")}));
                    break;
                }
            }
        }
        if failed.is_none() {
            for l in a.links {
                let op = Op::Link { from: l.from.clone(), to: l.to.clone(), label: l.label.clone() };
                match cortex.apply(&a.graph, op, now) {
                    Ok(()) => applied_links += 1,
                    Err(e) => {
                        failed = Some(json!({
                            "kind": "link",
                            "edge": {"from": l.from, "to": l.to, "label": l.label},
                            "error": format!("{e:#}"),
                        }));
                        break;
                    }
                }
            }
        }
        as_value(&json!({
            "applied_nodes": applied_nodes,
            "applied_links": applied_links,
            "failed": failed,
        }))
    }

    #[tool(description = "Reinforce a thought: the discussion confirmed it again.")]
    async fn reinforce(&self, Parameters(a): Parameters<NodeArg>) -> Result<String, String> {
        self.apply(&a.graph, Op::Reinforce { id: a.id }).await
    }

    #[tool(description = "Correct a belief: marks it superseded pointing at its replacement. Never deletes; consolidates immediately.")]
    async fn supersede(&self, Parameters(a): Parameters<SupersedeArgs>) -> Result<String, String> {
        self.apply(&a.graph, Op::Supersede { old: a.old, by: a.by }).await
    }

    #[tool(description = "Record where a thought stands in the working flow (free-form stage).")]
    async fn set_stage(&self, Parameters(a): Parameters<StageArgs>) -> Result<String, String> {
        self.apply(&a.graph, Op::SetStage { id: a.id, stage: a.stage }).await
    }

    #[tool(description = "Set a thought aside: not now, not wrong. It leaves the overview but keeps its connections.")]
    async fn park(&self, Parameters(a): Parameters<NodeArg>) -> Result<String, String> {
        self.apply(&a.graph, Op::Park { id: a.id }).await
    }

    #[tool(description = "Wake a parked thought back into the active graph.")]
    async fn unpark(&self, Parameters(a): Parameters<NodeArg>) -> Result<String, String> {
        self.apply(&a.graph, Op::Unpark { id: a.id }).await
    }

    #[tool(description = "The thinking settles: mark the whole graph settled. Consolidates immediately.")]
    async fn settle(&self, Parameters(a): Parameters<GraphArg>) -> Result<String, String> {
        self.apply(&a.graph, Op::Settle).await
    }

    #[tool(description = "Wake a settled graph back to active.")]
    async fn reopen(&self, Parameters(a): Parameters<GraphArg>) -> Result<String, String> {
        self.apply(&a.graph, Op::Reopen).await
    }

    #[tool(description = "Mermaid flowchart of a graph: the whole graph, or the radius around one focused thought. Superseded and parked thoughts are styled distinctly; reinforced edges show their weight.")]
    async fn mermaid(&self, Parameters(a): Parameters<MermaidArgs>) -> ToolResult<Value> {
        let data = match self.cortex.lock().await.read(&a.graph, unix_now(), |g| g.to_data()) {
            Ok(d) => d,
            Err(e) => return fail(e),
        };
        match neuron::render::mermaid(&data, a.focus.as_deref(), a.depth) {
            Ok(chart) => as_value(&json!({ "mermaid": chart })),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Export a graph: format \"md\" renders a readable markdown note, \"json\" returns the canonical GraphData interchange object.")]
    async fn export(&self, Parameters(a): Parameters<ExportArgs>) -> ToolResult<Value> {
        let data = match self.cortex.lock().await.read(&a.graph, unix_now(), |g| g.to_data()) {
            Ok(d) => d,
            Err(e) => return fail(e),
        };
        match a.format.as_str() {
            "md" => as_value(&json!({ "export": neuron::render::export_md(&data) })),
            "json" => as_value(&json!({ "export": data })),
            other => Err(format!("unknown format {other:?} (md|json)")),
        }
    }

    #[tool(description = "Consolidate to long-term storage now: one graph, or everything dirty.")]
    async fn consolidate(&self, Parameters(a): Parameters<ConsolidateArgs>) -> Result<String, String> {
        let result = self
            .cortex
            .lock()
            .await
            .consolidate(a.graph.as_deref(), unix_now());
        match result {
            Ok(()) => Ok("ok".into()),
            Err(e) => Err(format!("{e:#}")),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let db = args.db.unwrap_or_else(default_db);
    let policy = ConsolidationPolicy {
        dirty_threshold: args.dirty_threshold,
        quiet_secs: args.quiet_secs,
        max_loaded: args.max_loaded,
    };
    let cortex = Arc::new(Mutex::new(Cortex::open(&db, policy)?));

    let sweeper = {
        let cortex = Arc::clone(&cortex);
        tokio::spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(
                args.tick_secs.max(1),
            ));
            every.tick().await;
            loop {
                every.tick().await;
                let _ = cortex.lock().await.tick(unix_now());
            }
        })
    };

    let server = NeuronMcp { cortex: Arc::clone(&cortex) };
    let service = server.serve(rmcp::transport::stdio()).await?;

    tokio::select! {
        _ = service.waiting() => {}
        _ = shutdown_signal() => {}
    }

    sweeper.abort();
    cortex.lock().await.consolidate_all(unix_now())?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
