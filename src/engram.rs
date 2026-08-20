use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::types::{Edge, GraphData, GraphMeta, GraphStatus, Hit, Node, NodeStatus, Trace};

refinery::embed_migrations!("migrations");

/// Long-term storage: consolidates traces in, recalls graphs out.
/// Rows only — no domain logic lives here.
#[derive(Debug)]
pub struct EngramStore {
    conn: Connection,
}

impl EngramStore {
    pub fn open(path: &Path) -> Result<EngramStore> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut conn = Connection::open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::runner()
            .run(&mut conn)
            .context("running schema migrations (a newer database refuses an older binary)")?;
        Ok(EngramStore { conn })
    }

    pub fn create(&mut self, meta: &GraphMeta) -> Result<()> {
        let tx = self.write_tx()?;
        tx.execute(
            "INSERT INTO graphs (id, title, status, project, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![meta.id, meta.title, status_str(meta.status), meta.project,
                    meta.created, meta.updated],
        )
        .with_context(|| format!("graph {:?} already exists?", meta.id))?;
        tx.commit()?;
        Ok(())
    }

    pub fn consolidate(&mut self, graph_id: &str, trace: &Trace) -> Result<()> {
        if trace.is_empty() {
            return Ok(());
        }
        let tx = self.write_tx()?;
        apply_rows(&tx, graph_id, trace.meta.as_ref(), &trace.nodes, &trace.edges)?;
        apply_deletes(&tx, graph_id, trace)?;
        tx.commit()?;
        Ok(())
    }

    pub fn recall(&mut self, id: &str) -> Result<GraphData> {
        let tx = self.conn.transaction()?;
        let meta = read_meta(&tx, id)?;
        let nodes = read_nodes(&tx, id)?;
        let edges = read_edges(&tx, id)?;
        tx.commit()?;
        Ok(GraphData { meta, nodes, edges })
    }

    /// The one whole-graph write: a graph arriving from interchange.
    pub fn import(&mut self, data: &GraphData) -> Result<()> {
        let tx = self.write_tx()?;
        let taken: Option<i64> = tx
            .query_row("SELECT 1 FROM graphs WHERE id = ?1", [&data.meta.id], |r| r.get(0))
            .optional()?;
        if taken.is_some() {
            bail!("graph {:?} already exists; import never replaces", data.meta.id);
        }
        apply_rows(&tx, &data.meta.id, Some(&data.meta), &data.nodes, &data.edges)?;
        tx.commit()?;
        Ok(())
    }

    pub fn exists(&mut self, id: &str) -> Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row("SELECT 1 FROM graphs WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(found.is_some())
    }

    pub fn list(
        &mut self,
        status: Option<GraphStatus>,
        project: Option<&str>,
    ) -> Result<Vec<GraphMeta>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, status, project, created, updated FROM graphs
             WHERE (?1 IS NULL OR status = ?1)
               AND (?2 IS NULL OR project = ?2)
             ORDER BY updated DESC",
        )?;
        let rows = stmt.query_map(params![status.map(status_str), project], row_meta)?;
        collect_mapped(rows, "listing graphs")
    }

    pub fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT n.graph_id, n.id, n.title, rank
             FROM node_fts f JOIN nodes n ON n.nid = f.rowid
             WHERE node_fts MATCH ?1
             ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |r| {
            Ok(Hit {
                graph_id: r.get(0)?,
                node_id: r.get(1)?,
                title: r.get(2)?,
                rank: r.get(3)?,
            })
        })?;
        collect_mapped(rows, "searching nodes")
    }

    fn write_tx(&mut self) -> Result<rusqlite::Transaction> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }
}

// Storage encodings: the representations the schema fossilizes.
fn status_str(status: GraphStatus) -> &'static str {
    match status {
        GraphStatus::Active => "active",
        GraphStatus::Settled => "settled",
    }
}

fn status_parse(s: &str) -> Result<GraphStatus> {
    match s {
        "active" => Ok(GraphStatus::Active),
        "settled" => Ok(GraphStatus::Settled),
        _ => bail!("unknown graph status {s:?}"),
    }
}

fn node_status_parse(v: u8) -> Result<NodeStatus> {
    match v {
        0 => Ok(NodeStatus::Active),
        1 => Ok(NodeStatus::Superseded),
        2 => Ok(NodeStatus::Parked),
        _ => bail!("unknown node status {v}"),
    }
}

fn apply_rows(
    tx: &rusqlite::Transaction,
    graph_id: &str,
    meta: Option<&GraphMeta>,
    nodes: &[Node],
    edges: &[Edge],
) -> Result<()> {
    if let Some(meta) = meta {
        upsert_meta(tx, meta)?;
    }
    for node in nodes {
        upsert_node(tx, graph_id, node)?;
    }
    for edge in edges {
        upsert_edge(tx, graph_id, edge)?;
    }
    Ok(())
}

