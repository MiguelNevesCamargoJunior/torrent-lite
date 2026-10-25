use std::process::Command;
use std::sync::Arc;
use torrent_lite::{
    api_client::LocalClient,
    local_api::LocalApi,
    network_policy::DestinationPolicy,
    service_core::ServiceCore,
    service_discovery::EndpointLease,
    state_store::{AddInput, StateStore},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_discovers_running_service_and_uses_same_state() {
    let workspace = tempfile::tempdir().unwrap();
    let output = workspace.path().join("download");
    std::fs::create_dir(&output).unwrap();
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
    let lease = EndpointLease::publish(&database, &api).unwrap();
    let client = LocalClient::discover(&database).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .arg("list")
        .arg("--state")
        .arg(&database)
        .arg("--json")
        .output()
        .unwrap();
    assert!(cli.status.success());
    assert!(String::from_utf8(cli.stdout).unwrap().contains(&item.id));
    let rows = client.list().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, item.id);
    assert_eq!(rows[0].desired, "paused");
    let status = client.status().await.unwrap();
    assert_eq!(status.registered, 1);
    assert_eq!(status.paused, 1);
    let cli_status = Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .arg("status")
        .arg("--state")
        .arg(&database)
        .arg("--json")
        .output()
        .unwrap();
    assert!(cli_status.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&cli_status.stdout).unwrap();
    assert_eq!(parsed["registered"], 1);
    client.pause(&item.id).await.unwrap();
    client.remove(&item.id).await.unwrap();
    assert!(client.list().await.unwrap().is_empty());
    let add = Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .arg("add")
        .arg("magnet:?xt=urn:btih:0000000000000000000000000000000000000002&tr=https://tracker.example/announce")
        .arg("--state")
        .arg(&database)
        .arg("--paused")
        .output()
        .unwrap();
    assert!(add.status.success());
    let added = client.list().await.unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].desired, "paused");
    let mut shutdown = api.shutdown_receiver();
    let stop_task = tokio::spawn(async move { client.stop_service().await });
    tokio::time::timeout(std::time::Duration::from_secs(3), shutdown.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(*shutdown.borrow());
    api.close().await;
    service.stop().await;
    drop(lease);
    stop_task.await.unwrap().unwrap();
}
