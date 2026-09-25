//! Candidate-only journal IO. No pricing, authority decisions, or production migrations.
use super::owner::Owner;
use sqlx::{Connection, Row, SqliteConnection, sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous}};
use std::{path::Path, time::Duration};

pub(crate) struct CandidateStore {
    connection: SqliteConnection,
    _owner: Owner,
}
impl CandidateStore {
    pub(crate) async fn open(path: &Path, create: bool) -> Result<Self, ()> {
        let owner = Owner::acquire(path).map_err(|_| ())?;
        if create {
            crate::local::write_new(&owner.database, b"").map_err(|_| ())?;
        } else {
            crate::local::private_existing(&owner.database, false).map_err(|_| ())?;
        }
        let options = SqliteConnectOptions::new().filename(&owner.database).create_if_missing(false)
            .journal_mode(SqliteJournalMode::Wal).synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let connection = SqliteConnection::connect_with(&options).await.map_err(|_| ())?;
        owner.verify_path().map_err(|_| ())?;
        Ok(Self { connection, _owner: owner })
    }
    pub(crate) async fn begin(&mut self) -> Result<(), ()> {
        sqlx::query("BEGIN IMMEDIATE").execute(&mut self.connection).await.map_err(|_| ())?;
        Ok(())
    }
    pub(crate) async fn initialize(&mut self, setup: &[u8]) -> Result<(), ()> {
        for query in [
            "CREATE TABLE candidate_setup (id INTEGER PRIMARY KEY CHECK(id=1), body BLOB NOT NULL)",
            "CREATE TABLE candidate_entries (seq INTEGER PRIMARY KEY, command BLOB NOT NULL, at_us TEXT NOT NULL, response BLOB NOT NULL)",
            "CREATE TRIGGER setup_no_update BEFORE UPDATE ON candidate_setup BEGIN SELECT RAISE(ABORT,'immutable'); END",
            "CREATE TRIGGER setup_no_delete BEFORE DELETE ON candidate_setup BEGIN SELECT RAISE(ABORT,'immutable'); END",
            "CREATE TRIGGER entries_no_update BEFORE UPDATE ON candidate_entries BEGIN SELECT RAISE(ABORT,'immutable'); END",
            "CREATE TRIGGER entries_no_delete BEFORE DELETE ON candidate_entries BEGIN SELECT RAISE(ABORT,'immutable'); END",
        ] { sqlx::query(query).execute(&mut self.connection).await.map_err(|_| ())?; }
        sqlx::query("INSERT INTO candidate_setup VALUES (1, ?)").bind(setup).execute(&mut self.connection).await.map_err(|_| ())?;
        Ok(())
    }
    pub(crate) async fn setup(&mut self) -> Result<Vec<u8>, ()> {
        let row = sqlx::query("SELECT body FROM candidate_setup WHERE id=1").fetch_one(&mut self.connection).await.map_err(|_| ())?;
        row.try_get(0).map_err(|_| ())
    }
    pub(crate) async fn rows(&mut self) -> Result<Vec<(i64, Vec<u8>, String, Vec<u8>)>, ()> {
        let rows = sqlx::query("SELECT seq,command,at_us,response FROM candidate_entries ORDER BY seq LIMIT 33")
            .fetch_all(&mut self.connection).await.map_err(|_| ())?;
        rows.into_iter().map(|r| Ok((r.try_get(0).map_err(|_| ())?, r.try_get(1).map_err(|_| ())?,
            r.try_get(2).map_err(|_| ())?, r.try_get(3).map_err(|_| ())?))).collect()
    }
    pub(crate) async fn append(&mut self, seq: i64, command: &[u8], at: u64, response: &[u8]) -> Result<(), ()> {
        sqlx::query("INSERT INTO candidate_entries VALUES (?,?,?,?)").bind(seq).bind(command)
            .bind(at.to_string()).bind(response).execute(&mut self.connection).await.map_err(|_| ())?;
        Ok(())
    }
    pub(crate) async fn end(&mut self, commit: bool) -> Result<(), ()> {
        sqlx::query(if commit { "COMMIT" } else { "ROLLBACK" }).execute(&mut self.connection).await.map_err(|_| ())?;
        Ok(())
    }
    pub(crate) async fn close(self) {
        // Keep the owner guard alive while the connection is closed.
        let Self { connection, _owner } = self;
        let _ = connection.close().await;
        drop(_owner);
    }
}
