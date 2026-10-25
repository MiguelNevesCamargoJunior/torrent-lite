use crate::{InputError, path_policy::validate_file_set};
use librqbit_core::torrent_metainfo::torrent_from_bytes;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct TorrentSummary {
    pub info_hash: String,
    pub name: String,
    pub total_bytes: u64,
    pub files: Vec<TorrentFile>,
    pub private: bool,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct TorrentFile {
    pub path: Vec<String>,
    pub bytes: u64,
}

pub fn inspect_torrent(bytes: &[u8]) -> Result<TorrentSummary, InputError> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(InputError::TooLarge);
    }
    let meta = torrent_from_bytes(bytes).map_err(|_| InputError::InvalidTorrent)?;
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
    Ok(TorrentSummary {
        info_hash: meta.info_hash.as_string(),
        name: info.name().unwrap_or_default().into_owned(),
        total_bytes,
        files,
        private: info.info().private,
    })
}
