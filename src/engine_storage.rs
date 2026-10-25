//! Capability-scoped storage for a qualified engine session.
//! The owning service must keep the state store alive for the whole session.

use crate::{
    path_policy::validate_file_set,
    safe_storage::{FileIdentity, SafeRoot},
    state_store::{StateStore, StoredFileIdentity},
};
use librqbit::{
    ManagedTorrentShared, TorrentMetadata,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct DurableStorageFactory {
    root: Arc<SafeRoot>,
    state: Arc<Mutex<StateStore>>,
    torrent_id: String,
}

impl DurableStorageFactory {
    pub fn new(root: Arc<SafeRoot>, state: Arc<Mutex<StateStore>>, torrent_id: String) -> Self {
        Self {
            root,
            state,
            torrent_id,
        }
    }
}

impl StorageFactory for DurableStorageFactory {
    type Storage = DurableStorage;

    fn create(
        &self,
        _: &ManagedTorrentShared,
        _: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(DurableStorage {
            root: self.root.clone(),
            state: self.state.clone(),
            torrent_id: self.torrent_id.clone(),
            files: Vec::new(),
            paths: Vec::new(),
            lengths: Vec::new(),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

pub struct DurableStorage {
    root: Arc<SafeRoot>,
    state: Arc<Mutex<StateStore>>,
    torrent_id: String,
    files: Vec<Mutex<Option<cap_std::fs::File>>>,
    paths: Vec<Vec<String>>,
    lengths: Vec<u64>,
}

impl DurableStorage {
    fn open_file(
        &self,
        index: usize,
        path: &[String],
        length: u64,
        recorded: Option<&StoredFileIdentity>,
    ) -> anyhow::Result<cap_std::fs::File> {
        let components: Vec<&str> = path.iter().map(String::as_str).collect();
        if let Some(recorded) = recorded {
            anyhow::ensure!(
                recorded.path == path && recorded.length == length,
                "stored file metadata differs from torrent"
            );
            return Ok(self.root.open_existing(&components, recorded.identity)?);
        }

        let file = self.root.create_new(&components)?;
        let identity = FileIdentity::from_file(&file)?;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .record_file_identity(
                &self.torrent_id,
                index as u32,
                &components,
                length,
                identity,
            )?;
        Ok(file)
    }

    fn file(&self, file_id: usize) -> anyhow::Result<&Mutex<Option<cap_std::fs::File>>> {
        self.files
            .get(file_id)
            .ok_or_else(|| anyhow::anyhow!("invalid file index"))
    }

    fn open_for_write<'a>(
        &self,
        file_id: usize,
        slot: &'a mut Option<cap_std::fs::File>,
    ) -> anyhow::Result<&'a mut cap_std::fs::File> {
        if slot.is_none() {
            let path = self
                .paths
                .get(file_id)
                .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;
            let length = *self
                .lengths
                .get(file_id)
                .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;
            *slot = Some(self.open_file(file_id, path, length, None)?);
        }
        slot.as_mut()
            .ok_or_else(|| anyhow::anyhow!("file unavailable"))
    }

    fn check_range(&self, file_id: usize, offset: u64, count: usize) -> anyhow::Result<()> {
        let expected = self
            .lengths
            .get(file_id)
            .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;
        let end = offset
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("file offset overflow"))?;
        anyhow::ensure!(end <= *expected, "file access exceeds declared length");
        Ok(())
    }
}

impl TorrentStorage for DurableStorage {
    fn init(&mut self, _: &ManagedTorrentShared, metadata: &TorrentMetadata) -> anyhow::Result<()> {
        anyhow::ensure!(self.files.is_empty(), "storage already initialized");
        anyhow::ensure!(
            !metadata.info.info().private,
            "private torrents are not yet supported"
        );
        anyhow::ensure!(metadata.file_infos.len() <= 10_000, "too many files");
        let paths = metadata
            .file_infos
            .iter()
            .map(|info| {
                info.relative_filename
                    .components()
                    .map(|part| {
                        part.as_os_str()
                            .to_str()
                            .map(str::to_owned)
                            .ok_or_else(|| anyhow::anyhow!("non-UTF8 file path"))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        validate_file_set(&paths)?;

        let stored = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_file_identities(&self.torrent_id)?;
        let mut identities = vec![None; paths.len()];
        for record in &stored {
            let index = record.file_index as usize;
            let slot = identities
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("unexpected stored file index"))?;
            anyhow::ensure!(slot.is_none(), "duplicate stored file index");
            anyhow::ensure!(
                record.path == paths[index] && record.length == metadata.file_infos[index].len,
                "stored file metadata differs from torrent"
            );
            *slot = Some(record);
        }
        for (index, (info, path)) in metadata.file_infos.iter().zip(&paths).enumerate() {
            let file = identities[index]
                .map(|record| self.open_file(index, path, info.len, Some(record)))
                .transpose()?;
            self.files.push(Mutex::new(file));
            self.paths.push(path.clone());
            self.lengths.push(info.len);
        }
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.check_range(file_id, offset, buf.len())?;
        let mut file = self
            .file(file_id)?
            .lock()
            .map_err(|_| anyhow::anyhow!("file lock poisoned"))?;
        let file = file
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("file has not been created"))?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)?;
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        self.check_range(file_id, offset, buf.len())?;
        // The engine can issue an empty write at an exact file boundary.
        // It must not turn an unselected file into a real one.
        if buf.is_empty() {
            return Ok(());
        }
        let mut file = self
            .file(file_id)?
            .lock()
            .map_err(|_| anyhow::anyhow!("file lock poisoned"))?;
        let file = self.open_for_write(file_id, &mut file)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(buf)?;
        Ok(())
    }

    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }
    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.lengths.get(file_id) == Some(&length),
            "unexpected file length"
        );
        let mut file = self
            .file(file_id)?
            .lock()
            .map_err(|_| anyhow::anyhow!("file lock poisoned"))?;
        self.open_for_write(file_id, &mut file)?.set_len(length)?;
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let files = self
            .files
            .iter()
            .map(|file| {
                Ok(Mutex::new(
                    file.lock()
                        .map_err(|_| anyhow::anyhow!("file lock poisoned"))?
                        .as_ref()
                        .map(cap_std::fs::File::try_clone)
                        .transpose()?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Box::new(Self {
            root: self.root.clone(),
            state: self.state.clone(),
            torrent_id: self.torrent_id.clone(),
            files,
            paths: self.paths.clone(),
            lengths: self.lengths.clone(),
        }))
    }
}
