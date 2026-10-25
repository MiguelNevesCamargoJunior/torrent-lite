//! Qualification experiment: only synthetic files and loopback peers.
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    storage::{StorageFactory, StorageFactoryExt, TorrentStorage},
};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use torrent_lite::safe_storage::{FileIdentity, SafeRoot};

// Qualification adapter: new files only. It is intentionally unsuitable for
// production restart until existing-file identity is persisted and checked.
#[derive(Clone)]
struct NewFileFactory {
    root: Arc<SafeRoot>,
    identities: Option<Vec<FileIdentity>>,
}

impl StorageFactory for NewFileFactory {
    type Storage = NewFileStorage;

    fn create(
        &self,
        _: &librqbit::ManagedTorrentShared,
        _: &librqbit::TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(NewFileStorage {
            root: self.root.clone(),
            identities: self.identities.clone(),
            files: Vec::new(),
        })
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

struct NewFileStorage {
    root: Arc<SafeRoot>,
    identities: Option<Vec<FileIdentity>>,
    files: Vec<Mutex<cap_std::fs::File>>,
}

impl TorrentStorage for NewFileStorage {
    fn init(
        &mut self,
        _: &librqbit::ManagedTorrentShared,
        metadata: &librqbit::TorrentMetadata,
    ) -> anyhow::Result<()> {
        for (index, info) in metadata.file_infos.iter().enumerate() {
            let components: Vec<&str> = info
                .relative_filename
                .components()
                .map(|part| {
                    part.as_os_str()
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("non-UTF8 file path"))
                })
                .collect::<anyhow::Result<_>>()?;
            let file = match &self.identities {
                Some(identities) => self.root.open_existing(
                    &components,
                    *identities
                        .get(index)
                        .ok_or_else(|| anyhow::anyhow!("missing file identity"))?,
                )?,
                None => self.root.create_new(&components)?,
            };
            self.files.push(Mutex::new(file));
        }
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let mut file = self.files[file_id].lock().unwrap();
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)?;
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let mut file = self.files[file_id].lock().unwrap();
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
        self.files[file_id].lock().unwrap().set_len(length)?;
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let files = self
            .files
            .iter()
            .map(|file| Ok(Mutex::new(file.lock().unwrap().try_clone()?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Box::new(Self {
            root: self.root.clone(),
            identities: self.identities.clone(),
            files,
        }))
    }
}

fn isolated_options() -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        fastresume: false,
        persistence: None,
        listen: Some(ListenerOptions {
            listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            ipv4_only: true,
            ..Default::default()
        }),
        peer_limit: Some(2),
        ..Default::default()
    }
}

#[tokio::test]
async fn transfers_a_synthetic_file_between_local_sessions() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    let content: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    let source_file = source.join("synthetic.bin");
    std::fs::write(&source_file, &content).unwrap();

    let seeder = Session::new_with_opts(source, isolated_options())
        .await
        .unwrap();
    let (torrent, _handle) = seeder
        .create_and_serve_torrent(
            &source_file,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let peer = seeder.listen_addr().unwrap();
    assert!(peer.ip().is_loopback());

    let leecher = Session::new_with_opts(destination.clone(), isolated_options())
        .await
        .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(torrent.as_bytes().unwrap()),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                peer_limit: Some(2),
                storage_factory: Some(
                    NewFileFactory {
                        root: Arc::new(SafeRoot::open(&destination).unwrap()),
                        identities: None,
                    }
                    .boxed(),
                ),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), handle.wait_until_completed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(destination.join("synthetic.bin")).unwrap(),
        content
    );
    leecher.stop().await;
    let downloaded = destination.join("synthetic.bin");
    let stored_identity = FileIdentity::from_file(&cap_std::fs::File::from_std(
        std::fs::File::open(&downloaded).unwrap(),
    ))
    .unwrap();
    let mut damaged = std::fs::read(&downloaded).unwrap();
    damaged[17_000] ^= 0xff;
    std::fs::write(&downloaded, damaged).unwrap();

    let restarted = Session::new_with_opts(destination.clone(), isolated_options())
        .await
        .unwrap();
    let handle = restarted
        .add_torrent(
            AddTorrent::from_bytes(torrent.as_bytes().unwrap()),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                peer_limit: Some(2),
                overwrite: true,
                storage_factory: Some(
                    NewFileFactory {
                        root: Arc::new(SafeRoot::open(&destination).unwrap()),
                        identities: Some(vec![stored_identity]),
                    }
                    .boxed(),
                ),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), handle.wait_until_completed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&downloaded).unwrap(), content);
    restarted.stop().await;
    seeder.stop().await;
}