fn upsert_meta(tx: &rusqlite::Transaction, meta: &GraphMeta) -> Result<()> {
    tx.execute(
        "INSERT INTO graphs (id, title, status, project, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET
           title = excluded.title, status = excluded.status,
           project = excluded.project, updated = excluded.updated",
        params![meta.id, meta.title, status_str(meta.status), meta.project,
                meta.created, meta.updated],
    )?;
    Ok(())
}

fn upsert_node(tx: &rusqlite::Transaction, graph_id: &str, node: &Node) -> Result<()> {
    let skills = serde_json::to_string(&node.skills)?;
    tx.execute(
        "INSERT INTO nodes (graph_id, id, kind, title, content, status, stage,
                            skills, reinforced, superseded_by, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (graph_id, id) DO UPDATE SET
           kind = excluded.kind, title = excluded.title,
           content = excluded.content, status = excluded.status,
           stage = excluded.stage, skills = excluded.skills,
           reinforced = excluded.reinforced,
           superseded_by = excluded.superseded_by, updated = excluded.updated",
        params![graph_id, node.id, node.kind, node.title, node.content,
                node.status as u8, node.stage, skills, node.reinforced,
                node.superseded_by, node.created, node.updated],
    )?;
    Ok(())
}

fn upsert_edge(tx: &rusqlite::Transaction, graph_id: &str, edge: &Edge) -> Result<()> {
    tx.execute(
        "INSERT INTO edges (graph_id, from_id, to_id, label, weight, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (graph_id, from_id, to_id, label)
         DO UPDATE SET weight = excluded.weight",
        params![graph_id, edge.from, edge.to, edge.label, edge.weight, edge.created],
    )?;
    Ok(())
}

fn read_meta(tx: &rusqlite::Transaction, id: &str) -> Result<GraphMeta> {
    tx.query_row(
        "SELECT id, title, status, project, created, updated FROM graphs WHERE id = ?1",
        [id],
        row_meta,
    )
    .optional()?
    .with_context(|| format!("graph {id:?} does not exist"))
}

fn apply_deletes(tx: &rusqlite::Transaction, graph_id: &str, trace: &Trace) -> Result<()> {
    for id in &trace.deleted_nodes {
        tx.execute("DELETE FROM nodes WHERE graph_id = ?1 AND id = ?2",
                   params![graph_id, id])?;
    }
    for (from, to, label) in &trace.deleted_edges {
        tx.execute(
            "DELETE FROM edges
             WHERE graph_id = ?1 AND from_id = ?2 AND to_id = ?3 AND label = ?4",
            params![graph_id, from, to, label],
        )?;
    }
    Ok(())
}

fn row_meta(r: &rusqlite::Row) -> rusqlite::Result<GraphMeta> {
    let status: String = r.get(2)?;
    let status = status_parse(&status).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2, rusqlite::types::Type::Text, e.into())
    })?;
    Ok(GraphMeta {
        id: r.get(0)?,
        title: r.get(1)?,
        status,
        project: r.get(3)?,
        created: r.get(4)?,
        updated: r.get(5)?,
    })
}

fn read_nodes(tx: &rusqlite::Transaction, graph_id: &str) -> Result<Vec<Node>> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, kind, title, content, status, stage, skills, reinforced,
                superseded_by, created, updated
         FROM nodes WHERE graph_id = ?1 ORDER BY nid",
    )?;
    let rows = stmt.query_map([graph_id], |r| {
        let raw_status = r.get::<_, u8>(4)?;
        let raw_skills = r.get::<_, String>(6)?;
        let node = Node {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            content: r.get(3)?,
            status: NodeStatus::Active,
            stage: r.get(5)?,
            skills: Vec::new(),
            reinforced: r.get(7)?,
            superseded_by: r.get(8)?,
            created: r.get(9)?,
            updated: r.get(10)?,
        };
        Ok((node, raw_status, raw_skills))
    })?;
    rows.map(|row| decode_node(row.context("reading node")?)).collect()
}

fn decode_node((mut node, status, skills): (Node, u8, String)) -> Result<Node> {
    node.status = node_status_parse(status).with_context(|| format!("node {:?}", node.id))?;
    node.skills = serde_json::from_str(&skills)
        .with_context(|| format!("node {:?}: invalid skills JSON", node.id))?;
    Ok(node)
}

fn read_edges(tx: &rusqlite::Transaction, graph_id: &str) -> Result<Vec<Edge>> {
    let mut stmt = tx.prepare_cached(
        "SELECT from_id, to_id, label, weight, created
         FROM edges WHERE graph_id = ?1 ORDER BY from_id, to_id, label",
    )?;
    let rows = stmt.query_map([graph_id], |r| {
        Ok(Edge {
            from: r.get(0)?,
            to: r.get(1)?,
            label: r.get(2)?,
            weight: r.get(3)?,
            created: r.get(4)?,
        })
    })?;
    collect_mapped(rows, "reading edges")
}

fn collect_mapped<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
    what: &str,
) -> Result<Vec<T>> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| what.to_string())
}
