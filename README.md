# neurons

A thinking-graph layer for AI agents: many small graphs of ideas that
survive context compaction. Thoughts are captured as nodes, connected
and reinforced through discussion, corrected by supersession (never
deleted), parked when set aside, and settled when done. The in-memory
graph is the truth; a consolidation policy decides when it lands in
long-term storage (SQLite, machine-local).

## Install

Prebuilt binary from a release (needs gh auth while the repo is private):

    gh release download v1.0.0 -R trungth1406/neurons -p '*aarch64-apple-darwin*'
    tar xzf neuron-mcp-*.tar.gz && mv neuron-mcp ~/.cargo/bin/

Or build from source (Rust toolchain required):

    cargo install --git https://github.com/trungth1406/neurons --locked

Updating is the same command again. The database self-migrates on the
next open; an older binary refuses a newer database rather than corrupt it.

## Register with Claude Code

    claude mcp add neurons -- ~/.cargo/bin/neuron-mcp

That is the entire deployment. First run creates ~/.claude/neurons/ and
the database with full schema; nothing else to set up.

## Tools

Reads: summary, show, search, path, list — all budget-capped.
Writes: new_graph, add_node, link, reinforce, supersede, set_stage,
park, unpark, settle, reopen, consolidate.

## Configuration

All optional; defaults in parentheses.

| flag | env | meaning |
|---|---|---|
| --db | NEURON_DB | database path (~/.claude/neurons/neurons.db) |
| --tick-secs | NEURON_TICK_SECS | sweeper interval (15) |
| --dirty-threshold | NEURON_DIRTY_THRESHOLD | ops before consolidation (10) |
| --quiet-secs | NEURON_QUIET_SECS | idle seconds before sweep consolidates (60) |
| --max-loaded | NEURON_MAX_LOADED | graphs held hot (8) |

## Privacy

Thinking data lives only in the local SQLite file and never enters this
repository. The repo is where code lives; thoughts stay on the machine.

## Development

    cargo test                                # full suite
    cargo clippy --all-targets -- -D warnings # required clean

Design: docs/DESIGN.md. Decisions: docs/adr/. CI gates every PR on
ubuntu and macos; releases fire from version tags after the same gate.
