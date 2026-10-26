//! Synthetic loopback qualification of the persisted capability-scoped adapter.
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, ManagedTorrentShared,
    Session, SessionOptions, TorrentMetadata,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
};
use std::{
    io::{self, ErrorKind},
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
    torrent_file::inspect_torrent,
};

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

#[derive(Clone)]
struct BudgetStorageFactory {
    base: DurableStorageFactory,
    remaining: Arc<Mutex<usize>>,
    failed: Arc<AtomicBool>,
}

impl StorageFactory for BudgetStorageFactory {
    type Storage = BudgetStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(BudgetStorage {
            inner: Box::new(self.base.create(shared, metadata)?),
            remaining: self.remaining.clone(),
            failed: self.failed.clone(),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

struct BudgetStorage {
    inner: Box<dyn TorrentStorage>,
    remaining: Arc<Mutex<usize>>,
    failed: Arc<AtomicBool>,
}

impl TorrentStorage for BudgetStorage {
    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.inner.init(shared, metadata)
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.inner.pread_exact(file_id, offset, buf)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let mut remaining = self.remaining.lock().unwrap();
        if buf.len() <= *remaining {
            self.inner.pwrite_all(file_id, offset, buf)?;
            *remaining -= buf.len();
            return Ok(());
        }
        let accepted = *remaining;
        *remaining = 0;
        if accepted > 0 {
            self.inner.pwrite_all(file_id, offset, &buf[..accepted])?;
        }
        self.failed.store(true, Ordering::SeqCst);
        Err(io::Error::from(ErrorKind::StorageFull).into())
    }

    fn remove_file(&self, file_id: usize, filename: &Path) -> anyhow::Result<()> {
        self.inner.remove_file(file_id, filename)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.remove_directory_if_empty(path)
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.inner.ensure_file_length(file_id, length)
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            inner: self.inner.take()?,
            remaining: self.remaining.clone(),
            failed: self.failed.clone(),
        }))
    }
}

#[tokio::test]
async fn partial_storage_full_write_is_rechecked_and_repaired_after_restart() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let content: Vec<u8> = (0..64 * 1024).map(|index| (index % 251) as u8).collect();
    let source_file = source.join("disk-full.bin");
    std::fs::write(&source_file, &content).unwrap();
    let seeder = Session::new_with_opts(source, isolated_options())
        .await
        .unwrap();
    let (torrent, _) = seeder
        .create_and_serve_torrent(
            &source_file,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let metainfo = torrent.as_bytes().unwrap();
    let peer = seeder.listen_addr().unwrap();
    let database = workspace.path().join("state.db");
    let torrent_id = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&metainfo), &destination, false)
        .unwrap()
        .id;
    let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
    let root = Arc::new(SafeRoot::open(&destination).unwrap());
    let failed = Arc::new(AtomicBool::new(false));
    let storage = BudgetStorageFactory {
        base: DurableStorageFactory::new(root, state.clone(), torrent_id.clone()),
        remaining: Arc::new(Mutex::new(1)),
        failed: failed.clone(),
    };
    let leecher = Session::new_with_opts(destination.clone(), isolated_options())
        .await
        .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(metainfo.clone()),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                overwrite: true,
                storage_factory: Some(storage.boxed()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !failed.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("injected disk-full write was not reached");
    assert!(!handle.stats().finished);
    leecher.stop().await;
    drop(handle);
    drop(leecher);
    drop(state);
    let partial = std::fs::read(destination.join("disk-full.bin")).unwrap();
    assert_ne!(partial, content);

    let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
    assert_eq!(
        state
            .lock()
            .unwrap()
            .load_file_identities(&torrent_id)
            .unwrap()
            .len(),
        1
    );
    let root = Arc::new(SafeRoot::open(&destination).unwrap());
    let leecher = Session::new_with_opts(destination.clone(), isolated_options())
        .await
        .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(metainfo),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                overwrite: true,
                storage_factory: Some(DurableStorageFactory::new(root, state, torrent_id).boxed()),
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
        std::fs::read(destination.join("disk-full.bin")).unwrap(),
        content
    );
    leecher.stop().await;
    seeder.stop().await;
}

