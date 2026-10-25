//! Transactional intent store. The service must take its process lock before opening it.

use crate::{
    InputError, magnet::Magnet, path_policy::validate_components, safe_storage::FileIdentity,
    torrent_file::inspect_torrent,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    fs::{File, OpenOptions as FileOpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

const SCHEMA_VERSION: i64 = 4;
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
const CREATE_FILE_IDENTITIES_SQL: &str = "CREATE TABLE file_identities (
                    torrent_id TEXT NOT NULL REFERENCES torrents(id),
                    file_index INTEGER NOT NULL CHECK(file_index BETWEEN 0 AND 9999),
                    path_json TEXT NOT NULL,
                    length INTEGER NOT NULL CHECK(length >= 0),
                    identity BLOB NOT NULL CHECK(length(identity) = 24),
                    PRIMARY KEY(torrent_id, file_index)
                )";
const CREATE_RESOLVED_METAINFO_SQL: &str = "CREATE TABLE resolved_metainfo (
                    torrent_id TEXT PRIMARY KEY NOT NULL REFERENCES torrents(id),
                    metainfo BLOB NOT NULL CHECK(length(metainfo) BETWEEN 1 AND 8388608)
                )";
const CREATE_FILE_SELECTION_SQL: &str = "CREATE TABLE file_selection (
                    torrent_id TEXT PRIMARY KEY NOT NULL REFERENCES torrents(id),
                    selection_json TEXT NOT NULL CHECK(length(selection_json) BETWEEN 3 AND 60000)
                )";

fn matches_schema(
    connection: &Connection,
    table: &str,
    expected: &str,
) -> Result<bool, StoreError> {
    let actual: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .optional()?;
    Ok(actual.as_deref() == Some(expected))
}

