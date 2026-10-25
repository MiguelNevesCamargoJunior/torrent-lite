//! Qualifies the combined storage and authenticated outbound TCP proxy on loopback.
use librqbit::{
    AddTorrent, AddTorrentOptions, ConnectionOptions, CreateTorrentOptions, ListenerOptions,
    Session, SessionOptions, storage::StorageFactoryExt,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    network_policy::DestinationPolicy,
    policy_proxy::PolicyProxy,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
};

fn isolated_options(proxy: Option<String>) -> SessionOptions {
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
        connect: proxy.map(|proxy_url| ConnectionOptions {
            proxy_url: Some(proxy_url),
            enable_tcp: false,
            ..Default::default()
        }),
        peer_limit: Some(2),
        ..Default::default()
    }
}

#[tokio::test]
async fn engine_peer_uses_authenticated_proxy_with_persisted_storage() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let content: Vec<u8> = (0..64 * 1024).map(|index| (index % 251) as u8).collect();
    let source_file = source.join("synthetic.bin");
    std::fs::write(&source_file, &content).unwrap();
    let seeder = Session::new_with_opts(source, isolated_options(None))
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
    let peer = seeder.listen_addr().unwrap();
    let metainfo = torrent.as_bytes().unwrap();
    let database = workspace.path().join("state.db");
    let torrent_id = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&metainfo), &destination, false)
        .unwrap()
        .id;
    let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
    let root = Arc::new(SafeRoot::open(&destination).unwrap());
    let proxy = PolicyProxy::bind(DestinationPolicy::AllowLan)
        .await
        .unwrap();
    let leecher = Session::new_with_opts(
        destination.clone(),
        isolated_options(Some(proxy.proxy_url())),
    )
    .await
    .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(metainfo),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                peer_limit: Some(2),
                storage_factory: Some(
                    DurableStorageFactory::new(root, state.clone(), torrent_id).boxed(),
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
    assert!(proxy.successful_connections() > 0);
    leecher.stop().await;
    seeder.stop().await;
}
