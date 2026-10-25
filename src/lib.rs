//! Bounded input validation and capability-scoped filesystem building blocks.

#[cfg(feature = "durable-engine")]
pub mod engine_storage;
pub mod magnet;
pub mod network_policy;
pub mod path_policy;
#[cfg(feature = "policy-proxy")]
pub mod policy_proxy;
pub mod safe_storage;
#[cfg(feature = "state-store")]
pub mod state_store;
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
