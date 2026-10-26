//! Synthetic loopback benchmark. The seeder runs in a separate process.
use anyhow::{Context, Result};
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    storage::StorageFactoryExt,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
};

#[derive(Serialize, Deserialize)]
struct Ready {
    peer: String,
    metainfo_base64: String,
}

struct SeederChild(Child);

impl Drop for SeederChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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
        peer_limit: Some(8),
        ..Default::default()
    }
}

fn create_payload(path: &Path, size: u64) -> Result<()> {
    let mut file = File::create(path)?;
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut block = [0_u8; 1024 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let count = remaining.min(block.len() as u64) as usize;
        for byte in &mut block[..count] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        file.write_all(&block[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}

fn same_contents(left: &Path, right: &Path) -> Result<bool> {
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut remaining = left.metadata()?.len();
    if remaining != right.metadata()?.len() {
        return Ok(false);
    }
    let mut a = [0_u8; 1024 * 1024];
    let mut b = [0_u8; 1024 * 1024];
    while remaining > 0 {
        let count = remaining.min(a.len() as u64) as usize;
        left.read_exact(&mut a[..count])?;
        right.read_exact(&mut b[..count])?;
        if a[..count] != b[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(true)
}

async fn seed(source_file: &Path, ready_path: &Path, stop_path: &Path, size: u64) -> Result<()> {
    create_payload(source_file, size)?;
    let source_dir = source_file.parent().context("source has no parent")?;
    let session = Session::new_with_opts(source_dir.to_path_buf(), isolated_options()).await?;
    let (torrent, _) = session
        .create_and_serve_torrent(
            source_file,
            CreateTorrentOptions {
                piece_length: Some(256 * 1024),
                ..Default::default()
            },
        )
        .await?;
    let ready = Ready {
        peer: session
            .listen_addr()
            .context("seeder has no listener")?
            .to_string(),
        metainfo_base64: data_encoding::BASE64.encode(&torrent.as_bytes()?),
    };
    let temporary = ready_path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&ready)?)?;
    std::fs::rename(temporary, ready_path)?;
    while !stop_path.exists() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    session.stop().await;
    Ok(())
}

async fn measure(size_mib: u64) -> Result<()> {
    anyhow::ensure!((8..=256).contains(&size_mib), "size must be 8..=256 MiB");
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
        .spawn()?;
    let mut seeder = SeederChild(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready_path.exists() {
        if let Some(status) = seeder.0.try_wait()? {
            anyhow::bail!("seeder exited before ready: {status}");
        }
        anyhow::ensure!(Instant::now() < deadline, "seeder start timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let ready: Ready = serde_json::from_slice(&std::fs::read(&ready_path)?)?;
    let peer: SocketAddr = ready.peer.parse()?;
    anyhow::ensure!(peer.ip().is_loopback(), "seeder was not on loopback");
    let metainfo = data_encoding::BASE64.decode(ready.metainfo_base64.as_bytes())?;
    let database = workspace.path().join("state.db");
    let id = StateStore::open(&database)?
        .register(AddInput::Torrent(&metainfo), &destination, false)?
        .id;
    let state = Arc::new(Mutex::new(StateStore::open(&database)?));
    let root = Arc::new(SafeRoot::open(&destination)?.subdir(&id)?);
    let session = Session::new_with_opts(destination.clone(), isolated_options()).await?;

    let started = Instant::now();
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(metainfo),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![peer]),
                peer_limit: Some(8),
                overwrite: true,
                storage_factory: Some(DurableStorageFactory::new(root, state, id.clone()).boxed()),
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("engine returned no handle")?;
    tokio::time::timeout(Duration::from_secs(180), handle.wait_until_completed()).await??;
    let transfer_seconds = started.elapsed().as_secs_f64();
    anyhow::ensure!(
        same_contents(&source_file, &destination.join(&id).join("payload.bin"))?,
        "downloaded bytes differ from source"
    );
    session.stop().await;
    std::fs::write(&stop_path, b"stop")?;
    for _ in 0..100 {
        if seeder.0.try_wait()?.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let result = serde_json::json!({
        "schema_version": 1,
        "payload_bytes": size,
        "transfer_seconds": transfer_seconds,
        "transfer_mib_per_sec": size_mib as f64 / transfer_seconds,
        "transport": "tcp_loopback_direct",
        "storage": "durable_sqlite_full_piece_recheck",
    });
    println!("{result}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--seed") {
        anyhow::ensure!(args.len() == 6, "invalid seeder arguments");
        return seed(
            &PathBuf::from(&args[2]),
            &PathBuf::from(&args[3]),
            &PathBuf::from(&args[4]),
            args[5].parse()?,
        )
        .await;
    }
    anyhow::ensure!(
        args.len() == 3 && args[1] == "--size-mib",
        "usage: benchmark_local --size-mib N"
    );
    measure(args[2].parse()?).await
}