#[tokio::test]
async fn selecting_one_file_downloads_only_its_pieces_and_keeps_other_files_absent() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let wanted = vec![7_u8; 32 * 1024];
    for (name, contents) in [
        ("first.bin", vec![1_u8; 32 * 1024]),
        ("wanted.bin", wanted.clone()),
        ("last.bin", vec![3_u8; 32 * 1024]),
    ] {
        std::fs::write(source.join(name), contents).unwrap();
    }
    let seeder = Session::new_with_opts(source.clone(), isolated_options())
        .await
        .unwrap();
    let (torrent, _) = seeder
        .create_and_serve_torrent(
            &source,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let metainfo = torrent.as_bytes().unwrap();
    let summary = inspect_torrent(&metainfo).unwrap();
    let wanted_index = summary
        .files
        .iter()
        .position(|file| file.path.last().is_some_and(|name| name == "wanted.bin"))
        .unwrap();
    let database = workspace.path().join("state.db");
    let torrent_id = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&metainfo), &destination, false)
        .unwrap()
        .id;
    let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
    let root = Arc::new(SafeRoot::open(&destination).unwrap());
    let leecher = Session::new_with_opts(destination.clone(), isolated_options())
        .await
        .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(metainfo),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![seeder.listen_addr().unwrap()]),
                only_files: Some(vec![wanted_index]),
                overwrite: true,
                storage_factory: Some(DurableStorageFactory::new(root, state, torrent_id).boxed()),
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
    for (index, file) in summary.files.iter().enumerate() {
        let path = file
            .path
            .iter()
            .fold(destination.clone(), |path, part| path.join(part));
        if index == wanted_index {
            assert_eq!(std::fs::read(path).unwrap(), wanted);
        } else {
            assert!(!path.exists(), "unselected file was created: {path:?}");
        }
    }
    leecher.stop().await;
    seeder.stop().await;
}

#[tokio::test]
async fn persisted_identity_allows_recheck_and_repair_after_restart() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let content: Vec<u8> = (0..64 * 1024).map(|index| (index % 251) as u8).collect();
    let source_file = source.join("synthetic.bin");
    std::fs::write(&source_file, &content).unwrap();
    let seeder = Session::new_with_opts(source, isolated_options())
        .await
        .unwrap();
    let (torrent, _) = seeder
        .create_and_serve_torrent(
            &source_file,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let metainfo = torrent.as_bytes().unwrap();
    let peer = seeder.listen_addr().unwrap();
    let database = workspace.path().join("state.db");
    let torrent_id = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&metainfo), &destination, false)
        .unwrap()
        .id;

    for round in 0..2 {
        let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
        let root = Arc::new(SafeRoot::open(&destination).unwrap());
        let leecher = Session::new_with_opts(destination.clone(), isolated_options())
            .await
            .unwrap();
        let handle = leecher
            .add_torrent(
                AddTorrent::from_bytes(metainfo.clone()),
                Some(AddTorrentOptions {
                    initial_peers: Some(vec![peer]),
                    peer_limit: Some(2),
                    overwrite: round != 0,
                    storage_factory: Some(
                        DurableStorageFactory::new(root, state.clone(), torrent_id.clone()).boxed(),
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
        drop(handle);
        drop(leecher);
        drop(state);
        if round == 0 {
            let mut damaged = std::fs::read(destination.join("synthetic.bin")).unwrap();
            damaged[17_000] ^= 0xff;
            std::fs::write(destination.join("synthetic.bin"), damaged).unwrap();
        }
    }
    assert_eq!(
        StateStore::open(&database)
            .unwrap()
            .load_file_identities(&torrent_id)
            .unwrap()
            .len(),
        1
    );
    seeder.stop().await;
}

#[tokio::test]
async fn replaced_file_is_rejected_before_engine_writes() {
    let workspace = tempfile::tempdir().unwrap();
    let root_path = workspace.path().join("download");
    std::fs::create_dir(&root_path).unwrap();
    let root = SafeRoot::open(&root_path).unwrap();
    let file = root.create_new(&["file.bin"]).unwrap();
    let identity = torrent_lite::safe_storage::FileIdentity::from_file(&file).unwrap();
    drop(file);
    let database = workspace.path().join("state.db");
    let mut store = StateStore::open(&database).unwrap();
    let record = store
        .register(
            AddInput::Magnet("magnet:?xt=urn:btih:0000000000000000000000000000000000000001"),
            &root_path,
            true,
        )
        .unwrap();
    store
        .record_file_identity(&record.id, 0, &["file.bin"], 4, identity)
        .unwrap();
    drop(store);
    std::fs::remove_file(root_path.join("file.bin")).unwrap();
    std::fs::write(root_path.join("file.bin"), b"evil").unwrap();
    let stored = StateStore::open(&database)
        .unwrap()
        .load_file_identities(&record.id)
        .unwrap();
    assert_ne!(
        torrent_lite::safe_storage::FileIdentity::from_file(&cap_std::fs::File::from_std(
            std::fs::File::open(root_path.join("file.bin")).unwrap()
        ))
        .unwrap(),
        stored[0].identity
    );
    assert!(
        root.open_existing(&["file.bin"], stored[0].identity)
            .is_err()
    );
}
