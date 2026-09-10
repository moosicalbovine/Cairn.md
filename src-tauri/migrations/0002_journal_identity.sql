ALTER TABLE pending_file_operations ADD COLUMN temporary_identity TEXT;
ALTER TABLE pending_file_operations ADD COLUMN finalized_identity TEXT;

PRAGMA user_version = 2;
