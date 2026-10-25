//! Qualifies the combined storage and authenticated outbound TCP proxy on loopback.
use librqbit::{
    AddTorrent, AddTorrentOptions, ConnectionOptions, CreateTorrentOptions, ListenerOptions,
    Session, SessionOptions, storage::StorageFactoryExt,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    network_input::sanitize_torrent_with_policy,
    network_policy::DestinationPolicy,
    policy_proxy::PolicyProxy,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
};

fn isolated_options(proxy: Option<String>, enable_trackers: bool) -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: !enable_trackers,
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
    let seeder = Session::new_with_opts(source, isolated_options(None, false))
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
        isolated_options(Some(proxy.proxy_url()), false),
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

#[tokio::test]
async fn redirected_http_tracker_and_peer_both_use_the_proxy() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let content: Vec<u8> = (0..32 * 1024).map(|index| (index % 251) as u8).collect();
    let source_file = source.join("synthetic.bin");
    std::fs::write(&source_file, &content).unwrap();
    let seeder = Session::new_with_opts(source, isolated_options(None, false))
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

    let tracker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let tracker_origin = format!("http://{}", tracker.local_addr().unwrap());
    let tracker_url = format!("{tracker_origin}/redirect");
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = requests.clone();
    let tracker_task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = tracker.accept().await.unwrap();
            let mut request = [0_u8; 8192];
            let size = stream.read(&mut request).await.unwrap();
            request_count.fetch_add(1, Ordering::Relaxed);
            if request[..size].starts_with(b"GET /redirect") {
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {tracker_origin}/announce\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                continue;
            }
            let mut body = b"d8:intervali30e5:peers6:".to_vec();
            body.extend(Ipv4Addr::LOCALHOST.octets());
            body.extend(peer.port().to_be_bytes());
            body.push(b'e');
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        }
    });

    let original = torrent.as_bytes().unwrap();
    let parsed = librqbit_core::torrent_metainfo::torrent_from_bytes(&original).unwrap();
    let mut with_tracker = b"d8:announce".to_vec();
    with_tracker.extend(format!("{}:{tracker_url}", tracker_url.len()).as_bytes());
    with_tracker.extend(b"4:info");
    with_tracker.extend(parsed.info.raw_bytes.as_ref());
    with_tracker.push(b'e');
    let sanitized =
        sanitize_torrent_with_policy(&with_tracker, DestinationPolicy::AllowLan).unwrap();
    assert_eq!(sanitized.dropped_trackers, 0);
    let database = workspace.path().join("state.db");
    let torrent_id = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&with_tracker), &destination, false)
        .unwrap()
        .id;
    let state = Arc::new(Mutex::new(StateStore::open(&database).unwrap()));
    let proxy = PolicyProxy::bind(DestinationPolicy::AllowLan)
        .await
        .unwrap();
    let leecher = Session::new_with_opts(
        destination.clone(),
        isolated_options(Some(proxy.proxy_url()), true),
    )
    .await
    .unwrap();
    let handle = leecher
        .add_torrent(
            AddTorrent::from_bytes(sanitized.bytes),
            Some(AddTorrentOptions {
                peer_limit: Some(2),
                storage_factory: Some(
                    DurableStorageFactory::new(
                        Arc::new(SafeRoot::open(&destination).unwrap()),
                        state,
                        torrent_id,
                    )
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
    assert!(requests.load(Ordering::Relaxed) >= 2);
    assert!(proxy.successful_connections() >= 3);
    leecher.stop().await;
    seeder.stop().await;
    tracker_task.abort();
}
