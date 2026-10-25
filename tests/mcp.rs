use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};
use torrent_lite::{
    local_api::LocalApi,
    network_policy::DestinationPolicy,
    service_core::ServiceCore,
    service_discovery::EndpointLease,
    state_store::{AddInput, StateStore},
};

#[test]
fn read_only_stdio_server_advertises_only_read_tools() {
    let workspace = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .args(["mcp", "--state"])
        .arg(workspace.path().join("missing.db"))
        .arg("--read-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for message in [
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"torrent_remove","arguments":{"id":"00000000-0000-0000-0000-000000000001"}}}),
        ] {
            writeln!(stdin, "{message}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let messages: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let listed = messages.iter().find(|message| message["id"] == 2).unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"torrent_list"));
    assert!(!names.contains(&"torrent_remove"));
    assert!(!names.contains(&"torrent_add_magnet"));
    let refused = messages.iter().find(|message| message["id"] == 3).unwrap();
    assert!(refused["error"].is_object() || refused["result"]["isError"] == true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_mutation_controls_the_running_service_without_exposing_secret() {
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .args(["mcp", "--state"])
        .arg(&database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for message in [
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"torrent_remove","arguments":{"id":item.id}}}),
        ] {
            writeln!(stdin, "{message}").unwrap();
        }
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let transcript = String::from_utf8(result.stdout).unwrap();
    assert!(!transcript.contains(api.token()));
    let messages: Vec<serde_json::Value> = transcript
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let removed = messages.iter().find(|message| message["id"] == 2).unwrap();
    assert_eq!(removed["result"]["isError"], false);
    assert!(service.list().unwrap().is_empty());
    drop(lease);
    drop(api);
    service.stop().await;
}
