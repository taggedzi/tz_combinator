//! Filesystem checks for output paths.

use std::fs;
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPathError {
    pub code: &'static str,
    pub message: String,
}

fn path_error(code: &'static str, message: impl Into<String>) -> OutputPathError {
    OutputPathError {
        code,
        message: message.into(),
    }
}

/// Rejects output paths whose destination or existing parent components are
/// symbolic links/reparse points.
///
/// Output writers do not create parent directories, so requiring the parent
/// to exist also closes the gap where an attacker could create a missing
/// ancestor after validation and redirect the temporary file.
pub fn validate_output_path(path: &Path) -> Result<(), OutputPathError> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(path_error(
            "UNSAFE_OUTPUT_PATH",
            "output paths may not contain parent-directory traversal",
        ));
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if is_unsafe_target(&metadata) => {
            return Err(path_error(
                "UNSAFE_OUTPUT_PATH",
                "refusing to use a symbolic link or reparse point as output",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(path_error(
                "WRITE_FAILED",
                format!("could not inspect output path: {error}"),
            ));
        }
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    validate_parent_components(parent, false)
}

/// Creates missing output parent components one at a time while checking each
/// component for symlink/reparse-point redirection.
pub fn ensure_output_parent(path: &Path) -> Result<(), OutputPathError> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(path_error(
            "UNSAFE_OUTPUT_PATH",
            "output paths may not contain parent-directory traversal",
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    validate_parent_components(parent, true)
}

fn validate_parent_components(parent: &Path, create_missing: bool) -> Result<(), OutputPathError> {
    let mut current = if parent.is_absolute() {
        Path::new("").to_path_buf()
    } else {
        Path::new(".").to_path_buf()
    };
    for component in parent.components() {
        current.push(component.as_os_str());
        let metadata = loop {
            match fs::symlink_metadata(&current) {
                Ok(metadata) => break metadata,
                Err(error) if create_missing && error.kind() == std::io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(error) => {
                            return Err(path_error(
                                "WRITE_FAILED",
                                format!("could not create output parent: {error}"),
                            ));
                        }
                    }
                }
                Err(error) => {
                    return Err(path_error(
                        "WRITE_FAILED",
                        format!("could not inspect output parent: {error}"),
                    ));
                }
            }
        };
        if is_unsafe_target(&metadata) {
            return Err(path_error(
                "UNSAFE_OUTPUT_PATH",
                "refusing to use a symbolic-link or reparse-point output directory",
            ));
        }
        if !metadata.is_dir() {
            return Err(path_error(
                "WRITE_FAILED",
                "an output path component is not a directory",
            ));
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn is_unsafe_target(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_unsafe_target(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_nested_and_dangling_symlink_ancestors() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("combinator-path-safety-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("real/nested")).unwrap();
        symlink(root.join("real"), root.join("linked")).unwrap();
        symlink(root.join("missing"), root.join("dangling")).unwrap();

        assert_eq!(
            validate_output_path(&root.join("linked/nested/output.txt"))
                .unwrap_err()
                .code,
            "UNSAFE_OUTPUT_PATH"
        );
        assert_eq!(
            validate_output_path(&root.join("dangling/output.txt"))
                .unwrap_err()
                .code,
            "UNSAFE_OUTPUT_PATH"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_traversal_and_missing_parent() {
        let root = std::env::temp_dir().join(format!(
            "combinator-path-safety-basic-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        assert_eq!(
            validate_output_path(&root.join("..").join("outside.txt"))
                .unwrap_err()
                .code,
            "UNSAFE_OUTPUT_PATH"
        );
        assert_eq!(
            validate_output_path(&root.join("missing/output.txt"))
                .unwrap_err()
                .code,
            "WRITE_FAILED"
        );

        ensure_output_parent(&root.join("created/nested/preferences.json")).unwrap();
        assert!(root.join("created/nested").is_dir());

        fs::remove_dir_all(root).unwrap();
    }
}
