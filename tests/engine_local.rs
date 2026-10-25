//! Qualification experiment: only synthetic files and loopback peers.
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

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
