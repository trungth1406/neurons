use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{bail, Context, Result};
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::Direction;

use crate::types::{
    Edge, EdgeKey, GraphData, GraphMeta, GraphStatus, Neighborhood, NewNode, Node, NodeBrief,
    NodeIdx, NodeStatus, StatusCounts, Summary, Trace,
};

/// Journal of what changed since the last consolidation.
/// Keyed maps give last-wins dedup; values index the flat vecs.
#[derive(Debug, Default)]
struct TraceJournal {
    meta: bool,
    nodes: HashMap<String, NodeIdx>,
    edges: HashMap<EdgeKey, usize>,
    ops: u32,
}

/// The in-memory working truth: flat data + topology + memory trace.
/// Zero I/O, zero SQL; `now` is always injected.
#[derive(Debug)]
pub struct NeuronGraph {
    meta: GraphMeta,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    topo: StableDiGraph<NodeIdx, ()>,
    ids: HashMap<String, NodeIdx>,
    trace: TraceJournal,
    last_touch: i64,
}

// The domain never deletes, so petgraph indices stay sequential and
// NodeIdx maps to NodeIndex by value — no mirror map needed.
fn pg_of(idx: NodeIdx) -> NodeIndex {
    NodeIndex::new(idx.0 as usize)
}

impl NeuronGraph {
    pub fn new(meta: GraphMeta) -> NeuronGraph {
        NeuronGraph {
            last_touch: meta.updated,
            meta,
            nodes: Vec::new(),
            edges: Vec::new(),
            topo: StableDiGraph::new(),
            ids: HashMap::new(),
            trace: TraceJournal::default(),
        }
    }

    pub fn from_data(data: GraphData) -> Result<NeuronGraph> {
        let mut graph = NeuronGraph::new(data.meta);
        for node in data.nodes {
            graph.insert_node(node)?;
        }
        for edge in data.edges {
            graph.insert_edge(edge)?;
        }
        graph.trace = TraceJournal::default();
        Ok(graph)
    }

    /// Canonical view: nodes in insertion order, edges sorted by key —
    /// the same orders recall() produces, so roundtrips compare equal.
    pub fn to_data(&self) -> GraphData {
        let mut edges = self.edges.clone();
        edges.sort_by(|a, b| {
            (&a.from, &a.to, &a.label).cmp(&(&b.from, &b.to, &b.label))
        });
        GraphData {
            meta: self.meta.clone(),
            nodes: self.nodes.clone(),
            edges,
        }
    }

