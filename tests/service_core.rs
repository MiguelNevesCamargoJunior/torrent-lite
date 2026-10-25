use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use torrent_lite::{
    network_policy::DestinationPolicy,
    service_core::{ServiceCore, ServiceSettings},
    state_store::DesiredState,
};

#[test]
fn bandwidth_settings_reject_values_that_the_engine_cannot_apply() {
    assert!(ServiceSettings::new(0, None, None).is_err());
    assert!(ServiceSettings::new(1, Some(0), None).is_err());
    assert!(ServiceSettings::new(1, Some(15), None).is_err());
    assert!(ServiceSettings::new(1, None, Some(15)).is_err());
    assert!(ServiceSettings::new(1, Some(4_194_304), None).is_err());
    assert_eq!(
        ServiceSettings::new(1, Some(4_194_303), None)
            .unwrap()
            .download_limit_bps(),
        Some(4_294_966_272)
    );
    let limits = ServiceSettings::new(2, Some(64), Some(128)).unwrap();
    assert_eq!(limits.download_limit_bps(), Some(65_536));
    assert_eq!(limits.upload_limit_bps(), Some(131_072));
}

#[tokio::test]
async fn service_applies_global_bandwidth_limits_to_the_engine() {
    let workspace = tempfile::tempdir().unwrap();
    let download = workspace.path().join("download");
    std::fs::create_dir(&download).unwrap();
    let settings = ServiceSettings::new(1, Some(64), Some(128)).unwrap();
    let service = ServiceCore::open_with_settings(
        &workspace.path().join("state.db"),
        &download,
        DestinationPolicy::AllowLan,
        settings,
    )
    .await
    .unwrap();
    assert_eq!(
        service.bandwidth_limits_bps(),
        (Some(65_536), Some(131_072))
    );
    service.stop().await;
}

