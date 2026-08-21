//! Human-facing views of a graph snapshot: mermaid flowchart,
//! markdown export, and a self-contained interactive HTML page. Pure
//! functions over GraphData — zero I/O, zero SQL; adapters decide where
//! the text goes.

use std::collections::{BTreeMap, HashSet, VecDeque};

use anyhow::{bail, Result};

use crate::types::{GraphData, Node, NodeStatus};

/// Mermaid flowchart of the snapshot. `focus` narrows the view to the
/// nodes within `depth` hops of one thought (both directions); `None`
/// renders the whole graph. Superseded and parked thoughts carry a
/// distinct class; edge labels show weight when the link was reinforced.
pub fn mermaid(data: &GraphData, focus: Option<&str>, depth: usize) -> Result<String> {
    let radius = focus.map(|id| within_radius(data, id, depth)).transpose()?;
    let visible = |id: &str| radius.as_ref().is_none_or(|set| set.contains(id));

    let mut out = String::from("flowchart TD\n");
    let mut seen = (false, false);
    for node in data.nodes.iter().filter(|n| visible(&n.id)) {
        out.push_str(&format!("    {}[\"{}\"]", node.id, escape(&node.title)));
        match node.status {
            NodeStatus::Active => {}
            NodeStatus::Superseded => {
                out.push_str(":::superseded");
                seen.0 = true;
            }
            NodeStatus::Parked => {
                out.push_str(":::parked");
                seen.1 = true;
            }
        }
        out.push('\n');
    }
    for edge in &data.edges {
        if !visible(&edge.from) || !visible(&edge.to) {
            continue;
        }
        out.push_str(&format!(
            "    {} -->|\"{}\"| {}\n",
            edge.from,
            escape(&weighted(&edge.label, edge.weight)),
            edge.to
        ));
    }
    if seen.0 {
        out.push_str("    classDef superseded fill:#eee,color:#888,stroke:#999,stroke-dasharray: 5 5\n");
    }
    if seen.1 {
        out.push_str("    classDef parked fill:#fff8dc,color:#886,stroke:#cc9,stroke-dasharray: 2 3\n");
    }
    Ok(out)
}

/// Readable markdown note: embedded mermaid diagram, status counts,
/// one section per occupied status (superseded thoughts name their
/// forwarding address), then the edges as lines of reasoning. `focus`
/// narrows the diagram, sections, and edges to the radius within
/// `depth` hops of one thought.
pub fn export_md(data: &GraphData, focus: Option<&str>, depth: usize) -> Result<String> {
    let radius = focus.map(|id| within_radius(data, id, depth)).transpose()?;
    let visible = |id: &str| radius.as_ref().is_none_or(|set| set.contains(id));

    let mut out = format!("# {}\n\n", data.meta.title);
    let diagram = mermaid(data, focus, depth)?;
    out.push_str(&format!("```mermaid\n{diagram}```\n\n"));
    let count =
        |status| data.nodes.iter().filter(|n| n.status == status && visible(&n.id)).count();
    out.push_str(&format!(
        "{} active, {} parked, {} superseded\n",
        count(NodeStatus::Active),
        count(NodeStatus::Parked),
        count(NodeStatus::Superseded)
    ));

    let sections = [
        ("Active", NodeStatus::Active),
        ("Parked", NodeStatus::Parked),
        ("Superseded", NodeStatus::Superseded),
    ];
    for (heading, status) in sections {
        let members: Vec<&Node> =
            data.nodes.iter().filter(|n| n.status == status && visible(&n.id)).collect();
        if members.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {heading}\n\n"));
        for node in members {
            out.push_str(&bullet(node));
        }
    }

    let edges: Vec<_> = data
        .edges
        .iter()
        .filter(|e| visible(&e.from) && visible(&e.to))
        .collect();
    if !edges.is_empty() {
        out.push_str("\n## Edges\n\n");
        for edge in edges {
            out.push_str(&format!(
                "- {} -{}-> {}\n",
                edge.from,
                weighted(&edge.label, edge.weight),
                edge.to
            ));
        }
    }
    Ok(out)
}

fn bullet(node: &Node) -> String {
    let mut line = format!("- {} ({}", node.title, node.kind);
    if let Some(stage) = &node.stage {
        line.push_str(&format!(", stage {stage}"));
    }
    line.push_str(&format!(", reinforced {})", node.reinforced));
    if let Some(by) = &node.superseded_by {
        line.push_str(&format!(" -> superseded by {by}"));
    }
    line.push('\n');
    line
}

fn weighted(label: &str, weight: u32) -> String {
    if weight > 1 {
        format!("{label} x{weight}")
    } else {
        label.to_string()
    }
}

