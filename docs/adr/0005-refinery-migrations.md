# ADR-0005: Refinery owns schema migrations

Date: 2026-08-20
Status: accepted

## Context

The engram store hand-rolled ordered migrations over PRAGMA
user_version after rusqlite_migration proved uninstallable (MSRV and
links conflicts). The owner asked for the Flyway model instead.

## Decision

refinery (0.9, rusqlite feature) with migrations embedded from
migrations/*.sql at compile time. Constraint accepted: refinery caps
rusqlite at 0.39, so the crate pins rusqlite 0.39 until refinery
catches up. The stale-binary guard is re-expressed in refinery terms:
a database whose history contains a migration this binary does not
embed refuses to open (abort-on-missing), covered by test.

## Consequences

- DDL lives in versioned SQL files, not Rust strings; new migrations
  are new V{n}__*.sql files.
- refinery_schema_history replaces user_version as the version record.
