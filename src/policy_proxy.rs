//! Authenticated local SOCKS5 CONNECT proxy for filtering concrete socket destinations.
//! This is not yet wired to a production engine session.

use crate::network_policy::DestinationPolicy;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, lookup_host},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

const USERNAME: &str = "torrent-lite";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 150;

pub struct PolicyProxy {
    local_addr: SocketAddr,
    password: String,
    successful_connections: Arc<AtomicU64>,
    task: JoinHandle<()>,
}

impl PolicyProxy {
    pub async fn bind(policy: DestinationPolicy) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let local_addr = listener.local_addr()?;
        let mut secret = [0_u8; 32];
        getrandom::fill(&mut secret).map_err(|_| io::Error::other("OS randomness unavailable"))?;
        let password = secret
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let expected = password.clone();
        let successful_connections = Arc::new(AtomicU64::new(0));
        let count = successful_connections.clone();
        let task = tokio::spawn(async move { serve(listener, expected, policy, count).await });
        Ok(Self {
            local_addr,
            password,
            successful_connections,
            task,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn successful_connections(&self) -> u64 {
        self.successful_connections.load(Ordering::Relaxed)
    }

    /// Keep this URL in process memory only; never put it in logs, CLI arguments or state.
    pub fn proxy_url(&self) -> String {
        format!("socks5://{USERNAME}:{}@{}", self.password, self.local_addr)
    }
}

impl Drop for PolicyProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    listener: TcpListener,
    password: String,
    policy: DestinationPolicy,
    count: Arc<AtomicU64>,
) {
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue };
                let password = password.clone();
                let count = count.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = handle(stream, &password, policy, &count).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

async fn handle(
    mut client: TcpStream,
    password: &str,
    policy: DestinationPolicy,
    count: &AtomicU64,
) -> io::Result<()> {
    timeout(HANDSHAKE_TIMEOUT, authenticate(&mut client, password)).await??;
    let target = match timeout(HANDSHAKE_TIMEOUT, read_destination(&mut client)).await? {
        Ok(target) => target,
        Err(error) => {
            let _ = send_reply(&mut client, 7).await;
            return Err(error);
        }
    };
    let addresses = match resolve(target, policy).await {
        Ok(addresses) => addresses,
        Err(code) => {
            send_reply(&mut client, code).await?;
            return Ok(());
        }
    };
    for address in addresses {
        if let Ok(Ok(mut remote)) = timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            count.fetch_add(1, Ordering::Relaxed);
            send_reply(&mut client, 0).await?;
            let _ = tokio::io::copy_bidirectional(&mut client, &mut remote).await?;
            return Ok(());
        }
    }
    send_reply(&mut client, 5).await
}

async fn authenticate(client: &mut TcpStream, expected_password: &str) -> io::Result<()> {
    let mut greeting = [0_u8; 2];
    client.read_exact(&mut greeting).await?;
    if greeting[0] != 5 || greeting[1] == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS greeting",
        ));
    }
    let mut methods = vec![0; greeting[1] as usize];
    client.read_exact(&mut methods).await?;
    if !methods.contains(&2) {
        client.write_all(&[5, 255]).await?;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SOCKS authentication required",
        ));
    }
    client.write_all(&[5, 2]).await?;
    let mut auth = [0_u8; 2];
    client.read_exact(&mut auth).await?;
    if auth[0] != 1 || auth[1] == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS authentication",
        ));
    }
    let mut user = vec![0; auth[1] as usize];
    client.read_exact(&mut user).await?;
    let pass_len = client.read_u8().await? as usize;
    let mut pass = vec![0; pass_len];
    client.read_exact(&mut pass).await?;
    let valid =
        user == USERNAME.as_bytes() && constant_time_eq(&pass, expected_password.as_bytes());
    client.write_all(&[1, if valid { 0 } else { 1 }]).await?;
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SOCKS authentication failed",
        ))
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

enum Destination {
    Address(SocketAddr),
    Domain(String, u16),
}

async fn read_destination(client: &mut TcpStream) -> io::Result<Destination> {
    let mut header = [0_u8; 4];
    client.read_exact(&mut header).await?;
    if header[0] != 5 || header[1] != 1 || header[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported SOCKS request",
        ));
    }
    let destination = match header[3] {
        1 => {
            let mut bytes = [0_u8; 4];
            client.read_exact(&mut bytes).await?;
            IpAddr::V4(Ipv4Addr::from(bytes))
        }
        4 => {
            let mut bytes = [0_u8; 16];
            client.read_exact(&mut bytes).await?;
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        3 => {
            let length = client.read_u8().await? as usize;
            if length == 0 || length > 253 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid SOCKS domain",
                ));
            }
            let mut bytes = vec![0_u8; length];
            client.read_exact(&mut bytes).await?;
            let domain = String::from_utf8(bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SOCKS domain"))?;
            let port = client.read_u16().await?;
            if port == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid SOCKS port",
                ));
            }
            return Ok(Destination::Domain(domain, port));
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported SOCKS address",
            ));
        }
    };
    let port = client.read_u16().await?;
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid SOCKS port",
        ));
    }
    Ok(Destination::Address(SocketAddr::new(destination, port)))
}

async fn resolve(
    destination: Destination,
    policy: DestinationPolicy,
) -> Result<Vec<SocketAddr>, u8> {
    match destination {
        Destination::Address(address) => {
            if policy.permits_ip(address.ip()) {
                Ok(vec![address])
            } else {
                Err(2)
            }
        }
        Destination::Domain(domain, port) => {
            let resolved = timeout(CONNECT_TIMEOUT, lookup_host((domain.as_str(), port)))
                .await
                .map_err(|_| 4_u8)?
                .map_err(|_| 4_u8)?;
            let allowed: Vec<_> = resolved
                .filter(|addr| policy.permits_ip(addr.ip()))
                .take(16)
                .collect();
            if allowed.is_empty() {
                Err(2)
            } else {
                Ok(allowed)
            }
        }
    }
}

async fn send_reply(client: &mut TcpStream, code: u8) -> io::Result<()> {
    client.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}
