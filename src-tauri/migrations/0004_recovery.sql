CREATE TABLE recovery_snapshots (
    document_id TEXT PRIMARY KEY NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    session_generation TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    content BLOB NOT NULL,
    content_hash TEXT NOT NULL,
    base_fingerprint TEXT NOT NULL,
    intended_disk_hash TEXT NOT NULL,
    operation_id TEXT,
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN (
        'Draft',
        'Saving',
        'Recovered',
        'Conflict'
    )),
    durable_at INTEGER NOT NULL
) STRICT;

CREATE TABLE external_conflicts (
    document_id TEXT PRIMARY KEY NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    operation_id TEXT NOT NULL UNIQUE,
    external_bytes BLOB NOT NULL,
    external_hash TEXT NOT NULL,
    draft_revision INTEGER NOT NULL CHECK (draft_revision > 0),
    draft_hash TEXT NOT NULL,
    captured_at INTEGER NOT NULL
) STRICT;

CREATE INDEX recovery_snapshots_durable_idx
    ON recovery_snapshots(durable_at);
CREATE INDEX external_conflicts_captured_idx
    ON external_conflicts(captured_at);

PRAGMA user_version = 4;
