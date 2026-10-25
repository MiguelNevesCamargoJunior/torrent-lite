//! Offline, bounded input validation. No network or filesystem side effects.

pub mod magnet;
pub mod path_policy;
pub mod torrent_file;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InputError {
    #[error("input exceeds the supported size limit")]
    TooLarge,
    #[error("invalid magnet link")]
    InvalidMagnet,
    #[error("a valid BitTorrent v1 info hash is required")]
    InvalidHash,
    #[error("conflicting torrent identities")]
    ConflictingHash,
    #[error("this torrent format is not supported")]
    UnsupportedFormat,
    #[error("unsafe or unsupported file path")]
    UnsafePath,
    #[error("file paths collide")]
    PathCollision,
    #[error("invalid torrent metadata")]
    InvalidTorrent,
}