#[tokio::test]
async fn queue_limits_active_downloads_and_survives_restart() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let download = workspace.path().join("download");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&download).unwrap();
    let seeder = Session::new_with_opts(
        source.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                ipv4_only: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut sources = Vec::new();
    for index in 0..3 {
        let path = source.join(format!("sample-{index}.bin"));
        std::fs::write(&path, vec![index as u8 + 1; 32 * 1024]).unwrap();
        let (torrent, _) = seeder
            .create_and_serve_torrent(
                &path,
                CreateTorrentOptions {
                    piece_length: Some(16 * 1024),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        sources.push(torrent.as_bytes().unwrap());
    }
    let database = workspace.path().join("state.db");
    let service =
        ServiceCore::open_with_limit(&database, &download, DestinationPolicy::AllowLan, 1)
            .await
            .unwrap();
    let first = service.add_torrent(&sources[0], false).await.unwrap();
    let second = service.add_torrent(&sources[1], false).await.unwrap();
    let third = service.add_torrent(&sources[2], false).await.unwrap();
    assert!(service.metrics(&first.id).await.is_some());
    assert!(service.metrics(&second.id).await.is_none());
    assert!(service.metrics(&third.id).await.is_none());
    service.pause(&first.id).await.unwrap();
    assert!(service.metrics(&second.id).await.is_some());
    assert!(service.metrics(&third.id).await.is_none());
    service.stop().await;
    drop(service);

    let reopened =
        ServiceCore::open_with_limit(&database, &download, DestinationPolicy::AllowLan, 1)
            .await
            .unwrap();
    assert_eq!(
        reopened
            .list()
            .unwrap()
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str(), third.id.as_str()]
    );
    assert!(reopened.metrics(&first.id).await.is_none());
    assert!(reopened.metrics(&second.id).await.is_some());
    assert!(reopened.metrics(&third.id).await.is_none());
    reopened.stop().await;
    seeder.stop().await;
}

#[tokio::test]
async fn completed_download_releases_queue_slot_without_client_mutation() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let download = workspace.path().join("download");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&download).unwrap();
    let seeder = Session::new_with_opts(
        source.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                ipv4_only: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let tracker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let tracker_url = format!("http://{}/announce", tracker.local_addr().unwrap());
    let peer = seeder.listen_addr().unwrap();
    let (release_tracker, tracker_ready) = tokio::sync::watch::channel(false);
    let tracker_task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = tracker.accept().await.unwrap();
            let mut ready = tracker_ready.clone();
            tokio::spawn(async move {
                while !*ready.borrow() {
                    ready.changed().await.unwrap();
                }
                let mut request = [0_u8; 8192];
                let _ = socket.read(&mut request).await.unwrap();
                let mut body = b"d8:intervali1e5:peers6:".to_vec();
                body.extend(Ipv4Addr::LOCALHOST.octets());
                body.extend(peer.port().to_be_bytes());
                body.push(b'e');
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            });
        }
    });
    let mut sources = Vec::new();
    for index in 0..2 {
        let path = source.join(format!("queued-{index}.bin"));
        let size = if index == 0 { 128 * 1024 } else { 32 * 1024 };
        std::fs::write(&path, vec![index as u8 + 1; size]).unwrap();
        let (torrent, _) = seeder
            .create_and_serve_torrent(
                &path,
                CreateTorrentOptions {
                    piece_length: Some(16 * 1024),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let raw = torrent.as_bytes().unwrap();
        let parsed = librqbit_core::torrent_metainfo::torrent_from_bytes(&raw).unwrap();
        let mut bytes = b"d8:announce".to_vec();
        bytes.extend(format!("{}:{tracker_url}", tracker_url.len()).as_bytes());
        bytes.extend(b"4:info");
        bytes.extend(parsed.info.raw_bytes.as_ref());
        bytes.push(b'e');
        sources.push(bytes);
    }
    let database = workspace.path().join("state.db");
    let service = ServiceCore::open_with_settings(
        &database,
        &download,
        DestinationPolicy::AllowLan,
        ServiceSettings::new(1, Some(64), None).unwrap(),
    )
    .await
    .unwrap();
    let first = service.add_torrent(&sources[0], false).await.unwrap();
    let second = service.add_torrent(&sources[1], false).await.unwrap();
    assert!(service.metrics(&first.id).await.is_some());
    assert!(service.metrics(&second.id).await.is_none());
    let started = Instant::now();
    release_tracker.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(25), service.wait_completed(&first.id))
        .await
        .unwrap()
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(500));
    service.tick_queue().await.unwrap();
    assert!(service.metrics(&second.id).await.is_some());
    tokio::time::timeout(Duration::from_secs(25), service.wait_completed(&second.id))
        .await
        .unwrap()
        .unwrap();
    service.stop().await;
    tracker_task.abort();
    seeder.stop().await;
}

