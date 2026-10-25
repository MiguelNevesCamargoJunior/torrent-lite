//! Owner-scoped endpoint handoff for the one process holding the state lock.

use crate::local_api::LocalApi;
use data_encoding::BASE64;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

const MAX_RECORD_BYTES: u64 = 4096;
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    port: u16,
    generation: String,
    protected_token: String,
}

pub struct EndpointInfo {
    pub addr: SocketAddr,
    pub token: String,
}

pub struct EndpointLease {
    path: PathBuf,
    generation: String,
}

impl EndpointLease {
    pub fn publish(database: &Path, api: &LocalApi) -> io::Result<Self> {
        let path = endpoint_path(database)?;
        let generation = uuid::Uuid::new_v4().to_string();
        let token = protect_token(api.token().as_bytes())?;
        let record = Record {
            version: VERSION,
            port: api.local_addr().port(),
            generation: generation.clone(),
            protected_token: BASE64.encode(&token),
        };
        let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "endpoint too large",
            ));
        }
        let mut temporary_name = path
            .file_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "endpoint path has no file name",
                )
            })?
            .to_os_string();
        temporary_name.push(format!(".tmp-{generation}"));
        let temporary = path.with_file_name(temporary_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let write_result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = (|| -> io::Result<()> {
            #[cfg(windows)]
            if path.symlink_metadata().is_ok() {
                fs::remove_file(&path)?;
            }
            fs::rename(&temporary, &path)?;
            Ok(())
        })() {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(Self { path, generation })
    }

    pub fn read(database: &Path) -> io::Result<EndpointInfo> {
        let path = endpoint_path(database)?;
        let record = read_record(&path)?;
        if record.version != VERSION || record.port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported endpoint",
            ));
        }
        let encrypted = BASE64
            .decode(record.protected_token.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endpoint token"))?;
        let token = String::from_utf8(unprotect_token(&encrypted)?)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endpoint token"))?;
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid endpoint token",
            ));
        }
        Ok(EndpointInfo {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, record.port)),
            token,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EndpointLease {
    fn drop(&mut self) {
        if read_record(&self.path).is_ok_and(|record| record.generation == self.generation) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn endpoint_path(database: &Path) -> io::Result<PathBuf> {
    let mut name = database
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "database path has no file name",
            )
        })?
        .to_os_string();
    name.push(".endpoint");
    Ok(database.with_file_name(name))
}

fn read_record(path: &Path) -> io::Result<Record> {
    let metadata = path.symlink_metadata()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid endpoint file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "endpoint is not private",
            ));
        }
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint too large",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endpoint record"))
}

#[cfg(not(windows))]
fn protect_token(token: &[u8]) -> io::Result<Vec<u8>> {
    Ok(token.to_vec())
}

#[cfg(not(windows))]
fn unprotect_token(token: &[u8]) -> io::Result<Vec<u8>> {
    Ok(token.to_vec())
}

#[cfg(windows)]
fn protect_token(token: &[u8]) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData},
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: token.len() as u32,
        pbData: token.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input lives through the call; DPAPI allocates output for LocalFree.
    let success = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful DPAPI call returns cbData bytes at pbData.
    let encrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    // SAFETY: output was allocated by DPAPI using LocalAlloc.
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(encrypted)
}

#[cfg(windows)]
fn unprotect_token(token: &[u8]) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: token.len() as u32,
        pbData: token.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input lives through the call; DPAPI allocates output for LocalFree.
    let success = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful DPAPI call returns cbData bytes at pbData.
    let plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    // SAFETY: write_volatile clears the API allocation before LocalFree.
    unsafe {
        for index in 0..output.cbData as usize {
            std::ptr::write_volatile(output.pbData.add(index), 0);
        }
        LocalFree(output.pbData.cast());
    }
    Ok(plaintext)
}
