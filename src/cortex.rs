use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::engram::EngramStore;
use crate::graph::{NeuronGraph, Op, OpKind};
use crate::policy::{ConsolidationPolicy, Response, Stimulus};
use crate::types::{GraphMeta, GraphStatus, Hit, Summary};

/// Where active neurons live and fire. Holds hot graphs, executes the
/// consolidation policy, and is the single writer while alive (flock).
#[derive(Debug)]
pub struct Cortex {
    graphs: HashMap<String, NeuronGraph>,
    engrams: EngramStore,
    policy: ConsolidationPolicy,
    focus: Option<String>,
    _lock: File,
}

impl Cortex {
    /// Acquires the writer lock beside the database; a second cortex on
    /// the same database is refused immediately. The kernel releases the
    /// lock when the process dies — no stale locks.
    pub fn open(db_path: &Path, policy: ConsolidationPolicy) -> Result<Cortex> {
        let lock_path = db_path.with_file_name("cortex.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let lock = File::create(&lock_path)
            .with_context(|| format!("creating {}", lock_path.display()))?;
        // SAFETY: flock on a valid fd owned by `lock`; the lock's lifetime
        // is tied to the Cortex via the _lock field, and the kernel drops
        // it when the fd closes (process death included).
        let taken = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if taken != 0 {
            bail!(
                "another cortex owns {} (lock {}); use its MCP tools or stop it",
                db_path.display(),
                lock_path.display()
            );
        }
        Ok(Cortex {
            graphs: HashMap::new(),
            engrams: EngramStore::open(db_path)?,
            policy,
            focus: None,
            _lock: lock,
        })
    }

    pub fn create_graph(&mut self, meta: &GraphMeta) -> Result<()> {
        self.engrams.create(meta)?;
        self.graphs
            .insert(meta.id.clone(), NeuronGraph::new(meta.clone()));
        self.relieve_pressure(&meta.id)
    }

    /// How many graphs are hot right now (introspection for tests and stats).
    pub fn loaded(&self) -> usize {
        self.graphs.len()
    }

    /// The uniform write door: focus bookkeeping, the op itself, then
    /// the policy hears about it and consolidation may follow.
    pub fn apply(&mut self, graph_id: &str, op: Op, now: i64) -> Result<()> {
        self.hot(graph_id)?;
        self.turn_focus(graph_id, now)?;
        let kind = op.kind();
        self.hot(graph_id)?.apply(op, now)?;
        let stimulus = match kind {
            OpKind::Lifecycle => Stimulus::Lifecycle,
            OpKind::Mutation => Stimulus::Mutated,
        };
        self.hear(graph_id, stimulus, now)
    }

    /// Read access to a hot graph (loads it if sleeping); no stimulus.
    pub fn read<T>(
        &mut self,
        graph_id: &str,
        now: i64,
        f: impl FnOnce(&NeuronGraph) -> T,
    ) -> Result<T> {
        self.hot(graph_id)?;
        self.turn_focus(graph_id, now)?;
        Ok(f(self.hot(graph_id)?))
    }

    pub fn summary(&mut self, graph_id: &str, limit: usize, now: i64) -> Result<Summary> {
        self.read(graph_id, now, |g| g.summary(limit))
    }

    /// On-demand consolidation: one graph, or every dirty graph.
    pub fn consolidate(&mut self, graph_id: Option<&str>, now: i64) -> Result<()> {
        match graph_id {
            Some(id) => {
                if !self.graphs.contains_key(id) && !self.engrams.exists(id)? {
                    bail!("graph {id:?} does not exist");
                }
                self.hear(id, Stimulus::OnDemand, now)
            }
            None => self.consolidate_all(now),
        }
    }

    /// The sweeper: quiet dirty graphs consolidate.
    pub fn tick(&mut self, now: i64) -> Result<()> {
        self.sweep(Stimulus::Tick, now)
    }

    /// Shutdown path: everything dirty reaches the engram store.
    pub fn consolidate_all(&mut self, now: i64) -> Result<()> {
        self.sweep(Stimulus::Shutdown, now)
    }

    /// Every graph gets its chance even if one fails; first error surfaces.
    fn sweep(&mut self, stimulus: Stimulus, now: i64) -> Result<()> {
        let ids: Vec<String> = self.graphs.keys().cloned().collect();
        let mut first_err = None;
        for id in ids {
            if let Err(e) = self.hear(&id, stimulus, now) {
                first_err.get_or_insert(e);
            }
        }
        match first_err {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }

    pub fn list(
        &mut self,
        status: Option<GraphStatus>,
        project: Option<&str>,
    ) -> Result<Vec<GraphMeta>> {
        self.engrams.list(status, project)
    }

    /// Cold search over consolidated rows, plus the focused hot graph
    /// scanned in memory so fresh thought is findable before it lands.
    pub fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut cold = self.engrams.search(query, limit)?;
        let hot = match &self.focus {
            Some(id) => match self.graphs.get(id) {
                Some(graph) => hot_matches(id, graph, query),
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        let hot: Vec<Hit> = hot
            .into_iter()
            .filter(|h| !cold.iter().any(|c| c.graph_id == h.graph_id && c.node_id == h.node_id))
            .take(limit)
            .collect();
        cold.truncate(limit.saturating_sub(hot.len()));
        cold.extend(hot);
        Ok(cold)
    }

    fn hot(&mut self, graph_id: &str) -> Result<&mut NeuronGraph> {
        if !self.graphs.contains_key(graph_id) {
            let data = self.engrams.recall(graph_id)?;
            self.graphs
                .insert(graph_id.to_string(), NeuronGraph::from_data(data)?);
            self.relieve_pressure(graph_id)?;
        }
        Ok(self.graphs.get_mut(graph_id).expect("just inserted"))
    }

    fn turn_focus(&mut self, graph_id: &str, now: i64) -> Result<()> {
        let previous = match &self.focus {
            Some(prev) if prev != graph_id => prev.clone(),
            _ => {
                self.focus = Some(graph_id.to_string());
                return Ok(());
            }
        };
        self.focus = Some(graph_id.to_string());
        if self.graphs.contains_key(&previous) {
            self.hear(&previous, Stimulus::FocusSwitch, now)?;
        }
        Ok(())
    }

    fn relieve_pressure(&mut self, keep: &str) -> Result<()> {
        while self.graphs.len() > self.policy.max_loaded {
            let Some(id) = self.coldest_except(keep) else { break };
            let response = self.policy.evaluate(
                Stimulus::MemoryPressure,
                self.graphs[&id].dirty(),
                0,
                self.graphs.len(),
            );
            match response {
                Response::ConsolidateAndRelease => {
                    self.send_trace(&id)?;
                    self.graphs.remove(&id);
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn coldest_except(&self, keep: &str) -> Option<String> {
        self.graphs
            .iter()
            .filter(|(id, _)| id.as_str() != keep)
            .min_by_key(|(_, g)| g.touched())
            .map(|(id, _)| id.clone())
    }

    /// A stimulus reaches the cortex; the policy answers; we obey.
    fn hear(&mut self, graph_id: &str, stimulus: Stimulus, now: i64) -> Result<()> {
        let Some(graph) = self.graphs.get(graph_id) else {
            return Ok(());
        };
        let idle = now - graph.touched();
        let response =
            self.policy
                .evaluate(stimulus, graph.dirty(), idle, self.graphs.len());
        match response {
            Response::Ignore => Ok(()),
            Response::Consolidate => self.send_trace(graph_id),
            Response::ConsolidateAndRelease => {
                self.send_trace(graph_id)?;
                self.graphs.remove(graph_id);
                Ok(())
            }
        }
    }

    fn send_trace(&mut self, graph_id: &str) -> Result<()> {
        let Some(graph) = self.graphs.get_mut(graph_id) else {
            return Ok(());
        };
        let trace = graph.take_trace();
        self.engrams.consolidate(graph_id, &trace)
    }
}

fn hot_matches(graph_id: &str, graph: &NeuronGraph, query: &str) -> Vec<Hit> {
    let needle = query.to_lowercase();
    graph
        .nodes()
        .iter()
        .filter(|n| {
            n.title.to_lowercase().contains(&needle)
                || n.content.to_lowercase().contains(&needle)
        })
        .map(|n| Hit {
            graph_id: graph_id.to_string(),
            node_id: n.id.clone(),
            title: n.title.clone(),
            rank: 0.0,
        })
        .collect()
}
