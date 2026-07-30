//! Secure output-file creation and replacement.

use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use combinator_app::validate_output_path;

/// Owns an output file and commits it only after successful generation.
pub struct OutputFile {
    file: Option<File>,
    destination: PathBuf,
    temporary: Option<PathBuf>,
    overwrite: bool,
    committed: bool,
}

impl OutputFile {
    /// Opens a new destination exclusively, or stages an overwrite in a
    /// sibling temporary file for atomic replacement.
    pub fn open(path: &str, overwrite: bool) -> Result<Self, AppError> {
        let destination = PathBuf::from(path);
        validate_output_path(&destination)
            .map_err(|error| AppError::runtime(error.code, error.message).with("path", path))?;

        let (temporary, file) = create_sibling_temp(&destination)?;
        Ok(Self {
            file: Some(file),
            destination,
            temporary: Some(temporary),
            overwrite,
            committed: false,
        })
    }

    pub fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("output file remains open until commit")
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
            let result = if self.overwrite {
                replace_file(&temporary, &self.destination)
            } else {
                link_new_file(&temporary, &self.destination)
            };
            if let Err(e) = result {
                let _ = fs::remove_file(&temporary);
                self.committed = true;
                let code = if !self.overwrite && e.kind() == std::io::ErrorKind::AlreadyExists {
                    "OUTPUT_EXISTS"
                } else {
                    "WRITE_FAILED"
                };
                return Err(
                    AppError::runtime(code, format!("could not commit output file: {e}"))
                        .with("path", self.destination.display()),
                );
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for OutputFile {
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
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::runtime("WRITE_FAILED", "output path has no valid filename"))?;

    for _ in 0..32 {
        let suffix = random_suffix()?;
        let candidate = parent.join(format!(".{name}.combinator-{suffix}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
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

fn link_new_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(temporary, destination)?;
    let _ = fs::remove_file(temporary);
    Ok(())
}

fn random_suffix() -> Result<String, AppError> {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes).map_err(|e| {
        AppError::runtime(
            "WRITE_FAILED",
            format!("could not generate a secure temporary filename: {e}"),
        )
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
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("combinator-output-{name}-{}", std::process::id()))
    }

    #[test]
    fn commits_new_file_and_discards_uncommitted_file() {
        let destination = path("commit");
        let _ = fs::remove_file(&destination);
        {
            let mut output = OutputFile::open(destination.to_str().unwrap(), false).unwrap();
            output.file_mut().write_all(b"new").unwrap();
            output.commit().unwrap();
        }
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        let _ = fs::remove_file(&destination);

        let destination = path("drop");
        let _ = fs::remove_file(&destination);
        {
            let mut output = OutputFile::open(destination.to_str().unwrap(), false).unwrap();
            output.file_mut().write_all(b"discarded").unwrap();
        }
        assert!(!destination.exists());
    }

    #[test]
    fn overwrite_replaces_existing_file() {
        let destination = path("overwrite");
        let _ = fs::remove_file(&destination);
        fs::write(&destination, b"old").unwrap();
        let mut output = OutputFile::open(destination.to_str().unwrap(), true).unwrap();
        output.file_mut().write_all(b"new").unwrap();
        output.commit().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        let _ = fs::remove_file(&destination);
    }

    #[test]
    fn non_overwrite_commit_rejects_destination_created_after_open() {
        let destination = path("race");
        let _ = fs::remove_file(&destination);
        let mut output = OutputFile::open(destination.to_str().unwrap(), false).unwrap();
        output.file_mut().write_all(b"new").unwrap();
        fs::write(&destination, b"existing").unwrap();
        let error = output.commit().unwrap_err();
        assert_eq!(error.code, "OUTPUT_EXISTS");
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        let _ = fs::remove_file(&destination);
    }

    #[test]
    fn commit_to_directory_fails_without_replacing_it() {
        let destination =
            std::env::temp_dir().join(format!("combinator-output-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&destination);
        fs::create_dir(&destination).unwrap();
        let mut output = OutputFile::open(destination.to_str().unwrap(), false).unwrap();
        output.file_mut().write_all(b"new").unwrap();
        let error = output.commit().unwrap_err();
        assert!(matches!(error.code, "OUTPUT_EXISTS" | "WRITE_FAILED"));
        assert!(destination.is_dir());
        let _ = fs::remove_dir_all(destination);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_destination_and_parent() {
        use std::os::unix::fs::symlink;

        let target = path("symlink-target");
        let destination = path("symlink-destination");
        let parent = path("symlink-parent");
        let nested = parent.join("output.txt");
        let _ = fs::remove_file(&target);
        let _ = fs::remove_file(&destination);
        let _ = fs::remove_file(&nested);
        let _ = fs::remove_dir(&parent);
        fs::write(&target, b"target").unwrap();
        symlink(&target, &destination).unwrap();
        assert_eq!(
            OutputFile::open(destination.to_str().unwrap(), true)
                .err()
                .unwrap()
                .code,
            "UNSAFE_OUTPUT_PATH"
        );
        fs::create_dir(&parent).unwrap();
        let real_parent = path("symlink-real-parent");
        let _ = fs::remove_dir(&real_parent);
        fs::create_dir(&real_parent).unwrap();
        let link_parent = path("symlink-link-parent");
        let _ = fs::remove_file(&link_parent);
        symlink(&real_parent, &link_parent).unwrap();
        let linked_output = link_parent.join("output.txt");
        assert_eq!(
            OutputFile::open(linked_output.to_str().unwrap(), false)
                .err()
                .unwrap()
                .code,
            "UNSAFE_OUTPUT_PATH"
        );
        let _ = fs::remove_file(destination);
        let _ = fs::remove_file(link_parent);
        let _ = fs::remove_dir(real_parent);
        let _ = fs::remove_dir(parent);
        let _ = fs::remove_file(target);
    }
}
