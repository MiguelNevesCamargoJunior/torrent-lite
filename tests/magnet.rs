use torrent_lite::{InputError, magnet::Magnet};

const HASH: &str = "000102030405060708090a0b0c0d0e0f10111213";

#[test]
fn hexadecimal_identity_is_normalized_without_exposing_trackers() {
    let value = Magnet::parse("magnet:?xt=urn:btih:000102030405060708090A0B0C0D0E0F10111213&dn=Example+file&tr=https%3A%2F%2Fsecret.example").unwrap();
    assert_eq!(value.info_hash(), HASH);
    assert_eq!(value.display_name.as_deref(), Some("Example file"));
    assert!(!serde_json::to_string(&value).unwrap().contains("secret"));
}

#[test]
fn base32_and_encoded_topics_resolve_to_the_same_identity() {
    for topic in [
        "urn:btih:AAAQEAYEAUDAOCAJBIFQYDIOB4IBCEQT",
        "urn:btih:aaaqeayeaudaocajbifqydiob4ibceqt",
        "urn%3Abtih%3A000102030405060708090a0b0c0d0e0f10111213",
    ] {
        assert_eq!(
            Magnet::parse(&format!("magnet:?xt={topic}"))
                .unwrap()
                .info_hash(),
            HASH
        );
    }
}

#[test]
fn identical_topics_are_idempotent_but_conflicts_are_rejected() {
    assert!(Magnet::parse(&format!("magnet:?xt=urn:btih:{HASH}&xt=urn:btih:{HASH}")).is_ok());
    assert_eq!(
        Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{HASH}&xt=urn:btih:{}",
            "f".repeat(40)
        )),
        Err(InputError::ConflictingHash)
    );
}

#[test]
fn v2_and_hybrid_are_explicitly_unsupported() {
    for uri in [
        "magnet:?xt=urn:btmh:1220abcd".to_owned(),
        format!("magnet:?xt=urn:btih:{HASH}&xt=urn:btmh:1220abcd"),
    ] {
        assert_eq!(Magnet::parse(&uri), Err(InputError::UnsupportedFormat));
    }
}

#[test]
fn malformed_or_missing_identity_is_rejected() {
    for uri in [
        "magnet:?dn=name",
        "magnet:?xt=urn:btih:short",
        "magnet:?xt=urn:btih:GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
        "magnet:?xt=urn:btih:!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
    ] {
        assert_eq!(Magnet::parse(uri), Err(InputError::InvalidHash), "{uri}");
    }
}

#[test]
fn malformed_url_shapes_are_not_silently_normalized() {
    for uri in [
        format!("https://example.test/?xt=urn:btih:{HASH}"),
        format!("magnet://host?xt=urn:btih:{HASH}"),
        format!("magnet:path?xt=urn:btih:{HASH}"),
        format!(" magnet:?xt=urn:btih:{HASH}"),
        format!("magnet:?xt=urn:btih:{HASH}\n"),
        format!("magnet:?xt=urn:btih:{HASH}#fragment"),
    ] {
        assert_eq!(Magnet::parse(&uri), Err(InputError::InvalidMagnet));
    }
}

#[test]
fn input_size_is_bounded_before_parsing() {
    let prefix = format!("magnet:?xt=urn:btih:{HASH}&dn=");
    let boundary = format!("{prefix}{}", "a".repeat(16 * 1024 - prefix.len()));
    assert!(Magnet::parse(&boundary).is_ok());
    assert_eq!(Magnet::parse(&(boundary + "a")), Err(InputError::TooLarge));
}
