CREATE TABLE graphs (
  id      TEXT PRIMARY KEY,
  title   TEXT NOT NULL,
  status  TEXT NOT NULL DEFAULT 'active',
  project TEXT,
  created INTEGER NOT NULL,
  updated INTEGER NOT NULL
);

CREATE TABLE nodes (
  nid      INTEGER PRIMARY KEY,
  graph_id TEXT NOT NULL REFERENCES graphs(id),
  id       TEXT NOT NULL,
  kind     TEXT NOT NULL,
  title    TEXT NOT NULL,
  content  TEXT NOT NULL DEFAULT '',
  content_encoding TEXT NOT NULL DEFAULT 'plain',
  status   INTEGER NOT NULL DEFAULT 0,
  stage    TEXT,
  skills   TEXT NOT NULL DEFAULT '[]',
  reinforced INTEGER NOT NULL DEFAULT 1,
  superseded_by TEXT,
  created  INTEGER NOT NULL,
  updated  INTEGER NOT NULL,
  UNIQUE (graph_id, id)
);

CREATE TABLE edges (
  graph_id TEXT NOT NULL,
  from_id  TEXT NOT NULL,
  to_id    TEXT NOT NULL,
  label    TEXT NOT NULL,
  weight   INTEGER NOT NULL DEFAULT 1,
  created  INTEGER NOT NULL,
  PRIMARY KEY (graph_id, from_id, to_id, label)
) WITHOUT ROWID;

CREATE INDEX edges_in ON edges (graph_id, to_id);

CREATE VIRTUAL TABLE node_fts USING fts5(
  title, content,
  content='nodes', content_rowid='nid'
);

CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
  INSERT INTO node_fts(rowid, title, content)
  VALUES (new.nid, new.title, new.content);
END;

CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
  INSERT INTO node_fts(node_fts, rowid, title, content)
  VALUES ('delete', old.nid, old.title, old.content);
END;

CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
  INSERT INTO node_fts(node_fts, rowid, title, content)
  VALUES ('delete', old.nid, old.title, old.content);
  INSERT INTO node_fts(rowid, title, content)
  VALUES (new.nid, new.title, new.content);
END;
