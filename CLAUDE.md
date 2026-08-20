# neurons

Thinking-graph layer: NeuronGraph in memory is the truth; the EngramStore
consolidates traces to SQLite. Read docs/DESIGN.md before touching code;
decisions live in docs/adr/ (graph-first, consolidation stimuli, trace
journal, neuro vocabulary).

## Agent protocol (recorded per the flow's pick-once rule)

Serial ticket work: one ticket at a time, each in its own worktree on a
`ticket/<n>-<slug>` branch, landing via a PR the owner merges. Review and
qa gates run in-session before the PR opens.

## Commands

- `cargo test` — full seam suite (pure graph + tempdir engram)
- `cargo clippy --all-targets -- -D warnings` — required clean
- Worktree per ticket: `git worktree add ../neurons-t<N> -b ticket/<n>-<slug>`
