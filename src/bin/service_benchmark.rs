//! End-to-end synthetic benchmark for the authenticated local service path.

#[path = "../../examples/common/benchmark_support.rs"]
mod benchmark_support;

use anyhow::{Context, Result, bail, ensure};
use benchmark_support::{Ready, SeederChild, same_contents, seed};
use librqbit_core::torrent_metainfo::torrent_from_bytes;
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use torrent_lite::{
    api_client::LocalClient,
    local_api::LocalApi,
    network_policy::DestinationPolicy,
    service_core::{ServiceCore, ServiceSettings},
    service_discovery::EndpointLease,
};

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(180);
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("service benchmark failed: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--seed") {
        ensure!(args.len() == 6, "invalid seeder arguments");
        return seed(
            Path::new(&args[2]),
            Path::new(&args[3]),
            Path::new(&args[4]),
            args[5].parse()?,
        )
        .await;
    }
    ensure!(
        args.len() == 3 && args[1] == "--size-mib",
        "usage: service_benchmark --size-mib N"
    );
    measure(args[2].parse()?).await
}

async fn measure(size_mib: u64) -> Result<()> {
    ensure!((8..=256).contains(&size_mib), "size must be 8..=256 MiB");
    let workspace = tempfile::tempdir()?;
    let source = workspace.path().join("source");
    let destination = workspace.path().join("destination");
    std::fs::create_dir(&source)?;
    std::fs::create_dir(&destination)?;
    let source_file = source.join("payload.bin");
    let size = size_mib * 1024 * 1024;
    let ready_path = workspace.path().join("seed-ready.json");
    let stop_path = workspace.path().join("seed-stop");
    let child = Command::new(std::env::current_exe()?)
        .arg("--seed")
        .arg(&source_file)
        .arg(&ready_path)
        .arg(&stop_path)
        .arg(size.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut seeder = SeederChild(child);
    let ready = wait_for_seeder(&mut seeder, &ready_path).await?;
    let peer: SocketAddr = ready.peer.parse()?;
    ensure!(peer.ip().is_loopback(), "seeder was not on loopback");
    let metainfo = data_encoding::BASE64.decode(ready.metainfo_base64.as_bytes())?;

    let tracker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let tracker_url = format!("http://{}/announce", tracker.local_addr()?);
    let metainfo = add_tracker(&metainfo, &tracker_url)?;
    let tracker_requests = Arc::new(AtomicU64::new(0));
    let (announce_seen_tx, announce_seen_rx) = oneshot::channel();
    let (release_announce_tx, release_announce_rx) = oneshot::channel();
    let tracker_task = tokio::spawn(run_tracker(
        tracker,
        peer,
        Arc::clone(&tracker_requests),
        announce_seen_tx,
        release_announce_rx,
    ));

    let database = workspace.path().join("state.db");
    let settings = ServiceSettings::new(1, None, None)?;
    // AllowLan is limited to this benchmark's loopback tracker and seeder fixtures.
    let service = Arc::new(
        ServiceCore::open_with_settings(
            &database,
            &destination,
            DestinationPolicy::AllowLan,
            settings,
        )
        .await?,
    );
    let api = LocalApi::bind(Arc::clone(&service)).await?;
    let mut shutdown = api.shutdown_receiver();
    let lease = EndpointLease::publish(&database, &api)?;
    let shutdown_service = Arc::clone(&service);
    let service_stop_task = tokio::spawn(async move {
        while !*shutdown.borrow_and_update() {
            if shutdown.changed().await.is_err() {
                break;
            }
        }
        api.close().await;
        shutdown_service.stop().await;
        drop(lease);
    });
    let client = Arc::new(LocalClient::discover(&database)?);

    let started = Instant::now();
    let add_client = Arc::clone(&client);
    let torrent_for_add = metainfo.clone();
    let add_task =
        tokio::spawn(async move { add_client.add_torrent(&torrent_for_add, false).await });

    tokio::time::timeout(SERVICE_START_TIMEOUT, announce_seen_rx)
        .await
        .context("service did not announce to the local tracker")??;
    let proxy_tracker_connections = service.proxy_connections();
    ensure!(
        proxy_tracker_connections >= 1,
        "tracker connection did not pass through the policy proxy"
    );
    release_announce_tx
        .send(())
        .map_err(|_| anyhow::anyhow!("tracker stopped before releasing its peer"))?;

    let added = tokio::time::timeout(Duration::from_secs(45), add_task)
        .await
        .context("authenticated API add request timed out")?
        .context("authenticated API add task failed")??;

    let deadline = Instant::now() + TRANSFER_TIMEOUT;
    let proxy_peer_connections = loop {
        let rows = client
            .list()
            .await
            .context("list through local API failed")?;
        let row = rows
            .iter()
            .find(|row| row.id == added.id)
            .context("API did not return the added torrent")?;
        if let Some(code) = &row.error_code {
            bail!("service reported torrent error: {code}");
        }
        let connected_peers = row.peers.unwrap_or_default();
        let connections = service.proxy_connections();
        if connected_peers > 0 && connections > proxy_tracker_connections {
            break connections - proxy_tracker_connections;
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for a peer connection through the proxy");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    loop {
        let rows = client
            .list()
            .await
            .context("list through local API failed")?;
        let row = rows
            .iter()
            .find(|row| row.id == added.id)
            .context("API lost the added torrent")?;
        if let Some(code) = &row.error_code {
            bail!("service reported torrent error: {code}");
        }
        if row.finished == Some(true) {
            break;
        }
        if Instant::now() >= deadline {
            bail!("download did not complete within 180 seconds");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let transfer_seconds = started.elapsed().as_secs_f64();
    ensure!(transfer_seconds > 0.0, "transfer timer did not advance");
    let downloaded_file = destination.join(&added.id).join("payload.bin");
    ensure!(
        same_contents(&source_file, &downloaded_file)?,
        "downloaded bytes differ from the deterministic source"
    );
    ensure!(
        tracker_requests.load(Ordering::Relaxed) >= 1,
        "local tracker did not serve an announce"
    );

    client.stop_service().await?;
    tokio::time::timeout(Duration::from_secs(20), service_stop_task)
        .await
        .context("service did not stop after the authenticated request")?
        .context("service shutdown task failed")?;
    drop(client);
    drop(service);

    tracker_task.abort();
    let _ = tracker_task.await;
    stop_seeder(&mut seeder, &stop_path).await?;

    let restart_started = Instant::now();
    let reopened = ServiceCore::open_with_settings(
        &database,
        &destination,
        DestinationPolicy::AllowLan,
        settings,
    )
    .await
    .context("reopen service for full piece validation failed")?;
    tokio::time::timeout(TRANSFER_TIMEOUT, reopened.wait_completed(&added.id))
        .await
        .context("restart validation timed out")??;
    let restart_validation_seconds = restart_started.elapsed().as_secs_f64();
    ensure!(
        same_contents(&source_file, &downloaded_file)?,
        "bytes differed after restart validation"
    );
    reopened.stop().await;
    drop(reopened);

    let report = serde_json::json!({
        "schema_version": 1,
        "payload_bytes": size,
        "transfer_seconds": transfer_seconds,
        "transfer_mib_per_sec": size_mib as f64 / transfer_seconds,
        "transport": "tcp_loopback_through_policy_proxy",
        "storage": "durable_sqlite_full_piece_recheck",
        "tracker_announces": tracker_requests.load(Ordering::Relaxed),
        "proxy_tracker_connections": proxy_tracker_connections,
        "proxy_peer_connections": proxy_peer_connections,
        "download_verified": true,
        "restart_validation_seconds": restart_validation_seconds,
        "restart_validation_verified": true,
        "restart_without_tracker_or_seeder": true
    });
    println!("{report}");
    Ok(())
}

async fn wait_for_seeder(seeder: &mut SeederChild, ready_path: &Path) -> Result<Ready> {
    let deadline = Instant::now() + SERVICE_START_TIMEOUT;
    loop {
        if ready_path.exists() {
            return Ok(serde_json::from_slice(&std::fs::read(ready_path)?)?);
        }
        if let Some(status) = seeder.0.try_wait()? {
            bail!("seeder exited before becoming ready: {status}");
        }
        ensure!(Instant::now() < deadline, "seeder startup timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn stop_seeder(seeder: &mut SeederChild, stop_path: &Path) -> Result<()> {
    std::fs::write(stop_path, b"stop")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = seeder.0.try_wait()? {
            ensure!(status.success(), "seeder exited unsuccessfully: {status}");
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "seeder did not stop cleanly");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn add_tracker(metainfo: &[u8], tracker_url: &str) -> Result<Vec<u8>> {
    let parsed = torrent_from_bytes(metainfo).context("seeder produced invalid metainfo")?;
    let mut with_tracker = b"d8:announce".to_vec();
    with_tracker.extend(format!("{}:{tracker_url}", tracker_url.len()).as_bytes());
    with_tracker.extend(b"4:info");
    with_tracker.extend(parsed.info.raw_bytes.as_ref());
    with_tracker.push(b'e');
    let updated = torrent_from_bytes(&with_tracker).context("tracker metainfo is invalid")?;
    ensure!(
        updated.info_hash == parsed.info_hash,
        "adding the tracker changed the info hash"
    );
    Ok(with_tracker)
}

async fn run_tracker(
    listener: TcpListener,
    peer: SocketAddr,
    request_count: Arc<AtomicU64>,
    announce_seen: oneshot::Sender<()>,
    release_first: oneshot::Receiver<()>,
) -> Result<()> {
    let mut announce_seen = Some(announce_seen);
    let mut release_first = release_first;
    loop {
        let (mut stream, _) = listener.accept().await?;
        read_http_headers(&mut stream).await?;
        let first = request_count.fetch_add(1, Ordering::Relaxed) == 0;
        if first {
            if let Some(signal) = announce_seen.take() {
                let _ = signal.send(());
            }
            tokio::time::timeout(SERVICE_START_TIMEOUT, &mut release_first)
                .await
                .context("benchmark did not release the tracker peer")??;
        }
        let mut body = b"d8:intervali30e5:peers6:".to_vec();
        body.extend(Ipv4Addr::LOCALHOST.octets());
        body.extend(peer.port().to_be_bytes());
        body.push(b'e');
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).await?;
        stream.write_all(&body).await?;
    }
}

async fn read_http_headers(stream: &mut TcpStream) -> Result<()> {
    let mut request = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .context("tracker request read timed out")??;
        ensure!(read > 0, "tracker connection closed before request headers");
        request.extend_from_slice(&chunk[..read]);
        ensure!(
            request.len() <= 16 * 1024,
            "tracker request headers too large"
        );
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}
