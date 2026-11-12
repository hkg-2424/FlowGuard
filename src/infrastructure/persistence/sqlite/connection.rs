use rusqlite::{Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

use super::schema::apply_migrations;

/// Shared thread-safe handle to a local SQLite database with WAL mode and foreign keys enabled.
#[derive(Clone)]
pub struct SqliteDb {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDb {
    /// Opens or creates a SQLite database file at `path`, enabling WAL mode and applying migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut conn = Connection::open(path)?;
        Self::configure_connection(&mut conn, false)?;
        apply_migrations(&mut conn)?;

        info!("Opened FlowGuard SQLite database at: {}", path.display());
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Creates an in-memory SQLite database instance (primarily for fast integration testing).
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure_connection(&mut conn, true)?;
        apply_migrations(&mut conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn configure_connection(conn: &mut Connection, is_memory: bool) -> Result<()> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")?;

        if !is_memory {
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;
            conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        }

        Ok(())
    }

    /// Access the underlying SQLite connection inside a lock.
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, rusqlite::Error> {
        self.conn.lock().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::other(e.to_string())),
            )
        })
    }
}