fn valid_file_selection(indices: &[usize], file_count: usize) -> bool {
    !indices.is_empty()
        && indices.len() <= 10_000
        && indices.iter().all(|index| *index < file_count)
        && indices.windows(2).all(|pair| pair[0] < pair[1])
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFileIdentity {
    pub file_index: u32,
    pub path: Vec<String>,
    pub length: u64,
    pub identity: FileIdentity,
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
    #[error("migration backup already exists; inspect state before retrying")]
    MigrationBackupExists,
}

pub struct StateStore {
    connection: Connection,
    _lock_file: ProcessLock,
}

struct ProcessLock(File);

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
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
            .truncate(false)
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
            if !(1..=SCHEMA_VERSION).contains(&version)
                || app_id != APPLICATION_ID
                || !matches_schema(&connection, "torrents", CREATE_TORRENTS_SQL)?
            {
                return Err(StoreError::UnsupportedSchema);
            }
            if version < SCHEMA_VERSION {
                let mut backup_name = path
                    .file_name()
                    .ok_or(StoreError::UnsupportedSchema)?
                    .to_os_string();
                backup_name.push(format!(".v{version}.bak"));
                let backup_path = path.with_file_name(backup_name);
                match backup_path.symlink_metadata() {
                    Ok(_) => return Err(StoreError::MigrationBackupExists),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(StoreError::Io(error)),
                }
                let backup_path = backup_path.to_str().ok_or(StoreError::UnsupportedSchema)?;
                connection.execute_batch("PRAGMA synchronous=FULL;")?;
                connection.execute("VACUUM INTO ?1", [backup_path])?;
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                if version == 1 {
                    transaction.execute_batch(&format!("{CREATE_FILE_IDENTITIES_SQL};"))?;
                }
                if version <= 2 {
                    transaction.execute_batch(&format!("{CREATE_RESOLVED_METAINFO_SQL};"))?;
                }
                transaction.execute_batch(&format!(
                    "{CREATE_FILE_SELECTION_SQL}; PRAGMA user_version = {SCHEMA_VERSION};"
                ))?;
                transaction.commit()?;
            }
            if !matches_schema(&connection, "file_identities", CREATE_FILE_IDENTITIES_SQL)? {
                return Err(StoreError::UnsupportedSchema);
            }
            if !matches_schema(
                &connection,
                "resolved_metainfo",
                CREATE_RESOLVED_METAINFO_SQL,
            )? {
                return Err(StoreError::UnsupportedSchema);
            }
            if !matches_schema(&connection, "file_selection", CREATE_FILE_SELECTION_SQL)? {
                return Err(StoreError::UnsupportedSchema);
            }
        } else {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(&format!(
                "{CREATE_TORRENTS_SQL}; {CREATE_FILE_IDENTITIES_SQL}; {CREATE_RESOLVED_METAINFO_SQL}; {CREATE_FILE_SELECTION_SQL}; PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = {SCHEMA_VERSION};"
            ))?;
            transaction.commit()?;
        }
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=WAL;",
        )?;
        Ok(Self {
            connection,
            _lock_file: ProcessLock(lock_file),
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
            "SELECT id,info_hash,source_kind,destination_root,desired FROM torrents WHERE tombstone=0 ORDER BY rowid",
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

    pub fn store_resolved_metainfo(&mut self, id: &str, metainfo: &[u8]) -> Result<(), StoreError> {
        if metainfo.is_empty() || metainfo.len() > MAX_SOURCE_BYTES as usize {
            return Err(StoreError::Input(InputError::TooLarge));
        }
        let parsed = inspect_torrent(metainfo)?;
        let record: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT info_hash,source_kind FROM torrents WHERE id=?1 AND tombstone=0",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (info_hash, kind) = record.ok_or(StoreError::NotFound)?;
        if kind != "magnet" || parsed.info_hash != info_hash {
            return Err(StoreError::Conflict);
        }
        self.connection.execute(
            "INSERT INTO resolved_metainfo(torrent_id,metainfo) VALUES (?1,?2)
             ON CONFLICT(torrent_id) DO UPDATE SET metainfo=excluded.metainfo",
            params![id, metainfo],
        )?;
        Ok(())
    }

    pub fn load_resolved_metainfo(&self, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let info_hash: String = self.connection.query_row(
            "SELECT info_hash FROM torrents WHERE id=?1 AND source_kind='magnet' AND tombstone=0",
            [id], |row| row.get(0),
        ).optional()?.ok_or(StoreError::NotFound)?;
        let length: Option<i64> = self
            .connection
            .query_row(
                "SELECT length(metainfo) FROM resolved_metainfo WHERE torrent_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(length) = length else {
            return Ok(None);
        };
        if !(1..=MAX_SOURCE_BYTES).contains(&length) {
            return Err(StoreError::Corrupt);
        }
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT metainfo FROM resolved_metainfo WHERE torrent_id=?1",
            [id],
            |row| row.get(0),
        )?;
        if inspect_torrent(&bytes)
            .map(|item| item.info_hash)
            .ok()
            .as_deref()
            != Some(info_hash.as_str())
        {
            return Err(StoreError::Corrupt);
        }
        Ok(Some(bytes))
    }

    pub fn validate_file_selection(
        &self,
        id: &str,
        indices: Option<&[usize]>,
    ) -> Result<(), StoreError> {
        let record: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT source_kind,desired FROM torrents WHERE id=?1 AND tombstone=0",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (kind, desired) = record.ok_or(StoreError::NotFound)?;
        if desired != "paused" {
            return Err(StoreError::Conflict);
        }
        if let Some(indices) = indices {
            let source = match kind.as_str() {
                "torrent" => self.load_source(id)?,
                "magnet" => self
                    .load_resolved_metainfo(id)?
                    .ok_or(StoreError::Conflict)?,
                _ => return Err(StoreError::Corrupt),
            };
            let file_count = inspect_torrent(&source)?.files.len();
            if !valid_file_selection(indices, file_count) {
                return Err(StoreError::Conflict);
            }
        }
        Ok(())
    }

    pub fn set_file_selection(
        &mut self,
        id: &str,
        indices: Option<&[usize]>,
    ) -> Result<(), StoreError> {
        self.validate_file_selection(id, indices)?;
        if let Some(indices) = indices {
            let json = serde_json::to_string(indices).map_err(|_| StoreError::Corrupt)?;
            self.connection.execute(
                "INSERT INTO file_selection(torrent_id,selection_json) VALUES (?1,?2)
                 ON CONFLICT(torrent_id) DO UPDATE SET selection_json=excluded.selection_json",
                params![id, json],
            )?;
        } else {
            self.connection
                .execute("DELETE FROM file_selection WHERE torrent_id=?1", [id])?;
        }
        Ok(())
    }

    pub fn load_file_selection(&self, id: &str) -> Result<Option<Vec<usize>>, StoreError> {
        let kind: String = self
            .connection
            .query_row(
                "SELECT source_kind FROM torrents WHERE id=?1 AND tombstone=0",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let length: Option<i64> = self
            .connection
            .query_row(
                "SELECT length(selection_json) FROM file_selection WHERE torrent_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(length) = length else {
            return Ok(None);
        };
        if !(3..=60_000).contains(&length) {
            return Err(StoreError::Corrupt);
        }
        let json: String = self.connection.query_row(
            "SELECT selection_json FROM file_selection WHERE torrent_id=?1",
            [id],
            |row| row.get(0),
        )?;
        let indices: Vec<usize> = serde_json::from_str(&json).map_err(|_| StoreError::Corrupt)?;
        let source = match kind.as_str() {
            "torrent" => self.load_source(id)?,
            "magnet" => self
                .load_resolved_metainfo(id)?
                .ok_or(StoreError::Corrupt)?,
            _ => return Err(StoreError::Corrupt),
        };
        let file_count = inspect_torrent(&source)
            .map_err(|_| StoreError::Corrupt)?
            .files
            .len();
        if !valid_file_selection(&indices, file_count) {
            return Err(StoreError::Corrupt);
        }
        Ok(Some(indices))
    }

    pub fn record_file_identity(
        &mut self,
        torrent_id: &str,
        file_index: u32,
        components: &[&str],
        length: u64,
        identity: FileIdentity,
    ) -> Result<(), StoreError> {
        validate_components(components)?;
        if file_index >= 10_000 || length > i64::MAX as u64 {
            return Err(StoreError::Input(InputError::TooLarge));
        }
        let path_json = serde_json::to_string(components).map_err(|_| StoreError::Corrupt)?;
        if path_json.len() > 32 * 1024 {
            return Err(StoreError::Input(InputError::TooLarge));
        }
        let encoded = identity.to_record_bytes();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active: Option<i64> = transaction
            .query_row(
                "SELECT tombstone FROM torrents WHERE id=?1",
                [torrent_id],
                |row| row.get(0),
            )
            .optional()?;
        if active != Some(0) {
            return Err(StoreError::NotFound);
        }
        let existing: Option<(String, i64, Vec<u8>)> = transaction
            .query_row(
                "SELECT path_json,length,identity FROM file_identities WHERE torrent_id=?1 AND file_index=?2",
                params![torrent_id, file_index],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((old_path, old_length, old_identity)) = existing {
            if old_path == path_json
                && old_length == length as i64
                && old_identity.as_slice() == encoded.as_slice()
            {
                return Ok(());
            }
            return Err(StoreError::Conflict);
        }
        transaction.execute(
            "INSERT INTO file_identities(torrent_id,file_index,path_json,length,identity) VALUES (?1,?2,?3,?4,?5)",
            params![torrent_id, file_index, path_json, length as i64, encoded.as_slice()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_file_identities(
        &self,
        torrent_id: &str,
    ) -> Result<Vec<StoredFileIdentity>, StoreError> {
        let active: Option<i64> = self
            .connection
            .query_row(
                "SELECT tombstone FROM torrents WHERE id=?1",
                [torrent_id],
                |row| row.get(0),
            )
            .optional()?;
        if active != Some(0) {
            return Err(StoreError::NotFound);
        }
        let mut statement = self.connection.prepare(
            "SELECT file_index,path_json,length,identity FROM file_identities WHERE torrent_id=?1 ORDER BY file_index"
        )?;
        let rows = statement.query_map([torrent_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut output = Vec::new();
        for row in rows {
            let (file_index, path_json, length, encoded) = row?;
            let file_index = u32::try_from(file_index).map_err(|_| StoreError::Corrupt)?;
            let length = u64::try_from(length).map_err(|_| StoreError::Corrupt)?;
            let path: Vec<String> =
                serde_json::from_str(&path_json).map_err(|_| StoreError::Corrupt)?;
            validate_components(&path.iter().map(String::as_str).collect::<Vec<_>>())
                .map_err(|_| StoreError::Corrupt)?;
            let identity = FileIdentity::from_record_bytes(&encoded).ok_or(StoreError::Corrupt)?;
            output.push(StoredFileIdentity {
                file_index,
                path,
                length,
                identity,
            });
            if output.len() > 10_000 {
                return Err(StoreError::Corrupt);
            }
        }
        Ok(output)
    }
}
