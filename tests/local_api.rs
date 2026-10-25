use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use torrent_lite::{
    local_api::LocalApi, network_policy::DestinationPolicy, service_core::ServiceCore,
};

async fn request(addr: std::net::SocketAddr, headers: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET /api/v1/torrents HTTP/1.1\r\n{headers}Connection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(response).unwrap()
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
