//! Open torrent files relative to an already opened directory handle.

use crate::path_policy::validate_components;
#[cfg(unix)]
use cap_fs_ext::MetadataExt;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Persist this value with the torrent's state before reusing any existing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity {
    device: u64,
    file_id: [u8; 16],
}

impl FileIdentity {
    #[cfg(feature = "state-store")]
    pub(crate) fn to_record_bytes(self) -> [u8; 24] {
        let mut bytes = [0_u8; 24];
        bytes[..8].copy_from_slice(&self.device.to_le_bytes());
        bytes[8..].copy_from_slice(&self.file_id);
        bytes
    }

    #[cfg(feature = "state-store")]
    pub(crate) fn from_record_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 24 || bytes[8..].iter().all(|value| *value == 0) {
            return None;
        }
        let device = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        let mut file_id = [0_u8; 16];
        file_id.copy_from_slice(&bytes[8..]);
        Some(Self { device, file_id })
    }

    pub fn from_file(file: &File) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 || metadata.ino() == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsupported file identity",
                ));
            }
            let mut file_id = [0_u8; 16];
            file_id[..8].copy_from_slice(&metadata.ino().to_le_bytes());
            Ok(Self {
                device: metadata.dev(),
                file_id,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
            };
            let metadata = file.metadata()?;
            let information = winx::winapi_util::file::information(file.try_clone()?.into_std())?;
            let mut id_info = FILE_ID_INFO::default();
            // The file handle stays open, and the mutable buffer has the exact Win32 size.
            let success = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileIdInfo,
                    (&raw mut id_info).cast(),
                    std::mem::size_of::<FILE_ID_INFO>() as u32,
                )
            };
            if success == 0 {
                return Err(io::Error::last_os_error());
            }
            let file_id = id_info.FileId.Identifier;
            if !metadata.is_file() || information.number_of_links() != 1 || file_id == [0; 16] {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsupported file identity",
                ));
            }
            Ok(Self {
                device: id_info.VolumeSerialNumber,
                file_id,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = file;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file identity is unsupported on this platform",
            ))
        }
    }
}

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

    /// Create or reopen one child directory without resolving a new ambient path.
    pub fn subdir(&self, name: &str) -> io::Result<Self> {
        validate_components(&[name])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        match self.dir.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = self.dir.symlink_metadata(name)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "directory link is not allowed",
            ));
        }
        Ok(Self {
            dir: self.dir.open_dir_nofollow(name)?,
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
            // Reject a link even if it replaces the directory after the metadata check.
            parent = parent.open_dir_nofollow(name)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        options.follow(FollowSymlinks::No);
        parent.open_with(components[components.len() - 1], &options)
    }

    /// Reopen a previously created file only when its handle still has the recorded identity.
    pub fn open_existing(&self, components: &[&str], identity: FileIdentity) -> io::Result<File> {
        validate_components(components)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let relative = components.iter().fold(PathBuf::new(), |mut path, name| {
            path.push(name);
            path
        });
        let mut options = OpenOptions::new();
        options.read(true).write(true).follow(FollowSymlinks::No);
        let file = self.dir.open_with(relative, &options)?;
        if FileIdentity::from_file(&file)? != identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file identity changed",
            ));
        }
        Ok(file)
    }
}
