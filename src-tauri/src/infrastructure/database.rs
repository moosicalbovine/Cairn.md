use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::domain::library::LibraryError;

const MIGRATION_0001: &str = include_str!("../../migrations/0001_library.sql");

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

        let version: i64 = match connection.query_row("PRAGMA user_version", [], |row| row.get(0)) {
            Ok(version) => version,
            Err(error) if existed_with_data => return Ok(damaged(path, error.to_string())),
            Err(error) => return Err(LibraryError::database(error)),
        };
        if version > 1 {
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
            &["id", "absolute_path", "display_name", "last_scan_at"],
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
                "created_at",
                "updated_at",
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
    }

    Ok(true)
}
