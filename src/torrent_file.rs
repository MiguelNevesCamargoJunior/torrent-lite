use crate::{InputError, path_policy::validate_file_set};
use librqbit_core::torrent_metainfo::torrent_from_bytes;
use serde::Deserialize;

#[derive(Deserialize)]
struct RootVersion {
    info: InfoVersion,
}

#[derive(Deserialize)]
struct InfoVersion {
    #[serde(rename = "meta version")]
    meta_version: Option<u8>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TorrentSummary {
    pub info_hash: String,
    pub name: String,
    pub total_bytes: u64,
    pub files: Vec<TorrentFile>,
    pub private: bool,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TorrentFile {
    pub path: Vec<String>,
    pub bytes: u64,
}

pub fn inspect_torrent(bytes: &[u8]) -> Result<TorrentSummary, InputError> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(InputError::TooLarge);
    }
    preflight_bencode_numbers(bytes)?;
    let marker: RootVersion =
        librqbit_bencode::from_bytes(bytes).map_err(|_| InputError::InvalidTorrent)?;
    if marker.info.meta_version.is_some() {
        return Err(InputError::UnsupportedFormat);
    }
    let meta = torrent_from_bytes(bytes).map_err(|_| InputError::InvalidTorrent)?;
    let raw_info = &meta.info.data;
    let total_length = match (raw_info.length, raw_info.files.as_ref()) {
        (Some(length), None) => length,
        (None, Some(files)) => {
            if files.len() > 10_000 {
                return Err(InputError::TooLarge);
            }
            files
                .iter()
                .try_fold(0_u64, |sum, file| sum.checked_add(file.length))
                .ok_or(InputError::TooLarge)?
        }
        _ => return Err(InputError::InvalidTorrent),
    };
    let info = meta
        .info
        .data
        .validate()
        .map_err(|_| InputError::InvalidTorrent)?;
    let mut files = Vec::new();
    for details in info.iter_file_details() {
        if details.attrs().symlink || details.symlink_path.is_some() {
            return Err(InputError::UnsupportedFormat);
        }
        files.push(TorrentFile {
            path: details.filename.to_vec(),
            bytes: details.len,
        });
        if files.len() > 10_000 {
            return Err(InputError::TooLarge);
        }
    }
    let paths: Vec<Vec<String>> = files.iter().map(|file| file.path.clone()).collect();
    validate_file_set(&paths)?;
    let total_bytes = files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.bytes))
        .ok_or(InputError::TooLarge)?;
    if total_bytes != total_length {
        return Err(InputError::InvalidTorrent);
    }
    Ok(TorrentSummary {
        info_hash: meta.info_hash.as_string(),
        name: info.name().unwrap_or_default().into_owned(),
        total_bytes,
        files,
        private: info.info().private,
    })
}

// A bounded token scan before the upstream deserializer. Its integer fast path
// can panic on values outside signed 64-bit range; bytes inside strings are skipped.
fn preflight_bencode_numbers(bytes: &[u8]) -> Result<(), InputError> {
    let mut cursor = 0usize;
    let mut depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'd' | b'l' => {
                depth += 1;
                if depth > 64 {
                    return Err(InputError::TooLarge);
                }
                cursor += 1;
            }
            b'e' => {
                if depth == 0 {
                    return Err(InputError::InvalidTorrent);
                }
                depth -= 1;
                cursor += 1;
            }
            b'i' => {
                let start = cursor + 1;
                let end = bytes[start..]
                    .iter()
                    .position(|byte| *byte == b'e')
                    .map(|offset| start + offset)
                    .ok_or(InputError::InvalidTorrent)?;
                std::str::from_utf8(&bytes[start..end])
                    .ok()
                    .and_then(|number| number.parse::<i64>().ok())
                    .ok_or(InputError::InvalidTorrent)?;
                cursor = end + 1;
            }
            b'0'..=b'9' => {
                let start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&b':') {
                    return Err(InputError::InvalidTorrent);
                }
                let length = std::str::from_utf8(&bytes[start..cursor])
                    .ok()
                    .and_then(|number| number.parse::<usize>().ok())
                    .ok_or(InputError::InvalidTorrent)?;
                cursor = cursor
                    .checked_add(1)
                    .and_then(|n| n.checked_add(length))
                    .ok_or(InputError::InvalidTorrent)?;
                if cursor > bytes.len() {
                    return Err(InputError::InvalidTorrent);
                }
            }
            _ => return Err(InputError::InvalidTorrent),
        }
    }
    if depth != 0 {
        return Err(InputError::InvalidTorrent);
    }
    Ok(())
}
