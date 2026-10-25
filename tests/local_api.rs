use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use torrent_lite::{
    local_api::LocalApi,
    network_policy::DestinationPolicy,
    service_core::ServiceCore,
    state_store::{AddInput, StateStore},
};

async fn request(addr: std::net::SocketAddr, headers: &str) -> String {
    request_target(addr, "GET", "/api/v1/torrents", headers).await
}

async fn request_target(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\n{headers}Connection: close\r\nContent-Length: 0\r\n\r\n"
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
async fn removal_requires_authorization_and_preserves_downloaded_files() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let kept = output.join("keep.bin");
    std::fs::write(&kept, b"keep").unwrap();
    let database = workspace.path().join("state.db");
    let item = StateStore::open(&database)
        .unwrap()
        .register(
            AddInput::Magnet("magnet:?xt=urn:btih:0000000000000000000000000000000000000001"),
            &output,
            true,
        )
        .unwrap();
    let service = Arc::new(
        ServiceCore::open(&database, &output, DestinationPolicy::PublicOnly)
            .await
            .unwrap(),
    );
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let addr = api.local_addr();
    let path = format!("/api/v1/torrents/{}/remove", item.id);
    let host = format!("Host: {addr}\r\n");
    let secret = format!("Authorization: Bearer {}\r\n", api.token());
    assert!(
        request_target(addr, "POST", &path, &host)
            .await
            .starts_with("HTTP/1.1 401")
    );
    assert!(
        request_target(
            addr,
            "POST",
            &path,
            &format!("{host}{secret}Origin: http://evil.invalid\r\n")
        )
        .await
        .starts_with("HTTP/1.1 403")
    );
    assert_eq!(service.list().unwrap().len(), 1);
    assert!(
        request_target(addr, "POST", &path, &format!("{host}{secret}"))
            .await
            .starts_with("HTTP/1.1 204")
    );
    assert!(service.list().unwrap().is_empty());
    assert_eq!(std::fs::read(&kept).unwrap(), b"keep");
    drop(api);
    service.stop().await;
}

#[tokio::test]
async fn api_is_loopback_only_and_rejects_wrong_token_host_or_origin() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let service = Arc::new(
        ServiceCore::open(
            &workspace.path().join("state.db"),
            &output,
            DestinationPolicy::PublicOnly,
        )
        .await
        .unwrap(),
    );
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let addr = api.local_addr();
    assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    let host = format!("Host: {addr}\r\n");
    let secret = format!("Authorization: Bearer {}\r\n", api.token());

    assert!(request(addr, &host).await.starts_with("HTTP/1.1 401"));
    assert!(
        request(addr, &format!("{host}Authorization: Bearer wrong\r\n"))
            .await
            .starts_with("HTTP/1.1 401")
    );
    assert!(
        request(addr, &format!("Host: attacker.invalid\r\n{secret}"))
            .await
            .starts_with("HTTP/1.1 403")
    );
    assert!(
        request(
            addr,
            &format!("{host}{secret}Origin: https://attacker.invalid\r\n")
        )
        .await
        .starts_with("HTTP/1.1 403")
    );
    let allowed = request(addr, &format!("{host}{secret}Origin: http://{addr}\r\n")).await;
    assert!(allowed.starts_with("HTTP/1.1 200"));
    assert!(allowed.contains("[]"));
    drop(api);
    service.stop().await;
}
