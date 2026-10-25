use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_torrent-lite"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn inspect_magnet_emits_machine_readable_identity() {
    let result = run(&[
        "inspect-magnet",
        "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&dn=Example",
        "--json",
    ]);
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"info_hash":"000102030405060708090a0b0c0d0e0f10111213", "display_name":"Example"})
    );
    assert!(result.stderr.is_empty());
}

#[test]
fn invalid_input_does_not_echo_secrets() {
    let result = run(&[
        "inspect-magnet",
        "magnet:?xt=urn:btih:bad&tr=https://user:TOPSECRET@example.test",
    ]);
    assert_eq!(result.status.code(), Some(2));
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("info hash"));
    assert!(!error.contains("TOPSECRET"));
    assert!(result.stdout.is_empty());
}

#[test]
fn unsupported_commands_fail_instead_of_claiming_a_download() {
    let result = run(&[
        "download",
        "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
}

#[test]
fn human_output_escapes_terminal_control_sequences() {
    let result = run(&[
        "inspect-magnet",
        "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&dn=hello%1B%5B31m%0Aworld",
    ]);
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("hello"));
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains("\nworld"));
}

#[test]
fn usage_errors_do_not_echo_secret_bearing_arguments() {
    let secret = "magnet:?tr=https://user:TOPSECRET@example.test";
    for args in [
        vec![secret],
        vec![
            "inspect-magnet",
            "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213",
            secret,
        ],
        vec!["inspect-magnet", "--TOPSECRET"],
    ] {
        let result = run(&args);
        assert_eq!(result.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("TOPSECRET"));
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn help_and_version_remain_available_without_input_echo() {
    for flag in ["--help", "--version"] {
        let result = run(&[flag]);
        assert!(result.status.success());
        assert!(!result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn inspect_torrent_reads_a_local_file_and_emits_json() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("example.torrent");
    let mut bytes = b"d4:infod6:lengthi4e4:name8:file.txt12:piece lengthi4e6:pieces20:".to_vec();
    bytes.extend([0_u8; 20]);
    bytes.extend(b"ee");
    std::fs::write(&path, bytes).unwrap();
    let result = run(&["inspect-torrent", path.to_str().unwrap(), "--json"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value["info_hash"],
        "fc13cb96cc1f1dd111edc8c97537d90a7a3feba3"
    );
    assert_eq!(value["files"][0]["path"][0], "file.txt");
    assert_eq!(value["total_bytes"], 4);
}

#[test]
fn inspect_torrent_rejects_oversized_file_without_echoing_its_name() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("TOPSECRET.torrent");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    let result = run(&["inspect-torrent", path.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("TOPSECRET"));
}
