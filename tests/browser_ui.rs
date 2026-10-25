use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use torrent_lite::{
    browser_ui::BrowserUi, local_api::LocalApi, network_policy::DestinationPolicy,
    service_core::ServiceCore, service_discovery::EndpointLease,
};

async fn get(addr: std::net::SocketAddr, path: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn local_dashboard_requires_ephemeral_path_and_restricts_browser() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let database = workspace.path().join("state.db");
    let service = Arc::new(
        ServiceCore::open(&database, &output, DestinationPolicy::PublicOnly)
            .await
            .unwrap(),
    );
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let lease = EndpointLease::publish(&database, &api).unwrap();
    let ui = BrowserUi::bind(&database).await.unwrap();
    let addr = ui.local_addr();
    assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    let page = get(addr, ui.page_path(), &addr.to_string()).await;
    assert!(page.starts_with("HTTP/1.1 200"));
    assert!(page.contains("content-security-policy:"));
    assert!(page.contains("cache-control: no-store"));
    assert!(!page.contains(api.token()));
    assert!(
        get(addr, "/wrong/", &addr.to_string())
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        get(addr, ui.page_path(), "attacker.invalid")
            .await
            .starts_with("HTTP/1.1 403")
    );
    let data = get(addr, &format!("{}data", ui.page_path()), &addr.to_string()).await;
    assert!(data.starts_with("HTTP/1.1 200"));
    assert!(data.contains("[]"));
    drop(ui);
    drop(lease);
    drop(api);
    service.stop().await;
}
