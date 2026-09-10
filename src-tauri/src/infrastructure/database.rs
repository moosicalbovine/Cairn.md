use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::domain::library::LibraryError;

const MIGRATION_0001: &str = include_str!("../../migrations/0001_library.sql");
const MIGRATION_0002: &str = include_str!("../../migrations/0002_journal_identity.sql");
const MIGRATION_0003: &str = include_str!("../../migrations/0003_tracked_folders.sql");
const MIGRATION_0004: &str = include_str!("../../migrations/0004_recovery.sql");

pub const DATABASE_FILENAME: &str = "library.sqlite3";

pub enum DatabaseOpen {
    Healthy(Database),
    Damaged { path: PathBuf, reason: String },
}

pub struct Database {
    connection: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(app_data_dir: &Path) -> Result<DatabaseOpen, LibraryError> {
        fs::create_dir_all(app_data_dir).map_err(LibraryError::io)?;
        let path = app_data_dir.join(DATABASE_FILENAME);
        let existed_with_data = fs::metadata(&path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false);
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection = match Connection::open_with_flags(&path, flags) {
            Ok(connection) => connection,
            Err(error) if existed_with_data => {
                return Ok(damaged(path, error.to_string()));
            }
            Err(error) => return Err(LibraryError::database(error)),
        };

        if let Err(error) = configure(&connection) {
            if existed_with_data {
                return Ok(damaged(path, error.to_string()));
            }
            return Err(LibraryError::database(error));
        }

        if existed_with_data {
            match integrity_check(&connection) {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(damaged(path, "SQLite integrity_check failed".to_owned()));
                }
                Err(error) => {
                    return Ok(damaged(path, error.to_string()));
                }
            }
        }

