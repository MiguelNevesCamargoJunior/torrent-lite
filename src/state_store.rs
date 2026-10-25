//! Transactional intent store. The service must take its process lock before opening it.

use crate::{InputError, magnet::Magnet, torrent_file::inspect_torrent};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    fs::{File, OpenOptions as FileOpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

const SCHEMA_VERSION: i64 = 1;
const APPLICATION_ID: i64 = 0x746c6974;
const MAX_TORRENTS: i64 = 100;
const MAX_SOURCE_BYTES: i64 = 8 * 1024 * 1024;
const CREATE_TORRENTS_SQL: &str = "CREATE TABLE torrents (
                    id TEXT PRIMARY KEY NOT NULL,
                    info_hash TEXT NOT NULL UNIQUE,
                    source_kind TEXT NOT NULL CHECK(source_kind IN ('magnet','torrent')),
                    source BLOB NOT NULL,
                    destination_root TEXT NOT NULL,
                    desired TEXT NOT NULL CHECK(desired IN ('running','paused')),
                    tombstone INTEGER NOT NULL DEFAULT 0 CHECK(tombstone IN (0,1))
                )";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredState {
    Running,
    Paused,
}

impl DesiredState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }

    fn from_str(value: &str) -> Result<Self, StoreError> {
        match value {
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            _ => Err(StoreError::Corrupt),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Magnet,
    Torrent,
}

impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Magnet => "magnet",
            Self::Torrent => "torrent",
        }
    }

    fn from_str(value: &str) -> Result<Self, StoreError> {
        match value {
            "magnet" => Ok(Self::Magnet),
            "torrent" => Ok(Self::Torrent),
            _ => Err(StoreError::Corrupt),
        }
    }
}

pub enum AddInput<'a> {
    Magnet(&'a str),
    Torrent(&'a [u8]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTorrent {
    pub id: String,
    pub info_hash: String,
    pub source_kind: SourceKind,
    pub destination_root: String,
    pub desired: DesiredState,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("invalid torrent input: {0}")]
    Input(#[from] InputError),
    #[error("unable to use state or download directory")]
    Io(#[from] std::io::Error),
    #[error("state database error")]
    Database(#[from] rusqlite::Error),
    #[error("state database schema is unsupported")]
    UnsupportedSchema,
    #[error("state database contains invalid data")]
    Corrupt,
    #[error("torrent already exists with incompatible settings")]
    Conflict,
    #[error("torrent registry is full")]
    Limit,
    #[error("torrent ID does not exist")]
    NotFound,
    #[error("state database is already owned by another process")]
    Busy,
}

pub struct StateStore {
    connection: Connection,
    _lock_file: File,
}

impl StateStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut lock_name = path
            .file_name()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "database path has no file name",
                )
            })?
            .to_os_string();
        lock_name.push(".lock");
        let lock_file = FileOpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.with_file_name(lock_name))?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(StoreError::Busy),
            Err(TryLockError::Error(error)) => return Err(StoreError::Io(error)),
        }
        let existed = path.exists();
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        if existed {
            let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            let app_id: i64 =
                connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
            if version != SCHEMA_VERSION || app_id != APPLICATION_ID {
                return Err(StoreError::UnsupportedSchema);
            }
            let actual_schema: Option<String> = connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name='torrents'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if actual_schema.as_deref() != Some(CREATE_TORRENTS_SQL) {
                return Err(StoreError::UnsupportedSchema);
            }
        } else {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(&format!(
                "{CREATE_TORRENTS_SQL}; PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = {SCHEMA_VERSION};"
            ))?;
            transaction.commit()?;
        }
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=WAL;",
        )?;
        Ok(Self {
            connection,
            _lock_file: lock_file,
        })
    }

    pub fn register(
        &mut self,
        input: AddInput<'_>,
        destination_root: &Path,
        paused: bool,
    ) -> Result<StoredTorrent, StoreError> {
        let (kind, source, info_hash) = match input {
            AddInput::Magnet(uri) => {
                let parsed = Magnet::parse(uri)?;
                (
                    SourceKind::Magnet,
                    uri.as_bytes(),
                    parsed.info_hash().to_owned(),
                )
            }
            AddInput::Torrent(bytes) => {
                let parsed = inspect_torrent(bytes)?;
                (SourceKind::Torrent, bytes, parsed.info_hash)
            }
        };
        let destination = destination_root.canonicalize()?;
        if !destination.is_dir() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "download root is not a directory",
            )));
        }
        let destination = destination.to_str().ok_or_else(|| {
            StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "download root is not UTF-8",
            ))
        })?;
        if destination.len() > 4096 {
            return Err(StoreError::Input(InputError::TooLarge));
        }
        let desired = if paused {
            DesiredState::Paused
        } else {
            DesiredState::Running
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, String, String, i64)> = transaction
            .query_row(
                "SELECT id, source_kind, destination_root, desired, tombstone FROM torrents WHERE info_hash=?1",
                [&info_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        if let Some((id, stored_kind, stored_destination, stored_desired, tombstone)) = existing {
            if tombstone != 0 || stored_destination != destination {
                return Err(StoreError::Conflict);
            }
            return Ok(StoredTorrent {
                id,
                info_hash,
                source_kind: SourceKind::from_str(&stored_kind)?,
                destination_root: stored_destination,
                desired: DesiredState::from_str(&stored_desired)?,
            });
        }
        let count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM torrents WHERE tombstone=0",
            [],
            |row| row.get(0),
        )?;
        if count >= MAX_TORRENTS {
            return Err(StoreError::Limit);
        }
        let id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO torrents(id,info_hash,source_kind,source,destination_root,desired) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, info_hash, kind.as_str(), source, destination, desired.as_str()],
        )?;
        transaction.commit()?;
        Ok(StoredTorrent {
            id,
            info_hash,
            source_kind: kind,
            destination_root: destination.to_owned(),
            desired,
        })
    }

    pub fn set_intent(&mut self, id: &str, desired: DesiredState) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE torrents SET desired=?1 WHERE id=?2 AND tombstone=0",
            params![desired.as_str(), id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE torrents SET tombstone=1, desired='paused' WHERE id=?1",
            [id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn recover(&self) -> Result<Vec<StoredTorrent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id,info_hash,source_kind,destination_root,desired FROM torrents WHERE tombstone=0 ORDER BY id",
        )?;
        let records = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut output = Vec::new();
        for record in records {
            let (id, info_hash, kind, destination_root, desired) = record?;
            output.push(StoredTorrent {
                id,
                info_hash,
                source_kind: SourceKind::from_str(&kind)?,
                destination_root,
                desired: DesiredState::from_str(&desired)?,
            });
            if output.len() > MAX_TORRENTS as usize {
                return Err(StoreError::Corrupt);
            }
        }
        Ok(output)
    }

    pub fn load_source(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let length: Option<i64> = self
            .connection
            .query_row(
                "SELECT length(source) FROM torrents WHERE id=?1 AND tombstone=0",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        match length {
            None => return Err(StoreError::NotFound),
            Some(length) if !(0..=MAX_SOURCE_BYTES).contains(&length) => {
                return Err(StoreError::Corrupt);
            }
            _ => {}
        }
        Ok(self.connection.query_row(
            "SELECT source FROM torrents WHERE id=?1 AND tombstone=0",
            [id],
            |row| row.get(0),
        )?)
    }
}
