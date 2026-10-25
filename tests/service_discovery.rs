use std::{net::Ipv4Addr, sync::Arc};
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
