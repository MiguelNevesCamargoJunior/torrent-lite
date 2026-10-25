use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
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
    request_body(addr, method, path, headers, "").await
}

async fn request_body(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\n{headers}Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
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
async fn add_rejects_unauthorized_body_and_registers_paused_magnet() {
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
    let host = format!("Host: {addr}\r\nContent-Type: application/json\r\n");
    let body = r#"{"magnet":"magnet:?xt=urn:btih:0000000000000000000000000000000000000001&tr=https://tracker.example/announce","paused":true}"#;
    let denied = request_body(addr, "POST", "/api/v1/torrents", &host, body).await;
    assert!(denied.starts_with("HTTP/1.1 401"));
    assert!(denied.contains("\"code\":\"unauthorized\""));
    assert!(!denied.contains("magnet:?"));
    assert!(service.list().unwrap().is_empty());
    let secret = format!("Authorization: Bearer {}\r\n", api.token());
    let missing_route =
        request_target(addr, "GET", "/api/v1/missing", &format!("{host}{secret}")).await;
    assert!(missing_route.starts_with("HTTP/1.1 404"));
    assert!(missing_route.contains("\"code\":\"not_found\""));
    let malformed = request_body(
        addr,
        "POST",
        "/api/v1/torrents",
        &format!("{host}{secret}"),
        "{",
    )
    .await;
    assert!(malformed.starts_with("HTTP/1.1 400"));
    assert!(malformed.contains("\"code\":\"bad_request\""));
    let allowed = request_body(
        addr,
        "POST",
        "/api/v1/torrents",
        &format!("{host}{secret}"),
        body,
    )
    .await;
    assert!(allowed.starts_with("HTTP/1.1 201"));
    assert_eq!(service.list().unwrap().len(), 1);
    assert_eq!(
        service.list().unwrap()[0].desired,
        torrent_lite::state_store::DesiredState::Paused
    );
    let listed = request(addr, &format!("{host}{secret}")).await;
    assert!(listed.contains("\"name\":null"));
    assert!(listed.contains("\"observed_state\":null"));
    assert!(listed.contains("\"error_code\":null"));
    assert!(listed.contains("\"retry_after_secs\":null"));
    assert!(listed.contains("\"download_bytes_per_sec\":null"));
    drop(api);
    service.stop().await;
}

#[tokio::test]
async fn activation_failure_has_a_safe_machine_readable_state() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let database = workspace.path().join("state.db");
    let service = Arc::new(
        ServiceCore::open(&database, &output, DestinationPolicy::PublicOnly)
            .await
            .unwrap(),
    );
    let mut metainfo = b"d4:infod6:lengthi4e4:name8:file.txt12:piece lengthi4e6:pieces20:".to_vec();
    metainfo.extend([0_u8; 20]);
    metainfo.extend(b"ee");
    let item = service.add_torrent(&metainfo, true).await.unwrap();
    let collision = output.join(&item.id);
    std::fs::write(&collision, b"collision").unwrap();
    service.resume(&item.id).await.unwrap();
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let headers = format!(
        "Host: {}\r\nAuthorization: Bearer {}\r\n",
        api.local_addr(),
        api.token()
    );
    let listed = request(api.local_addr(), &headers).await;
    assert!(listed.contains("\"observed_state\":\"retrying\""));
    assert!(listed.contains("\"error_code\":\"activation_failed\""));
    assert!(listed.contains("\"retry_after_secs\":"));
    assert!(!listed.contains("collision"));
    service.pause(&item.id).await.unwrap();
    let paused = request(api.local_addr(), &headers).await;
    assert!(paused.contains("\"error_code\":null"));
    drop(api);
    service.stop().await;
}

#[tokio::test]
async fn pending_magnet_reports_resolution_without_claiming_download_progress() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let tracker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let tracker_url = format!("http://{}/announce", tracker.local_addr().unwrap());
    let service = Arc::new(
        ServiceCore::open(
            &workspace.path().join("state.db"),
            &output,
            DestinationPolicy::AllowLan,
        )
        .await
        .unwrap(),
    );
    let magnet =
        format!("magnet:?xt=urn:btih:0000000000000000000000000000000000000001&tr={tracker_url}");
    let item = service.add_magnet(&magnet).await.unwrap();
    service.tick_queue().await.unwrap();
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let headers = format!(
        "Host: {}\r\nAuthorization: Bearer {}\r\n",
        api.local_addr(),
        api.token()
    );
    let resolving = request(api.local_addr(), &headers).await;
    assert!(resolving.contains("\"observed_state\":\"resolving\""));
    assert!(resolving.contains("\"progress_bytes\":null"));
    assert!(resolving.contains("\"error_code\":null"));
    service.pause(&item.id).await.unwrap();
    let paused = request(api.local_addr(), &headers).await;
    assert!(paused.contains("\"observed_state\":null"));
    drop(api);
    service.stop().await;
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
    assert!(
        request_target(
            addr,
            "POST",
            &path,
            &format!("{host}{secret}Origin: http://evil.invalid\r\n")
        )
        .await
        .contains("\"code\":\"forbidden\"")
    );
    assert_eq!(service.list().unwrap().len(), 1);
    let pause_path = format!("/api/v1/torrents/{}/pause", item.id);
    assert!(
        request_target(addr, "POST", &pause_path, &format!("{host}{secret}"))
            .await
            .starts_with("HTTP/1.1 204")
    );
    assert_eq!(
        service.list().unwrap()[0].desired,
        torrent_lite::state_store::DesiredState::Paused
    );
    let verify_path = format!("/api/v1/torrents/{}/verify", item.id);
    let unverified = request_target(addr, "POST", &verify_path, &format!("{host}{secret}")).await;
    assert!(unverified.starts_with("HTTP/1.1 422"));
    assert!(unverified.contains("\"code\":\"unprocessable\""));
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
