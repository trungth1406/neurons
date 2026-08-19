use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphStatus {
    Active,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Active = 0,
    Superseded = 1,
    Parked = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeIdx(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphMeta {
    pub id: String,
    pub title: String,
    pub status: GraphStatus,
    pub project: Option<String>,
    pub created: i64,
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub stage: Option<String>,
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub status: NodeStatus,
    pub stage: Option<String>,
    pub skills: Vec<String>,
    pub reinforced: u32,
    pub superseded_by: Option<String>,
    pub created: i64,
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: String,
    pub weight: u32,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphData {
    pub meta: GraphMeta,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

pub type EdgeKey = (String, String, String);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Trace {
    pub meta: Option<GraphMeta>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub deleted_nodes: Vec<String>,
    pub deleted_edges: Vec<EdgeKey>,
}

impl Trace {
    pub fn is_empty(&self) -> bool {
        self.meta.is_none()
            && self.nodes.is_empty()
            && self.edges.is_empty()
            && self.deleted_nodes.is_empty()
            && self.deleted_edges.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeBrief {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub reinforced: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StatusCounts {
    pub active: usize,
    pub superseded: usize,
    pub parked: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub meta: GraphMeta,
    pub counts: StatusCounts,
    pub frontier: Vec<NodeBrief>,
    pub top: Vec<NodeBrief>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighborhood {
    pub center: Node,
    pub out: Vec<(Edge, NodeBrief)>,
    pub inc: Vec<(Edge, NodeBrief)>,
}