/// Mermaid-hostile characters become entity codes so free-form titles
/// cannot break out of their quoted node text.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("#quot;"),
            '`' => escaped.push_str("#96;"),
            '[' => escaped.push_str("#91;"),
            ']' => escaped.push_str("#93;"),
            '{' => escaped.push_str("#123;"),
            '}' => escaped.push_str("#125;"),
            '<' => escaped.push_str("#lt;"),
            '>' => escaped.push_str("#gt;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// BFS over the edges in both directions: every node id within `depth`
/// hops of the focus. Unknown focus is a refusal, not an empty view.
///
/// This is the one authority for radius semantics. Every other view of
/// depth — including the HTML page's client-side scoping — must derive
/// from it: [`adjacency`] + [`radius_from_adjacency`] are its embedded
/// transcription pair, held equivalent by tests/render.rs.
pub fn within_radius(data: &GraphData, focus: &str, depth: usize) -> Result<HashSet<String>> {
    if !data.nodes.iter().any(|n| n.id == focus) {
        bail!("focus node {focus:?} is not in graph {:?}", data.meta.id);
    }
    let mut seen: HashSet<String> = HashSet::from([focus.to_string()]);
    let mut queue: VecDeque<(String, usize)> = VecDeque::from([(focus.to_string(), 0)]);
    while let Some((id, dist)) = queue.pop_front() {
        if dist == depth {
            continue;
        }
        for edge in &data.edges {
            let neighbor = if edge.from == id {
                &edge.to
            } else if edge.to == id {
                &edge.from
            } else {
                continue;
            };
            if seen.insert(neighbor.clone()) {
                queue.push_back((neighbor.clone(), dist + 1));
            }
        }
    }
    Ok(seen)
}

const PAGE_TEMPLATE: &str = include_str!("assets/page.html");
/// d3-force standalone bundle; provenance and ISC license live in the
/// file's own header comment.
const VENDORED_D3_FORCE: &str = include_str!("../vendor/d3-force.min.js");

/// A complete, self-contained interactive page: the graph as a
/// free-floating force-directed SVG with in-page focus and depth
/// controls, a detail card per thought, and status styling parity with
/// the mermaid view. `focus` and `depth` pick the initial scope;
/// rescoping happens client-side over the embedded adjacency list,
/// whose BFS is a transcription of [`radius_from_adjacency`] and is
/// proven equivalent to [`within_radius`] by the render tests.
pub fn export_html(data: &GraphData, focus: Option<&str>, depth: usize) -> Result<String> {
    if let Some(id) = focus {
        within_radius(data, id, depth)?;
    }
    let payload = serde_json::json!({
        "data": data,
        "adjacency": adjacency(data),
        "initial": { "focus": focus, "depth": depth },
    });
    // Every `<` becomes \u003c inside the JSON so no free-form text can
    // smuggle a closing script tag into the payload element.
    let json = serde_json::to_string(&payload)?.replace('<', "\\u003c");
    let page = fill(PAGE_TEMPLATE, "@@NEURON:VENDOR@@", VENDORED_D3_FORCE)?;
    let page = fill(&page, "@@NEURON:TITLE@@", &escape_html(&data.meta.title))?;
    fill(&page, "@@NEURON:PAYLOAD@@", &json)
}

/// Splice `value` into the template at its `marker`.
fn fill(template: &str, marker: &str, value: &str) -> Result<String> {
    let Some((before, after)) = template.split_once(marker) else {
        bail!("page template lost its {marker} marker");
    };
    Ok(format!("{before}{value}{after}"))
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// Undirected neighbor lists in [`within_radius`]'s exact edge-scan
/// order: for each node every edge is probed `from` side first, and
/// parallel edges keep their duplicate entries. The HTML page embeds
/// this list and walks it with a transcription of
/// [`radius_from_adjacency`].
pub fn adjacency(data: &GraphData) -> BTreeMap<String, Vec<String>> {
    data.nodes
        .iter()
        .map(|node| {
            let neighbors = data
                .edges
                .iter()
                .filter_map(|edge| {
                    if edge.from == node.id {
                        Some(edge.to.clone())
                    } else if edge.to == node.id {
                        Some(edge.from.clone())
                    } else {
                        None
                    }
                })
                .collect();
            (node.id.clone(), neighbors)
        })
        .collect()
}

/// BFS over a precomputed adjacency list — the algorithm the page's JS
/// transcribes step for step. Must stay membership-equivalent to
/// [`within_radius`]; tests/render.rs fails if they ever diverge.
pub fn radius_from_adjacency(
    adjacency: &BTreeMap<String, Vec<String>>,
    focus: &str,
    depth: usize,
) -> HashSet<String> {
    let mut seen: HashSet<String> = HashSet::from([focus.to_string()]);
    let mut queue: VecDeque<(String, usize)> = VecDeque::from([(focus.to_string(), 0)]);
    while let Some((id, dist)) = queue.pop_front() {
        if dist == depth {
            continue;
        }
        for neighbor in adjacency.get(&id).map_or(&[][..], Vec::as_slice) {
            if seen.insert(neighbor.clone()) {
                queue.push_back((neighbor.clone(), dist + 1));
            }
        }
    }
    seen
}
