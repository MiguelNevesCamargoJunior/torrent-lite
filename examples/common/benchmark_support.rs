//! Shared deterministic payload and separate-process seeder helpers for benchmarks.

use anyhow::{Context, Result};
use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    process::Child,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
pub struct Ready {
    pub peer: String,
    pub metainfo_base64: String,
}

pub struct SeederChild(pub Child);

impl Drop for SeederChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn isolated_options() -> SessionOptions {
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

pub fn create_payload(path: &Path, size: u64) -> Result<()> {
    let mut file = File::create(path)?;
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut block = vec![0_u8; 1024 * 1024];
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

pub fn same_contents(left: &Path, right: &Path) -> Result<bool> {
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut remaining = left.metadata()?.len();
    if remaining != right.metadata()?.len() {
        return Ok(false);
    }
    let mut a = vec![0_u8; 1024 * 1024];
    let mut b = vec![0_u8; 1024 * 1024];
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

pub async fn seed(
    source_file: &Path,
    ready_path: &Path,
    stop_path: &Path,
    size: u64,
) -> Result<()> {
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
