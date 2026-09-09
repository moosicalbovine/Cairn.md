CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS libraries (
    id TEXT PRIMARY KEY NOT NULL,
    root_path TEXT NOT NULL,
    root_identity TEXT NOT NULL,
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    previous_root_path TEXT,
    manifest_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    path_key TEXT NOT NULL,
    file_identity TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (library_id, path_key)
) STRICT;

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    path_key TEXT NOT NULL,
    source_path TEXT,
    imported_at INTEGER,
    disk_fingerprint TEXT NOT NULL,
    disk_revision INTEGER NOT NULL DEFAULT 0,
    file_identity TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (library_id, path_key)
) STRICT;

CREATE TABLE IF NOT EXISTS tracked_folders (
    id TEXT PRIMARY KEY NOT NULL,
    absolute_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    last_scan_at INTEGER
) STRICT;

CREATE TABLE IF NOT EXISTS pending_file_operations (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN (
        'Intent recorded',
        'Temporary durable',
        'Filesystem finalized',
        'Metadata committed',
        'Cleanup complete'
    )),
    payload_json TEXT NOT NULL,
    expected_fingerprint TEXT,
    finalized_fingerprint TEXT,
    temporary_path TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS projects_library_idx ON projects(library_id);
CREATE INDEX IF NOT EXISTS documents_project_idx ON documents(project_id);
CREATE INDEX IF NOT EXISTS pending_operations_library_idx
    ON pending_file_operations(library_id, created_at);

PRAGMA user_version = 1;
