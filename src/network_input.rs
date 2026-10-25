//! Rewrites untrusted sources before the engine can see their tracker URLs.

use crate::{
    InputError, magnet::Magnet, network_policy::DestinationPolicy, torrent_file::inspect_torrent,
};
use librqbit_bencode::{bencode_serialize_to_writer, raw_value::RawValue};
use librqbit_core::torrent_metainfo::torrent_from_bytes;
use serde::Serialize;
use url::Url;

const MAX_TRACKERS: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub struct SanitizedMagnet {
    pub uri: String,
    pub dropped_trackers: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SanitizedTorrent {
    pub bytes: Vec<u8>,
    pub dropped_trackers: usize,
}

pub fn sanitize_magnet(input: &str) -> Result<SanitizedMagnet, InputError> {
    sanitize_magnet_with_policy(input, DestinationPolicy::PublicOnly)
}

pub fn sanitize_magnet_with_policy(
    input: &str,
    policy: DestinationPolicy,
) -> Result<SanitizedMagnet, InputError> {
    let magnet = Magnet::parse(input)?;
    let source = Url::parse(input).map_err(|_| InputError::InvalidMagnet)?;
    let mut uri = Url::parse(&format!("magnet:?xt=urn:btih:{}", magnet.info_hash()))
        .map_err(|_| InputError::InvalidMagnet)?;
    let mut count = 0;
    let mut dropped_trackers = 0;
    for (key, value) in source.query_pairs() {
        if key != "tr" {
            continue;
        }
        count += 1;
        if count > MAX_TRACKERS {
            return Err(InputError::TooLarge);
        }
        match policy.validate_tracker_url(&value) {
            Ok(tracker) => {
                uri.query_pairs_mut().append_pair("tr", tracker.as_str());
            }
            Err(_) => dropped_trackers += 1,
        }
    }
    let uri = uri.to_string();
    if uri.len() > 16 * 1024 {
        return Err(InputError::TooLarge);
    }
    Ok(SanitizedMagnet {
        uri,
        dropped_trackers,
    })
}

#[derive(Serialize)]
struct SafeMetainfo<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    announce: Option<&'a str>,
    #[serde(rename = "announce-list", skip_serializing_if = "Vec::is_empty")]
    announce_list: Vec<Vec<&'a str>>,
    info: RawValue<&'a [u8]>,
}

pub fn sanitize_torrent(input: &[u8]) -> Result<SanitizedTorrent, InputError> {
    sanitize_torrent_with_policy(input, DestinationPolicy::PublicOnly)
}

pub fn sanitize_torrent_with_policy(
    input: &[u8],
    policy: DestinationPolicy,
) -> Result<SanitizedTorrent, InputError> {
    let before = inspect_torrent(input)?;
    let parsed = torrent_from_bytes(input).map_err(|_| InputError::InvalidTorrent)?;
    let mut safe = Vec::new();
    let mut dropped_trackers = 0;
    for tracker in parsed.iter_announce() {
        if safe.len() + dropped_trackers >= MAX_TRACKERS {
            return Err(InputError::TooLarge);
        }
        let Ok(tracker) = std::str::from_utf8(tracker.as_ref()) else {
            dropped_trackers += 1;
            continue;
        };
        match policy.validate_tracker_url(tracker) {
            Ok(url) => safe.push(url.to_string()),
            Err(_) => dropped_trackers += 1,
        }
    }
    if before.private && safe.len() != 1 {
        return Err(InputError::UnsupportedFormat);
    }
    let safe_refs: Vec<Vec<&str>> = safe.iter().map(|url| vec![url.as_str()]).collect();
    let mut bytes = Vec::with_capacity(input.len());
    bencode_serialize_to_writer(
        &SafeMetainfo {
            announce: safe.first().map(String::as_str),
            announce_list: safe_refs,
            info: RawValue(parsed.info.raw_bytes.as_ref()),
        },
        &mut bytes,
    )
    .map_err(|_| InputError::InvalidTorrent)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(InputError::TooLarge);
    }
    let after = inspect_torrent(&bytes)?;
    if after.info_hash != before.info_hash {
        return Err(InputError::InvalidTorrent);
    }
    Ok(SanitizedTorrent {
        bytes,
        dropped_trackers,
    })
}
