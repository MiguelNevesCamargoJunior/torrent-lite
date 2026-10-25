use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
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
async fn dashboard_file_selection_uses_csrf_and_updates_paused_torrent() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
    let database = workspace.path().join("state.db");
    let service = Arc::new(
        ServiceCore::open(&database, &output, DestinationPolicy::AllowLan)
            .await
            .unwrap(),
    );
    let api = LocalApi::bind(service.clone()).await.unwrap();
    let _lease = EndpointLease::publish(&database, &api).unwrap();
    let ui = BrowserUi::bind(&database).await.unwrap();
    let mut metainfo = b"d4:infod5:filesld6:lengthi4e4:pathl5:a.bineed6:lengthi4e4:pathl5:b.bineee4:name4:test12:piece lengthi4e6:pieces40:".to_vec();
    metainfo.extend([0_u8; 40]);
    metainfo.extend(b"ee");
    let item = service.add_torrent(&metainfo, true).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://{}{}", ui.local_addr(), ui.page_path());
    let path = format!("{base}torrents/{}/files", item.id);
    let initial: serde_json::Value = client
        .get(&path)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial["summary"]["files"].as_array().unwrap().len(), 2);
    assert!(initial["only_files"].is_null());
    let no_csrf = client
        .post(&path)
        .json(&serde_json::json!({"only_files":[1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(no_csrf.status(), reqwest::StatusCode::FORBIDDEN);
    let token: serde_json::Value = client
        .get(format!("{base}csrf"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let selected = client
        .post(&path)
        .header("origin", format!("http://{}", ui.local_addr()))
        .header("x-csrf-token", token["token"].as_str().unwrap())
        .json(&serde_json::json!({"only_files":[1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(selected.status(), reqwest::StatusCode::NO_CONTENT);
    let updated: serde_json::Value = client
        .get(&path)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated["only_files"], serde_json::json!([1]));
    drop(ui);
    drop(api);
    service.stop().await;
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
    assert!(page.contains("Transferências"));
    assert!(page.contains("setAttribute('role','progressbar')"));
    assert!(!page.contains("<script src="));
    assert!(!page.contains("<link rel=\"stylesheet\""));
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
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://{addr}{}", ui.page_path());
    let csrf_response = client.get(format!("{base}csrf")).send().await.unwrap();
    assert_eq!(csrf_response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        csrf_response.headers()["cache-control"].to_str().unwrap(),
        "no-store"
    );
    let csrf_body: serde_json::Value = csrf_response.json().await.unwrap();
    let csrf = csrf_body["token"].as_str().unwrap();
    assert_ne!(csrf, api.token());
    assert_eq!(csrf.len(), 64);
    let item = service
        .add_magnet_with_pause(
            "magnet:?xt=urn:btih:0000000000000000000000000000000000000001&tr=https://tracker.example/announce",
            true,
        )
        .await
        .unwrap();
    let remove_url = format!("{base}torrents/{}/remove", item.id);
    let origin = format!("http://{addr}");
    let missing_origin = client
        .post(&remove_url)
        .header("content-type", "application/json")
        .header("x-csrf-token", csrf)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(missing_origin.status(), reqwest::StatusCode::FORBIDDEN);
    let foreign_origin = client
        .post(&remove_url)
        .header("origin", "https://attacker.invalid")
        .header("content-type", "application/json")
        .header("x-csrf-token", csrf)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign_origin.status(), reqwest::StatusCode::FORBIDDEN);
    let missing_token = client
        .post(&remove_url)
        .header("origin", &origin)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(missing_token.status(), reqwest::StatusCode::FORBIDDEN);
    let wrong_media = client
        .post(format!("{base}add-torrent"))
        .header("origin", &origin)
        .header("x-csrf-token", csrf)
        .header("content-type", "text/plain")
        .body("not a torrent")
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_media.status(),
        reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let too_large = client
        .post(format!("{base}add-magnet"))
        .header("origin", &origin)
        .header("x-csrf-token", csrf)
        .header("content-type", "application/json")
        .body(vec![b'a'; 16 * 1024 + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(too_large.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(service.list().unwrap().len(), 1);
    let valid = client
        .post(&remove_url)
        .header("origin", &origin)
        .header("content-type", "application/json")
        .header("x-csrf-token", csrf)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(valid.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(service.list().unwrap().is_empty());
    let added_magnet = client
        .post(format!("{base}add-magnet"))
        .header("origin", &origin)
        .header("x-csrf-token", csrf)
        .json(&serde_json::json!({
            "magnet": "magnet:?xt=urn:btih:0000000000000000000000000000000000000002&tr=https://tracker.example/announce",
            "paused": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(added_magnet.status(), reqwest::StatusCode::CREATED);
    assert_eq!(service.list().unwrap().len(), 1);
    assert_eq!(
        service.list().unwrap()[0].desired,
        torrent_lite::state_store::DesiredState::Paused
    );
    let added_id = service.list().unwrap()[0].id.clone();
    let removed_magnet = client
        .post(format!("{base}torrents/{added_id}/remove"))
        .header("origin", &origin)
        .header("x-csrf-token", csrf)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(removed_magnet.status(), reqwest::StatusCode::NO_CONTENT);

    let source = workspace.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let source_file = source.join("sample.bin");
    std::fs::write(&source_file, vec![17_u8; 16 * 1024]).unwrap();
    let seeder = Session::new_with_opts(
        source,
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                ipv4_only: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (torrent, _) = seeder
        .create_and_serve_torrent(
            &source_file,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let uploaded = client
        .post(format!("{base}add-torrent"))
        .header("origin", &origin)
        .header("x-csrf-token", csrf)
        .header("content-type", "application/x-bittorrent")
        .body(torrent.as_bytes().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(uploaded.status(), reqwest::StatusCode::CREATED);
    assert_eq!(service.list().unwrap().len(), 1);
    seeder.stop().await;
    drop(ui);
    drop(lease);
    drop(api);
    service.stop().await;
}
