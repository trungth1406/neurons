//! Human-facing views of a graph snapshot: mermaid flowchart and
//! markdown export. Pure functions over GraphData — zero I/O, zero SQL;
//! adapters decide where the text goes.

use std::collections::{HashSet, VecDeque};

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

/// Readable markdown note: title, status counts, one section per
/// occupied status (superseded thoughts name their forwarding address),
/// then the edges as lines of reasoning.
pub fn export_md(data: &GraphData) -> String {
    let mut out = format!("# {}\n\n", data.meta.title);
    if let Ok(diagram) = mermaid(data, None, 0) {
        out.push_str(&format!("```mermaid\n{diagram}```\n\n"));
    }
    let count = |status| data.nodes.iter().filter(|n| n.status == status).count();
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
        let members: Vec<&Node> = data.nodes.iter().filter(|n| n.status == status).collect();
        if members.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {heading}\n\n"));
        for node in members {
            out.push_str(&bullet(node));
        }
    }

    if !data.edges.is_empty() {
        out.push_str("\n## Edges\n\n");
        for edge in &data.edges {
            out.push_str(&format!(
                "- {} -{}-> {}\n",
                edge.from,
                weighted(&edge.label, edge.weight),
                edge.to
            ));
        }
    }
    out
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
fn within_radius(data: &GraphData, focus: &str, depth: usize) -> Result<HashSet<String>> {
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
