//! Synthetic loopback qualification of the persisted capability-scoped adapter.
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    storage::StorageFactoryExt,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
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
