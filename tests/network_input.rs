use torrent_lite::{
    InputError,
    network_input::{sanitize_magnet, sanitize_torrent, sanitize_torrent_with_policy},
    network_policy::DestinationPolicy,
    torrent_file::inspect_torrent,
};

const HASH: &str = "000102030405060708090a0b0c0d0e0f10111213";

fn metainfo(trackers: &[&str], private: bool) -> Vec<u8> {
    let mut info = b"d6:lengthi4e4:name8:file.txt12:piece lengthi4e6:pieces20:".to_vec();
    info.extend([0_u8; 20]);
    if private {
        info.extend(b"7:privatei1e");
    }
    info.push(b'e');
    let mut output = b"d".to_vec();
    if let Some(first) = trackers.first() {
        output.extend(b"8:announce");
        output.extend(format!("{}:{first}", first.len()).as_bytes());
    }
    if !trackers.is_empty() {
        output.extend(b"13:announce-listl");
        for tracker in trackers {
            output.push(b'l');
            output.extend(format!("{}:{tracker}", tracker.len()).as_bytes());
            output.push(b'e');
        }
        output.push(b'e');
    }
    output.extend(b"4:info");
    output.extend(info);
    output.push(b'e');
    output
}

#[test]
fn magnet_discards_unsupported_trackers_and_reports_the_count() {
    let input = format!(
        "magnet:?xt=urn:btih:{HASH}&tr=udp%3A%2F%2Ftracker.example.org%3A80&tr=https%3A%2F%2Ftracker.example.org%2Fannounce"
    );
    let result = sanitize_magnet(&input).unwrap();
    assert_eq!(result.dropped_trackers, 1);
    assert!(result.uri.contains(HASH));
    assert!(result.uri.contains("https"));
    assert!(!result.uri.contains("udp"));
}

#[test]
fn torrent_rewrite_preserves_infohash_and_removes_udp_tracker() {
    let input = metainfo(
        &[
            "udp://tracker.example.org:80",
            "https://tracker.example.org/announce",
        ],
        false,
    );
    let before = inspect_torrent(&input).unwrap();
    let result = sanitize_torrent(&input).unwrap();
    let after = inspect_torrent(&result.bytes).unwrap();
    assert_eq!(before.info_hash, after.info_hash);
    assert_eq!(result.dropped_trackers, 1);
    let decoded = librqbit_core::torrent_metainfo::torrent_from_bytes(&result.bytes).unwrap();
    let trackers: Vec<_> = decoded
        .iter_announce()
        .map(|bytes| std::str::from_utf8(bytes.as_ref()).unwrap())
        .collect();
    assert_eq!(trackers, ["https://tracker.example.org/announce"]);
}

#[test]
fn private_torrent_fails_closed_before_engine_receives_it() {
    let input = metainfo(&["https://tracker.example.org/announce"], true);
    assert_eq!(sanitize_torrent(&input), Err(InputError::UnsupportedFormat));
}

#[test]
fn explicit_lan_mode_can_retain_local_http_tracker() {
    let input = metainfo(&["http://127.0.0.1:8080/announce"], false);
    assert_eq!(sanitize_torrent(&input).unwrap().dropped_trackers, 1);
    let local = sanitize_torrent_with_policy(&input, DestinationPolicy::AllowLan).unwrap();
    assert_eq!(local.dropped_trackers, 0);
    assert_eq!(
        inspect_torrent(&local.bytes).unwrap().info_hash,
        inspect_torrent(&input).unwrap().info_hash
    );
}