        let mut version: i64 =
            match connection.query_row("PRAGMA user_version", [], |row| row.get(0)) {
                Ok(version) => version,
                Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
                Err(error) => return Err(LibraryError::database(error)),
            };
        if version > 4 {
            return Ok(damaged(
                path,
                format!("Unsupported metadata schema version {version}"),
            ));
        }
        if version < 1 {
            let transaction = match connection.transaction() {
                Ok(transaction) => transaction,
                Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
                Err(error) => return Err(LibraryError::database(error)),
            };
            if let Err(error) = transaction.execute_batch(MIGRATION_0001) {
                return if existed_with_data {
                    Ok(damaged(path, format!("Metadata migration failed: {error}")))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            if let Err(error) = transaction.commit() {
                return if existed_with_data {
                    Ok(damaged(path, error.to_string()))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            version = 1;
        }
        if version < 2 {
            let transaction = match connection.transaction() {
                Ok(transaction) => transaction,
                Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
                Err(error) => return Err(LibraryError::database(error)),
            };
            if let Err(error) = transaction.execute_batch(MIGRATION_0002) {
                return if existed_with_data {
                    Ok(damaged(path, format!("Metadata migration failed: {error}")))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            if let Err(error) = transaction.commit() {
                return if existed_with_data {
                    Ok(damaged(path, error.to_string()))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            version = 2;
        }
        if version < 3 {
            let transaction = match connection.transaction() {
                Ok(transaction) => transaction,
                Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
                Err(error) => return Err(LibraryError::database(error)),
            };
            if let Err(error) = transaction.execute_batch(MIGRATION_0003) {
                return if existed_with_data {
                    Ok(damaged(path, format!("Metadata migration failed: {error}")))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            if let Err(error) = transaction.commit() {
                return if existed_with_data {
                    Ok(damaged(path, error.to_string()))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            version = 3;
        }
        if version < 4 {
            let transaction = match connection.transaction() {
                Ok(transaction) => transaction,
                Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
                Err(error) => return Err(LibraryError::database(error)),
            };
            if let Err(error) = transaction.execute_batch(MIGRATION_0004) {
                return if existed_with_data {
                    Ok(damaged(path, format!("Metadata migration failed: {error}")))
                } else {
                    Err(LibraryError::database(error))
                };
            }
            if let Err(error) = transaction.commit() {
                return if existed_with_data {
                    Ok(damaged(path, error.to_string()))
                } else {
                    Err(LibraryError::database(error))
                };
            }
        }

        match schema_check(&connection) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(damaged(
                    path,
                    "SQLite schema does not match the supported metadata model".to_owned(),
                ));
            }
            Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
            Err(error) => return Err(LibraryError::database(error)),
        }

        match integrity_check(&connection) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(damaged(
                    path,
                    "SQLite integrity_check failed after migration".to_owned(),
                ));
            }
            Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
            Err(error) => return Err(LibraryError::database(error)),
        }

        Ok(DatabaseOpen::Healthy(Self { connection, path }))
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn preserve_evidence(&self) {
        preserve_failure_evidence(&self.path);
    }
}

fn damaged(path: PathBuf, reason: String) -> DatabaseOpen {
    preserve_failure_evidence(&path);
    DatabaseOpen::Damaged { path, reason }
}

fn preserve_failure_evidence(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let evidence = parent.join("metadata-quarantine");
    if fs::create_dir_all(&evidence).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    for suffix in ["", "-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        if source.is_file() {
            let destination = evidence.join(format!("library-{stamp}.sqlite3{suffix}"));
            let _ = fs::copy(source, destination);
        }
    }
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn integrity_check(connection: &Connection) -> rusqlite::Result<bool> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    Ok(result == "ok")
}

fn schema_check(connection: &Connection) -> rusqlite::Result<bool> {
    const REQUIRED_TABLES: &[(&str, &[&str])] = &[
        ("app_settings", &["key", "value"]),
        (
            "libraries",
            &[
                "id",
                "root_path",
                "root_identity",
                "binding_generation",
                "previous_root_path",
                "manifest_json",
                "updated_at",
            ],
        ),
        (
            "projects",
            &[
                "id",
                "library_id",
                "relative_path",
                "path_key",
                "file_identity",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "documents",
            &[
                "id",
                "library_id",
                "project_id",
                "relative_path",
                "path_key",
                "source_path",
                "imported_at",
                "disk_fingerprint",
                "disk_revision",
                "file_identity",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "tracked_folders",
            &[
                "id",
                "absolute_path",
                "display_name",
                "last_scan_at",
                "path_key",
                "folder_identity",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "pending_file_operations",
            &[
                "id",
                "library_id",
                "kind",
                "phase",
                "payload_json",
                "expected_fingerprint",
                "finalized_fingerprint",
                "temporary_path",
                "temporary_identity",
                "finalized_identity",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "recovery_snapshots",
            &[
                "document_id",
                "session_generation",
                "revision",
                "content",
                "content_hash",
                "base_fingerprint",
                "intended_disk_hash",
                "operation_id",
                "lifecycle_state",
                "durable_at",
            ],
        ),
        (
            "external_conflicts",
            &[
                "document_id",
                "operation_id",
                "external_bytes",
                "external_hash",
                "draft_revision",
                "draft_hash",
                "captured_at",
            ],
        ),
    ];

    for &(table, required_columns) in REQUIRED_TABLES {
        let object_type = connection
            .query_row(
                "SELECT type FROM sqlite_master WHERE name = ?1",
                [table],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if object_type.as_deref() != Some("table") {
            return Ok(false);
        }

        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if required_columns
            .iter()
            .any(|required| !columns.iter().any(|column| column == *required))
        {
            return Ok(false);
        }
        let schema_sql = connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get::<_, String>(0),
        )?;
        if !schema_sql.to_ascii_uppercase().contains("STRICT") {
            return Ok(false);
        }
    }

    if !has_unique_index(connection, "projects", &["library_id", "path_key"])?
        || !has_unique_index(connection, "documents", &["library_id", "path_key"])?
        || !has_unique_index(connection, "tracked_folders", &["path_key"])?
        || !has_foreign_key(
            connection,
            "projects",
            "library_id",
            "libraries",
            "id",
            "CASCADE",
        )?
        || !has_foreign_key(
            connection,
            "documents",
            "library_id",
            "libraries",
            "id",
            "CASCADE",
        )?
        || !has_foreign_key(
            connection,
            "documents",
            "project_id",
            "projects",
            "id",
            "CASCADE",
        )?
        || !has_foreign_key(
            connection,
            "pending_file_operations",
            "library_id",
            "libraries",
            "id",
            "CASCADE",
        )?
        || !has_foreign_key(
            connection,
            "recovery_snapshots",
            "document_id",
            "documents",
            "id",
            "CASCADE",
        )?
        || !has_foreign_key(
            connection,
            "external_conflicts",
            "document_id",
            "documents",
            "id",
            "CASCADE",
        )?
    {
        return Ok(false);
    }

    for index in [
        "projects_library_idx",
        "documents_project_idx",
        "pending_operations_library_idx",
        "recovery_snapshots_durable_idx",
        "external_conflicts_captured_idx",
    ] {
        let exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(false);
        }
    }

    let phase_check = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'pending_file_operations'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    if [
        "Intent recorded",
        "Temporary durable",
        "Filesystem finalized",
        "Metadata committed",
        "Cleanup complete",
    ]
    .iter()
    .any(|phase| !phase_check.contains(phase))
    {
        return Ok(false);
    }

    let foreign_key_violation = connection
        .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |_| {
            Ok(())
        })
        .optional()?;
    if foreign_key_violation.is_some() {
        return Ok(false);
    }

    Ok(true)
}

fn has_unique_index(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> rusqlite::Result<bool> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_index_list(?1) WHERE \"unique\" = 1 ORDER BY name")?;
    let indexes = statement
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for index in indexes {
        let mut columns_statement =
            connection.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = columns_statement
            .query_map([index], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns
            .iter()
            .map(String::as_str)
            .eq(expected_columns.iter().copied())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_foreign_key(
    connection: &Connection,
    table: &str,
    from_column: &str,
    target_table: &str,
    target_column: &str,
    on_delete: &str,
) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM pragma_foreign_key_list(?1) WHERE \"from\" = ?2 AND \"table\" = ?3 AND \"to\" = ?4 AND on_delete = ?5 LIMIT 1",
            [table, from_column, target_table, target_column, on_delete],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
}