#[tokio::test]
async fn paused_torrent_survives_reopen_and_resumes_with_verified_data() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("source");
    let download = workspace.path().join("download");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&download).unwrap();
    let content: Vec<u8> = (0..32 * 1024).map(|i| (i % 251) as u8).collect();
    let source_file = source.join("sample.bin");
    std::fs::write(&source_file, &content).unwrap();
    let seeder = Session::new_with_opts(
        source,
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                ipv4_only: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
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
    let tracker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let tracker_url = format!("http://{}/announce", tracker.local_addr().unwrap());
    let peer = seeder.listen_addr().unwrap();
    let tracker_task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = tracker.accept().await.unwrap();
            let mut request = [0_u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let mut body = b"d8:intervali1e5:peers6:".to_vec();
            body.extend(Ipv4Addr::LOCALHOST.octets());
            body.extend(peer.port().to_be_bytes());
            body.push(b'e');
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        }
    });
    let raw = torrent.as_bytes().unwrap();
    let parsed = librqbit_core::torrent_metainfo::torrent_from_bytes(&raw).unwrap();
    let mut bytes = b"d8:announce".to_vec();
    bytes.extend(format!("{}:{tracker_url}", tracker_url.len()).as_bytes());
    bytes.extend(b"4:info");
    bytes.extend(parsed.info.raw_bytes.as_ref());
    bytes.push(b'e');
    let database = workspace.path().join("state.db");

    let service = ServiceCore::open(&database, &download, DestinationPolicy::AllowLan)
        .await
        .unwrap();
    let item = service.add_torrent(&bytes, true).await.unwrap();
    assert_eq!(item.desired, DesiredState::Paused);
    assert_eq!(service.list().unwrap().len(), 1);
    service.stop().await;
    drop(service);

    let service = ServiceCore::open(&database, &download, DestinationPolicy::AllowLan)
        .await
        .unwrap();
    assert_eq!(service.list().unwrap()[0].desired, DesiredState::Paused);
    service.resume(&item.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(25), service.wait_completed(&item.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(download.join(&item.id).join("sample.bin")).unwrap(),
        content
    );
    let (completed, total, finished) = service.progress(&item.id).await.unwrap();
    assert_eq!(completed, content.len() as u64);
    assert_eq!(total, content.len() as u64);
    assert!(finished);
    let metrics = service.metrics(&item.id).await.unwrap();
    assert_eq!(metrics.progress_bytes, content.len() as u64);
    assert_eq!(metrics.total_bytes, content.len() as u64);
    assert!(metrics.finished);
    assert!(metrics.observed_state == "live" || metrics.observed_state == "paused");
    assert!(service.proxy_connections() > 0);
    service.pause(&item.id).await.unwrap();
    assert_eq!(service.list().unwrap()[0].desired, DesiredState::Paused);
    let downloaded = download.join(&item.id).join("sample.bin");
    let mut corrupted = content.clone();
    corrupted[0] ^= 0xff;
    std::fs::write(&downloaded, corrupted).unwrap();
    service.verify(&item.id).await.unwrap();
    assert_eq!(service.list().unwrap()[0].desired, DesiredState::Paused);
    assert_eq!(
        service.metrics(&item.id).await.unwrap().observed_state,
        "paused"
    );
    let (checked, total, finished) = service.progress(&item.id).await.unwrap();
    assert!(checked < total);
    assert!(!finished);
    service.resume(&item.id).await.unwrap();
    let repaired =
        tokio::time::timeout(Duration::from_secs(20), service.wait_completed(&item.id)).await;
    assert!(
        repaired.is_ok(),
        "verification repair stalled: {:?}, proxy connections: {}",
        service.metrics(&item.id).await,
        service.proxy_connections()
    );
    repaired.unwrap().unwrap();
    assert_eq!(std::fs::read(&downloaded).unwrap(), content);
    service.remove(&item.id).await.unwrap();
    assert!(service.list().unwrap().is_empty());
    assert_eq!(
        std::fs::read(download.join(&item.id).join("sample.bin")).unwrap(),
        content
    );
    service.stop().await;
    drop(service);
    let service = ServiceCore::open(&database, &download, DestinationPolicy::AllowLan)
        .await
        .unwrap();
    assert!(service.list().unwrap().is_empty());
    service.stop().await;
    drop(service);

    let magnet_download = workspace.path().join("magnet-download");
    std::fs::create_dir(&magnet_download).unwrap();
    let magnet_database = workspace.path().join("magnet-state.db");
    let magnet = format!("magnet:?xt=urn:btih:{}&tr={tracker_url}", item.info_hash);
    let service = ServiceCore::open(
        &magnet_database,
        &magnet_download,
        DestinationPolicy::AllowLan,
    )
    .await
    .unwrap();
    let magnet_item = service.add_magnet(&magnet).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(25),
        service.wait_completed(&magnet_item.id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        std::fs::read(magnet_download.join(&magnet_item.id).join("sample.bin")).unwrap(),
        content
    );
    service.stop().await;
    drop(service);
    tracker_task.abort();
    let service = ServiceCore::open(
        &magnet_database,
        &magnet_download,
        DestinationPolicy::AllowLan,
    )
    .await
    .unwrap();
    assert_eq!(service.list().unwrap().len(), 1);
    service.verify(&magnet_item.id).await.unwrap();
    service.stop().await;
    seeder.stop().await;
}
