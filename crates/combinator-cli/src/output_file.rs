//! Secure output-file creation and replacement.

use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Owns an output file and commits it only after successful generation.
pub struct OutputFile {
    file: Option<File>,
    destination: PathBuf,
    temporary: Option<PathBuf>,
    committed: bool,
}

impl OutputFile {
    /// Opens a new destination exclusively, or stages an overwrite in a
    /// sibling temporary file for atomic replacement.
    pub fn open(path: &str, overwrite: bool) -> Result<Self, AppError> {
        let destination = PathBuf::from(path);
        if !overwrite {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|e| {
                    let code = if e.kind() == std::io::ErrorKind::AlreadyExists {
                        "OUTPUT_EXISTS"
                    } else {
                        "WRITE_FAILED"
                    };
                    AppError::runtime(code, format!("could not create output file: {e}"))
                        .with("path", path)
                })?;
            return Ok(Self {
                file: Some(file),
                destination,
                temporary: None,
                committed: false,
            });
        }

        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if is_unsafe_target(&metadata) {
                return Err(AppError::runtime(
                    "UNSAFE_OUTPUT_PATH",
                    "refusing to overwrite a symbolic link or reparse point",
                )
                .with("path", path));
            }
        }

        let (temporary, file) = create_sibling_temp(&destination)?;
        Ok(Self {
            file: Some(file),
            destination,
            temporary: Some(temporary),
            committed: false,
        })
    }

    pub fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("output file remains open until commit")
    }

    /// Commits the completed output. A staged overwrite is replaced atomically.
    pub fn commit(mut self) -> Result<(), AppError> {
        self.file
            .as_ref()
            .expect("output file remains open until commit")
            .sync_all()
            .map_err(|e| {
                AppError::runtime("WRITE_FAILED", format!("could not sync output file: {e}"))
                    .with("path", self.destination.display())
            })?;
        self.file.take();
        if let Some(temporary) = self.temporary.take() {
            if let Err(e) = replace_file(&temporary, &self.destination) {
                let _ = fs::remove_file(&temporary);
                self.committed = true;
                return Err(
                    AppError::runtime("WRITE_FAILED", format!("could not commit output file: {e}"))
                        .with("path", self.destination.display()),
                );
            }
        }
        self.committed = true;
        Ok(())
    }
}

#[cfg(not(windows))]
fn is_unsafe_target(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_unsafe_target(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

impl Drop for OutputFile {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.file.take();
        if let Some(temporary) = self.temporary.take() {
            let _ = fs::remove_file(temporary);
        } else {
            let _ = fs::remove_file(&self.destination);
        }
    }
}

fn create_sibling_temp(destination: &Path) -> Result<(PathBuf, File), AppError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::runtime("WRITE_FAILED", "output path has no valid filename"))?;

    for _ in 0..32 {
        let suffix = random_suffix()?;
        let candidate = parent.join(format!(".{name}.combinator-{suffix}.tmp"));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(AppError::runtime(
                    "WRITE_FAILED",
                    format!("could not create temporary output file: {e}"),
                )
                .with("path", destination.display()))
            }
        }
    }

    Err(AppError::runtime(
        "WRITE_FAILED",
        "could not choose a unique temporary output filename",
    )
    .with("path", destination.display()))
}

fn random_suffix() -> Result<String, AppError> {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes).map_err(|e| {
        AppError::runtime("WRITE_FAILED", format!("could not generate a secure temporary filename: {e}"))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(unix)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    let mut source = File::open("/dev/urandom")?;
    source.read_exact(bytes)
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    // SAFETY: the destination is a valid writable byte slice for the duration
    // of the OS call, and the system-preferred RNG does not require a handle.
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
    let target: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both buffers are NUL-terminated UTF-16 paths that remain alive
    // for the duration of the OS call; the flags request replacement and flush.
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
