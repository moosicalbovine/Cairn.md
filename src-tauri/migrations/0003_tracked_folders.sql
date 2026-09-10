ALTER TABLE tracked_folders ADD COLUMN path_key TEXT;
ALTER TABLE tracked_folders ADD COLUMN folder_identity TEXT;
ALTER TABLE tracked_folders ADD COLUMN created_at INTEGER;
ALTER TABLE tracked_folders ADD COLUMN updated_at INTEGER;

UPDATE tracked_folders
SET path_key = lower(replace(absolute_path, '\', '/')),
    created_at = coalesce(last_scan_at, 0),
    updated_at = coalesce(last_scan_at, 0);

CREATE UNIQUE INDEX tracked_folders_path_idx ON tracked_folders(path_key);

PRAGMA user_version = 3;
