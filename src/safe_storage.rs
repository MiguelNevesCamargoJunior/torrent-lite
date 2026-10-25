//! Create new torrent files relative to an already opened directory handle.
//! Existing files need a separate, identity-checked resume path before use.

use crate::path_policy::validate_components;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
use std::{io, path::Path};

pub struct SafeRoot {
    dir: Dir,
}

impl SafeRoot {
    /// The caller explicitly chooses the trusted root. Torrent paths are always relative to it.
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            dir: Dir::open_ambient_dir(path, ambient_authority())?,
        })
    }

    /// Create a file only if it does not already exist, including if a link occupies its name.
    pub fn create_new(&self, components: &[&str]) -> io::Result<File> {
        validate_components(components)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut parent = self.dir.try_clone()?;
        for name in &components[..components.len() - 1] {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            let metadata = parent.symlink_metadata(name)?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "directory link is not allowed",
                ));
            }
            // cap-std resolves against this handle and rejects an escape if a directory
            // is replaced between the metadata check and this open.
            parent = parent.open_dir(name)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        options.follow(FollowSymlinks::No);
        parent.open_with(components[components.len() - 1], &options)
    }
}
