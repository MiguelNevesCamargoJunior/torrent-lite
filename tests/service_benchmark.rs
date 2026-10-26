use serde_json::Value;
use std::process::Command;

#[test]
fn benchmark_uses_authenticated_service_and_validates_proxied_transfer_after_restart() {
    let output = Command::new(env!("CARGO_BIN_EXE_service_benchmark"))
        .args(["--size-mib", "8"])
        .output()
        .expect("service benchmark process starts");
    assert!(
        output.status.success(),
        "service benchmark failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("benchmark emits JSON");

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["transport"], "tcp_loopback_through_policy_proxy");
    assert!(report["tracker_announces"].as_u64().is_some_and(|n| n >= 1));
    assert!(
        report["proxy_tracker_connections"]
            .as_u64()
            .is_some_and(|n| n >= 1)
    );
    assert!(
        report["proxy_peer_connections"]
            .as_u64()
            .is_some_and(|n| n >= 1)
    );
    assert_eq!(report["download_verified"], true);
    assert!(
        report["restart_validation_seconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0)
    );
}
