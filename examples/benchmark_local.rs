//! Synthetic loopback benchmark. The seeder runs in a separate process.
#[path = "common/benchmark_support.rs"]
mod benchmark_support;

use anyhow::{Context, Result};
use benchmark_support::{Ready, SeederChild, isolated_options, same_contents, seed};
use librqbit::{AddTorrent, AddTorrentOptions, Session, storage::StorageFactoryExt};
use std::{
    net::SocketAddr,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use torrent_lite::{
    engine_storage::DurableStorageFactory,
    safe_storage::SafeRoot,
    state_store::{AddInput, StateStore},
};

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
