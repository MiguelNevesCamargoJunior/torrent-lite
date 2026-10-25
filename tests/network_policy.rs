use std::net::IpAddr;
use torrent_lite::network_policy::{DestinationPolicy, TrackerUrlError};

fn ip(value: &str) -> IpAddr {
    value.parse().unwrap()
}

#[test]
fn public_unicast_addresses_are_permitted() {
    let policy = DestinationPolicy::PublicOnly;
    for address in [
        "8.8.8.8",
        "1.1.1.1",
        "2001:4860:4860::8888",
        "2606:4700:4700::1111",
    ] {
        assert!(policy.permits_ip(ip(address)), "{address}");
    }
}

#[test]
fn special_ipv4_destinations_are_denied() {
    let policy = DestinationPolicy::PublicOnly;
    for address in [
        "0.0.0.0",
        "10.1.2.3",
        "100.64.1.2",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.0.0.1",
        "192.0.2.1",
        "192.168.1.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
    ] {
        assert!(!policy.permits_ip(ip(address)), "{address}");
    }
}

#[test]
fn special_ipv6_destinations_are_denied() {
    let policy = DestinationPolicy::PublicOnly;
    for address in [
        "::",
        "::1",
        "::ffff:127.0.0.1",
        "64:ff9b::7f00:1",
        "2001::1",
        "2001:db8::1",
        "2002:c0a8:0101::",
        "3fff::1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
    ] {
        assert!(!policy.permits_ip(ip(address)), "{address}");
    }
}

#[test]
fn tracker_url_requires_http_without_embedded_credentials() {
    let policy = DestinationPolicy::PublicOnly;
    assert!(
        policy
            .validate_tracker_url("https://tracker.example.org/announce")
            .is_ok()
    );
    for url in [
        "udp://tracker.example.org:80/announce",
        "file:///etc/passwd",
        "http://user:pass@tracker.example.org/announce",
        "http://tracker.example.org/#frag",
        "http://127.0.0.1/announce",
        "http://[::1]/announce",
        "http://localhost/announce",
    ] {
        assert!(policy.validate_tracker_url(url).is_err(), "{url}");
    }
    assert!(matches!(
        policy.validate_tracker_url(&"x".repeat(8193)),
        Err(TrackerUrlError::TooLong)
    ));
}