    pub fn meta(&self) -> &GraphMeta {
        &self.meta
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn add_node(&mut self, new: NewNode, now: i64) -> Result<()> {
        if self.ids.contains_key(&new.id) {
            bail!("node {:?} already exists", new.id);
        }
        let node = Node {
            id: new.id,
            kind: new.kind,
            title: new.title,
            content: new.content,
            status: NodeStatus::Active,
            stage: new.stage,
            skills: new.skills,
            reinforced: 1,
            superseded_by: None,
            created: now,
            updated: now,
        };
        let idx = self.insert_node(node)?;
        self.journal_node(idx, now);
        Ok(())
    }

    pub fn link(&mut self, from: &str, to: &str, label: &str, now: i64) -> Result<()> {
        self.require(from)?;
        self.require(to)?;
        let key = (from.to_string(), to.to_string(), label.to_string());
        let slot = match self.find_edge(&key) {
            Some(i) => {
                self.edges[i].weight += 1;
                i
            }
            None => {
                let edge = Edge {
                    from: key.0.clone(),
                    to: key.1.clone(),
                    label: key.2.clone(),
                    weight: 1,
                    created: now,
                };
                self.insert_edge(edge)?
            }
        };
        self.trace.edges.insert(key, slot);
        self.touch(now);
        Ok(())
    }

    pub fn reinforce(&mut self, id: &str, now: i64) -> Result<()> {
        let idx = self.require(id)?;
        let node = &mut self.nodes[idx.0 as usize];
        node.reinforced += 1;
        node.updated = now;
        self.journal_node(idx, now);
        Ok(())
    }

    pub fn supersede(&mut self, old: &str, by: &str, now: i64) -> Result<()> {
        if old == by {
            bail!("a node cannot supersede itself");
        }
        self.require(by)?;
        let old_idx = self.require(old)?;
        let node = &mut self.nodes[old_idx.0 as usize];
        node.status = NodeStatus::Superseded;
        node.superseded_by = Some(by.to_string());
        node.updated = now;
        self.journal_node(old_idx, now);
        Ok(())
    }

    pub fn set_stage(&mut self, id: &str, stage: &str, now: i64) -> Result<()> {
        let idx = self.require(id)?;
        let node = &mut self.nodes[idx.0 as usize];
        node.stage = Some(stage.to_string());
        node.updated = now;
        self.journal_node(idx, now);
        Ok(())
    }

    pub fn settle(&mut self, now: i64) {
        self.meta.status = GraphStatus::Settled;
        self.touch(now);
    }

    pub fn reopen(&mut self, now: i64) {
        self.meta.status = GraphStatus::Active;
        self.touch(now);
    }

    pub fn summary(&self, limit: usize) -> Summary {
        let counts = count_statuses(&self.nodes);
        let mut active: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Active)
            .collect();
        active.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
        let frontier = active.iter().take(limit).map(|n| brief(n)).collect();
        active.sort_by(|a, b| b.reinforced.cmp(&a.reinforced).then(a.id.cmp(&b.id)));
        let top = active.iter().take(limit).map(|n| brief(n)).collect();
        Summary {
            meta: self.meta.clone(),
            counts,
            frontier,
            top,
        }
    }

    /// Edges reachable within `depth` hops of `id`, split by direction.
    pub fn neighborhood(&self, id: &str, depth: usize) -> Result<Neighborhood> {
        let center_idx = self.require(id)?;
        let center = self.nodes[center_idx.0 as usize].clone();
        let out = self.walk(center_idx, depth, Direction::Outgoing);
        let inc = self.walk(center_idx, depth, Direction::Incoming);
        Ok(Neighborhood { center, out, inc })
    }

    /// Shortest path by hop count, following edge direction.
    pub fn path(&self, from: &str, to: &str) -> Result<Option<Vec<String>>> {
        let start_pg = pg_of(self.require(from)?);
        let goal_pg = pg_of(self.require(to)?);
        let mut prev: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut queue = VecDeque::from([start_pg]);
        let mut seen = HashSet::from([start_pg]);
        while let Some(at) = queue.pop_front() {
            if at == goal_pg {
                return Ok(Some(self.reconstruct(&prev, at)));
            }
            for next in self.topo.neighbors_directed(at, Direction::Outgoing) {
                if seen.insert(next) {
                    prev.insert(next, at);
                    queue.push_back(next);
                }
            }
        }
        Ok(None)
    }

    fn reconstruct(&self, prev: &HashMap<NodeIndex, NodeIndex>, goal: NodeIndex) -> Vec<String> {
        let mut path = vec![self.node_id_at(goal).to_string()];
        let mut cur = goal;
        while let Some(&p) = prev.get(&cur) {
            path.push(self.node_id_at(p).to_string());
            cur = p;
        }
        path.reverse();
        path
    }

    pub fn take_trace(&mut self) -> Trace {
        let journal = std::mem::take(&mut self.trace);
        let mut node_idxs: Vec<NodeIdx> = journal.nodes.into_values().collect();
        node_idxs.sort_by_key(|idx| idx.0);
        let mut edge_slots: Vec<usize> = journal.edges.into_values().collect();
        edge_slots.sort_unstable();
        Trace {
            meta: journal.meta.then(|| self.meta.clone()),
            nodes: node_idxs
                .into_iter()
                .map(|idx| self.nodes[idx.0 as usize].clone())
                .collect(),
            edges: edge_slots
                .into_iter()
                .map(|slot| self.edges[slot].clone())
                .collect(),
            deleted_nodes: Vec::new(),
            deleted_edges: Vec::new(),
        }
    }

