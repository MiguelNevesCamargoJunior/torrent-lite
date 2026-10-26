#![no_main]

use libfuzzer_sys::fuzz_target;
use torrent_lite::{
    magnet::Magnet,
    network_input::{sanitize_magnet, sanitize_torrent},
    torrent_file::inspect_torrent,
};

fuzz_target!(|data: &[u8]| {
    let Some((&kind, input)) = data.split_first() else {
        return;
    };
    if kind & 1 == 0 {
        let inspected = inspect_torrent(input);
        if let Ok(sanitized) = sanitize_torrent(input) {
            let original = inspected.expect("sanitizer accepted uninspectable torrent");
            let output =
                inspect_torrent(&sanitized.bytes).expect("sanitizer emitted invalid torrent");
            assert_eq!(original.info_hash, output.info_hash);
            assert_eq!(original.files, output.files);
        }
    } else if let Ok(input) = std::str::from_utf8(input) {
        let parsed = Magnet::parse(input);
        if let Ok(sanitized) = sanitize_magnet(input) {
            let original = parsed.expect("sanitizer accepted invalid magnet");
            let output = Magnet::parse(&sanitized.uri).expect("sanitizer emitted invalid magnet");
            assert_eq!(original.info_hash(), output.info_hash());
        }
    }
});
