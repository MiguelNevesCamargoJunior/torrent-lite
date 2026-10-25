use torrent_lite::{InputError, torrent_file::inspect_torrent};

fn metainfo(info: &[u8]) -> Vec<u8> {
    let mut result = b"d4:info".to_vec();
    result.extend(info);
    result.push(b'e');
    result
}

fn single(name: &[u8], attrs: &[u8]) -> Vec<u8> {
    let mut info = b"d6:lengthi4e4:name".to_vec();
    info.extend(name.len().to_string().as_bytes());
    info.push(b':');
    info.extend(name);
    info.extend(b"12:piece lengthi4e6:pieces20:");
    info.extend([0_u8; 20]);
    if !attrs.is_empty() {
        info.extend(b"4:attr");
        info.extend(attrs.len().to_string().as_bytes());
        info.push(b':');
        info.extend(attrs);
    }
    info.push(b'e');
    metainfo(&info)
}

#[test]
fn reads_v1_single_file_without_network_or_disk() {
    let value = inspect_torrent(&single(b"file.txt", b"")).unwrap();
    assert_eq!(value.info_hash, "fc13cb96cc1f1dd111edc8c97537d90a7a3feba3");
    assert_eq!(value.name, "file.txt");
    assert_eq!(value.total_bytes, 4);
    assert_eq!(value.files.len(), 1);
    assert_eq!(value.files[0].path, ["file.txt"]);
    assert_eq!(value.files[0].bytes, 4);
    assert!(!value.private);
}

#[test]
fn rejects_unsafe_names_and_symlinks() {
    assert_eq!(
        inspect_torrent(&single(b"CON.txt", b"")),
        Err(InputError::UnsafePath)
    );
    assert_eq!(
        inspect_torrent(&single(b"safe.txt", b"l")),
        Err(InputError::UnsupportedFormat)
    );
}

#[test]
fn rejects_invalid_or_oversize_metadata() {
    assert_eq!(
        inspect_torrent(b"not bencode"),
        Err(InputError::InvalidTorrent)
    );
    assert_eq!(
        inspect_torrent(&vec![0; 8 * 1024 * 1024 + 1]),
        Err(InputError::TooLarge)
    );
}

#[test]
fn rejects_v2_and_hybrid_metadata_explicitly() {
    let mut hybrid = single(b"file.txt", b"");
    hybrid.splice(
        hybrid.len() - 2..hybrid.len() - 2,
        b"12:meta versioni2e".iter().copied(),
    );
    assert_eq!(inspect_torrent(&hybrid), Err(InputError::UnsupportedFormat));

    let v2 = b"d4:infod12:meta versioni2e4:name8:file.txtee";
    assert_eq!(inspect_torrent(v2), Err(InputError::UnsupportedFormat));
}

#[test]
fn reads_multifile_paths_and_private_flag() {
    let mut info = b"d5:filesld6:lengthi2e4:pathl3:dir5:a.txteed6:lengthi2e4:pathl3:dir5:b.txteee4:name4:root12:piece lengthi4e6:pieces20:".to_vec();
    info.extend([0_u8; 20]);
    info.extend(b"7:privatei1ee");
    let value = inspect_torrent(&metainfo(&info)).unwrap();
    assert_eq!(value.name, "root");
    assert_eq!(value.total_bytes, 4);
    assert_eq!(value.files.len(), 2);
    assert_eq!(value.files[0].path, ["dir", "a.txt"]);
    assert_eq!(value.files[1].path, ["dir", "b.txt"]);
    assert!(value.private);
}