    pub fn dirty(&self) -> u32 {
        self.trace.ops
    }

    pub fn touched(&self) -> i64 {
        self.last_touch
    }

    fn insert_node(&mut self, node: Node) -> Result<NodeIdx> {
        if self.ids.contains_key(&node.id) {
            bail!("node {:?} already exists", node.id);
        }
        let idx = NodeIdx(self.nodes.len() as u32);
        let pg_idx = self.topo.add_node(idx);
        debug_assert_eq!(pg_idx, pg_of(idx));
        self.ids.insert(node.id.clone(), idx);
        self.nodes.push(node);
        Ok(idx)
    }

    fn insert_edge(&mut self, edge: Edge) -> Result<usize> {
        let from = self.require(&edge.from)?;
        let to = self.require(&edge.to)?;
        self.topo.add_edge(pg_of(from), pg_of(to), ());
        self.edges.push(edge);
        Ok(self.edges.len() - 1)
    }

    fn find_edge(&self, key: &EdgeKey) -> Option<usize> {
        self.edges
            .iter()
            .position(|e| e.from == key.0 && e.to == key.1 && e.label == key.2)
    }

    fn require(&self, id: &str) -> Result<NodeIdx> {
        self.ids
            .get(id)
            .copied()
            .with_context(|| format!("node {id:?} does not exist"))
    }

    fn node_id_at(&self, pg_idx: NodeIndex) -> &str {
        let idx = self.topo[pg_idx];
        &self.nodes[idx.0 as usize].id
    }

    fn walk(&self, start: NodeIdx, depth: usize, dir: Direction) -> Vec<(Edge, NodeBrief)> {
        let mut found = Vec::new();
        let mut collected: HashSet<(NodeIndex, NodeIndex)> = HashSet::new();
        let mut frontier = vec![pg_of(start)];
        let mut seen = HashSet::from([pg_of(start)]);
        for _ in 0..depth {
            let mut next_frontier = Vec::new();
            for &at in &frontier {
                for next in self.topo.neighbors_directed(at, dir) {
                    if collected.insert((at, next)) {
                        self.collect_edges(at, next, dir, &mut found);
                    }
                    if seen.insert(next) {
                        next_frontier.push(next);
                    }
                }
            }
            frontier = next_frontier;
            if frontier.is_empty() {
                break;
            }
        }
        found
    }

    fn collect_edges(
        &self,
        at: NodeIndex,
        next: NodeIndex,
        dir: Direction,
        found: &mut Vec<(Edge, NodeBrief)>,
    ) {
        let (from, to) = match dir {
            Direction::Outgoing => (self.node_id_at(at), self.node_id_at(next)),
            Direction::Incoming => (self.node_id_at(next), self.node_id_at(at)),
        };
        let far = self.node_at(match dir {
            Direction::Outgoing => to,
            Direction::Incoming => from,
        });
        for edge in self.edges.iter().filter(|e| e.from == from && e.to == to) {
            found.push((edge.clone(), brief(far)));
        }
    }

    fn node_at(&self, id: &str) -> &Node {
        &self.nodes[self.ids[id].0 as usize]
    }

    fn journal_node(&mut self, idx: NodeIdx, now: i64) {
        let id = self.nodes[idx.0 as usize].id.clone();
        self.trace.nodes.insert(id, idx);
        self.touch(now);
    }

    fn touch(&mut self, now: i64) {
        self.trace.ops += 1;
        self.last_touch = now;
        self.meta.updated = now;
        self.trace.meta = true;
    }
}

fn count_statuses(nodes: &[Node]) -> StatusCounts {
    let mut counts = StatusCounts::default();
    for node in nodes {
        match node.status {
            NodeStatus::Active => counts.active += 1,
            NodeStatus::Superseded => counts.superseded += 1,
            NodeStatus::Parked => counts.parked += 1,
        }
    }
    counts
}

fn brief(node: &Node) -> NodeBrief {
    NodeBrief {
        id: node.id.clone(),
        kind: node.kind.clone(),
        title: node.title.clone(),
        reinforced: node.reinforced,
    }
}

