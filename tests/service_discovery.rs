use std::process::{Child, Command, Stdio};
use std::time::Duration;
use std::{net::Ipv4Addr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use torrent_lite::{
    local_api::LocalApi, network_policy::DestinationPolicy, service_core::ServiceCore,
    service_discovery::EndpointLease,
};

#[tokio::test]
async fn endpoint_is_private_versioned_and_detects_tampering() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("download");
    std::fs::create_dir(&root).unwrap();
    let database = workspace.path().join("state.db");
    let service = Arc::new(
        ServiceCore::open(&database, &root, DestinationPolicy::PublicOnly)
            .await
            .unwrap(),
    );
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let lease = EndpointLease::publish(&database, &api).unwrap();
    let endpoint = EndpointLease::read(&database).unwrap();
    assert_eq!(endpoint.addr, api.local_addr());
    assert_eq!(endpoint.addr.ip(), Ipv4Addr::LOCALHOST);
    assert_eq!(endpoint.token, api.token());

    let bytes = std::fs::read(lease.path()).unwrap();
    assert!(bytes.len() <= 4096);
    #[cfg(windows)]
    assert!(!String::from_utf8_lossy(&bytes).contains(api.token()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(lease.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        std::fs::set_permissions(lease.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(EndpointLease::read(&database).is_err());
        std::fs::set_permissions(lease.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(lease.path(), b"{\"version\":999}").unwrap();
    assert!(EndpointLease::read(&database).is_err());
    std::fs::write(lease.path(), vec![b'X'; 4097]).unwrap();
    assert!(EndpointLease::read(&database).is_err());
    drop(lease);
    drop(api);
    service.stop().await;
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn await_new_endpoint(
    database: &std::path::Path,
    old_token: Option<&str>,
) -> torrent_lite::service_discovery::EndpointInfo {
    for _ in 0..200 {
        if let Ok(endpoint) = EndpointLease::read(database)
            && old_token != Some(endpoint.token.as_str())
        {
            return endpoint;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("service did not publish a new endpoint");
}

async fn list_over_http(endpoint: &torrent_lite::service_discovery::EndpointInfo) -> String {
    let mut stream = TcpStream::connect(endpoint.addr).await.unwrap();
    let request = format!(
        "GET /api/v1/torrents HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        endpoint.addr, endpoint.token
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(response).unwrap()
}

#[tokio::test]
async fn forced_stop_then_restart_replaces_stale_endpoint() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("download");
    std::fs::create_dir(&root).unwrap();
    let database = workspace.path().join("state.db");
    let start = || {
        ChildGuard(
            Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
                .arg("serve")
                .arg("--output")
                .arg(&root)
                .arg("--state")
                .arg(&database)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let mut first = start();
    let before = await_new_endpoint(&database, None).await;
    assert!(list_over_http(&before).await.starts_with("HTTP/1.1 200"));
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    let second = start();
    let after = await_new_endpoint(&database, Some(&before.token)).await;
    assert_ne!(after.token, before.token);
    assert!(list_over_http(&after).await.starts_with("HTTP/1.1 200"));
    drop(second);
}
