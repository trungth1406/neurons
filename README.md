# neurons

A thinking-graph layer for AI agents: many small graphs of ideas that
survive context compaction. Thoughts are captured as nodes, connected
and reinforced through discussion, corrected by supersession (never
deleted), parked when set aside, and settled when done. The in-memory
graph is the truth; a consolidation policy decides when it lands in
long-term storage (SQLite, machine-local).

## Install

One line (needs `gh` auth while the repo is private):

    curl -fsSL https://raw.githubusercontent.com/trungth1406/neurons/main/install.sh | bash

Or build from source (Rust toolchain required):

    cargo install --git https://github.com/trungth1406/neurons --locked

Both install `neuron-mcp` to `~/.cargo/bin/` and register it with Claude Code.
Updating is the same command again. The database self-creates on first use
and self-migrates on upgrade; an older binary refuses a newer database
rather than corrupt it.

## Configuration

All optional; defaults in parentheses.

| flag | env | meaning |
|---|---|---|
| --db | NEURON_DB | database path (~/.claude/neurons/<project>/neurons.db) |
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
