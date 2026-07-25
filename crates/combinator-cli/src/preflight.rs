//! Pure pre-flight validation for file output.

use crate::error::AppError;
use combinator_core::SizeEstimate;

/// Fails if the output file exists and overwrite was not requested.
pub fn check_output_path(path: &str, overwrite: bool) -> Result<(), AppError> {
    if !overwrite && std::path::Path::new(path).exists() {
        return Err(AppError::runtime(
            "OUTPUT_EXISTS",
            "output file already exists; pass --overwrite to replace it",
        )
        .with("path", path));
    }
    Ok(())
}

/// Verifies the estimated output fits within available space and any filesystem limit.
pub fn check_capacity(
    estimate: SizeEstimate,
    available: u64,
    fs_max: Option<u64>,
) -> Result<(), AppError> {
    let bytes = match estimate {
        SizeEstimate::Bytes(b) => b,
        SizeEstimate::Overflow => {
            return Err(AppError::runtime(
                "COUNT_OVERFLOW",
                "estimated output size is too large to represent; cannot verify capacity (use --no-preflight to bypass)",
            ));
        }
    };

    if let Some(max) = fs_max {
        if bytes > max as u128 {
            return Err(AppError::runtime(
                "FILE_SIZE_LIMIT",
                "estimated output exceeds the filesystem's maximum file size",
            )
            .with("estimated_bytes", bytes)
            .with("limit_bytes", max));
        }
    }

    if bytes > available as u128 {
        return Err(AppError::runtime(
            "INSUFFICIENT_SPACE",
            "estimated output exceeds available disk space",
        )
        .with("estimated_bytes", bytes)
        .with("available_bytes", available));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use combinator_core::SizeEstimate;

    #[test]
    fn missing_path_is_ok() {
        assert!(check_output_path("definitely-missing-98765.txt", false).is_ok());
    }

    #[test]
    fn existing_path_without_overwrite_errors() {
        let path = std::env::temp_dir().join("combinator_preflight_exists.txt");
        std::fs::write(&path, "x").unwrap();
        let res = check_output_path(path.to_str().unwrap(), false);
        let overwrite_ok = check_output_path(path.to_str().unwrap(), true).is_ok();
        std::fs::remove_file(&path).ok();
        assert_eq!(res.unwrap_err().code, "OUTPUT_EXISTS");
        assert!(overwrite_ok);
    }

    #[test]
    fn fits_when_estimate_below_available() {
        assert!(check_capacity(SizeEstimate::Bytes(100), 1000, None).is_ok());
    }

    #[test]
    fn insufficient_space_errors() {
        let e = check_capacity(SizeEstimate::Bytes(2000), 1000, None).unwrap_err();
        assert_eq!(e.code, "INSUFFICIENT_SPACE");
    }

    #[test]
    fn fs_max_exceeded_errors() {
        let e = check_capacity(
            SizeEstimate::Bytes(5_000_000_000),
            u64::MAX,
            Some(4_294_967_296),
        )
        .unwrap_err();
        assert_eq!(e.code, "FILE_SIZE_LIMIT");
    }

    #[test]
    fn overflow_estimate_cannot_verify() {
        let e = check_capacity(SizeEstimate::Overflow, u64::MAX, None).unwrap_err();
        assert_eq!(e.code, "COUNT_OVERFLOW");
    }
}
