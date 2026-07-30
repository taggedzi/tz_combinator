//! Safe staged file output for first-party application interfaces.

use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{AppError, OutputRecord, OutputSink};

/// Writes encoded records to a sibling temporary file and commits on success.
pub struct FileSink {
    file: Option<File>,
    destination: PathBuf,
    temporary: Option<PathBuf>,
    overwrite: bool,
    committed: bool,
}

impl FileSink {
    /// Opens a staged output destination.
    pub fn open(path: impl AsRef<Path>, overwrite: bool) -> Result<Self, AppError> {
        let destination = path.as_ref().to_path_buf();
        crate::validate_output_path(&destination)
            .map_err(|error| AppError::runtime(error.code, error.message))?;
        let (temporary, file) = create_sibling_temp(&destination)?;
        Ok(Self {
            file: Some(file),
            destination,
            temporary: Some(temporary),
            overwrite,
            committed: false,
        })
    }

    /// Flushes and commits the staged file.
    pub fn commit(mut self) -> Result<(), AppError> {
        self.file
            .as_ref()
            .expect("file remains open until commit")
            .sync_all()
            .map_err(|error| AppError::runtime("WRITE_FAILED", error.to_string()))?;
        self.file.take();
        if let Some(temporary) = self.temporary.take() {
            let result = if self.overwrite {
                replace_file(&temporary, &self.destination)
            } else {
                link_new_file(&temporary, &self.destination)
            };
            if let Err(error) = result {
                let _ = fs::remove_file(&temporary);
                self.committed = true;
                let code = if !self.overwrite && error.kind() == std::io::ErrorKind::AlreadyExists {
                    "OUTPUT_EXISTS"
                } else {
                    "WRITE_FAILED"
                };
                return Err(AppError::runtime(code, error.to_string()));
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl OutputSink for FileSink {
    fn record(&mut self, record: OutputRecord) -> Result<(), AppError> {
        self.file
            .as_mut()
            .expect("file remains open until commit")
            .write_all(record.value.as_bytes())
            .map_err(|error| AppError::runtime("WRITE_FAILED", error.to_string()))
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.file.take();
        if let Some(temporary) = self.temporary.take() {
            let _ = fs::remove_file(temporary);
        }
    }
}

fn create_sibling_temp(destination: &Path) -> Result<(PathBuf, File), AppError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::runtime("WRITE_FAILED", "output path has no valid filename"))?;
    for _ in 0..32 {
        let candidate = parent.join(format!(".{name}.combinator-{:#x}.tmp", random_suffix()?));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AppError::runtime("WRITE_FAILED", error.to_string())),
        }
    }
    Err(AppError::runtime(
        "WRITE_FAILED",
        "could not choose a unique temporary output filename",
    ))
}

fn random_suffix() -> Result<u128, AppError> {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes)
        .map_err(|error| AppError::runtime("WRITE_FAILED", error.to_string()))?;
    Ok(u128::from_le_bytes(bytes))
}

#[cfg(unix)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    File::open("/dev/urandom")?.read_exact(bytes)
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    // SAFETY: bytes points to a valid writable buffer for the duration of the call.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}

fn link_new_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(temporary, destination)?;
    let _ = fs::remove_file(temporary);
    Ok(())
}

#[cfg(unix)]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both paths are NUL-terminated buffers alive for the OS call.
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("combinator-app-{name}-{}", std::process::id()))
    }

    fn record(value: &str) -> OutputRecord {
        OutputRecord {
            ordinal: 0,
            value: value.to_string(),
            fields: Vec::new(),
        }
    }

    #[test]
    fn commits_new_file_and_cleans_uncommitted_file() {
        let path = destination("commit");
        let _ = fs::remove_file(&path);
        {
            let mut sink = FileSink::open(&path, false).unwrap();
            sink.record(record("new")).unwrap();
            sink.commit().unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let _ = fs::remove_file(&path);

        let path = destination("drop");
        let _ = fs::remove_file(&path);
        {
            let mut sink = FileSink::open(&path, false).unwrap();
            sink.record(record("discarded")).unwrap();
        }
        assert!(!path.exists());
    }

    #[test]
    fn overwrite_replaces_existing_file() {
        let path = destination("overwrite");
        let _ = fs::remove_file(&path);
        fs::write(&path, b"old").unwrap();
        let mut sink = FileSink::open(&path, true).unwrap();
        sink.record(record("new")).unwrap();
        sink.commit().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn non_overwrite_commit_rejects_destination_created_after_open() {
        let path = destination("race");
        let _ = fs::remove_file(&path);
        let mut sink = FileSink::open(&path, false).unwrap();
        sink.record(record("new")).unwrap();
        fs::write(&path, b"existing").unwrap();
        assert_eq!(sink.commit().unwrap_err().code, "OUTPUT_EXISTS");
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        let _ = fs::remove_file(&path);
    }
}
