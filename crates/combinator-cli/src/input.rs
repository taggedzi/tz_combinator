//! Gathering lists from inline, file, and stdin sources; delimiter validation.

use crate::error::AppError;

pub const MAX_DELIM_BYTES: usize = 4096;

/// Validates the three delimiters. All three respect the byte cap; the inline
/// list delimiter must additionally be non-empty.
pub fn validate_delims(field_sep: &str, rec_sep: &str, list_delim: &str) -> Result<(), AppError> {
    for (name, d) in [("--sep", field_sep), ("--rec-sep", rec_sep), ("--list-delim", list_delim)] {
        if d.len() > MAX_DELIM_BYTES {
            return Err(AppError::usage(
                "BAD_DELIMITER",
                format!("{name} exceeds the {MAX_DELIM_BYTES}-byte limit"),
            )
            .with("flag", name)
            .with("bytes", d.len()));
        }
    }
    if list_delim.is_empty() {
        return Err(AppError::usage(
            "BAD_DELIMITER",
            "--list-delim must not be empty",
        ));
    }
    Ok(())
}

/// Splits an inline `--list` value on a non-empty delimiter.
pub fn split_inline(value: &str, delim: &str) -> Vec<String> {
    value.split(delim).map(|s| s.to_string()).collect()
}

/// Reads a file as a list, one item per line, stripping a trailing `\r`.
/// The path `-` reads standard input instead (explicit stdin only).
pub fn read_file_list(path: &str) -> Result<Vec<String>, AppError> {
    let content = if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read stdin: {e}"))
                .with("path", "-")
        })?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?
    };
    Ok(split_lines(&content))
}

fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(|l| l.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_delims() {
        assert!(validate_delims("", "\n", ",").is_ok());
    }

    #[test]
    fn rejects_empty_list_delim() {
        let e = validate_delims("", "\n", "").unwrap_err();
        assert_eq!(e.code, "BAD_DELIMITER");
        assert_eq!(e.exit, 2);
    }

    #[test]
    fn rejects_oversized_delim() {
        let big = "x".repeat(MAX_DELIM_BYTES + 1);
        let e = validate_delims(&big, "\n", ",").unwrap_err();
        assert_eq!(e.code, "BAD_DELIMITER");
    }

    #[test]
    fn splits_inline_on_comma() {
        assert_eq!(split_inline("red,blue,green", ","), vec!["red", "blue", "green"]);
    }

    #[test]
    fn splits_inline_on_custom_delim() {
        assert_eq!(split_inline("a::b", "::"), vec!["a", "b"]);
    }

    #[test]
    fn read_missing_file_errors() {
        let e = read_file_list("does-not-exist-12345.txt").unwrap_err();
        assert_eq!(e.code, "FILE_UNREADABLE");
        assert_eq!(e.exit, 1);
    }

    #[test]
    fn file_lines_strip_crlf() {
        // Written and read back via a temp file.
        let dir = std::env::temp_dir();
        let path = dir.join("combinator_test_crlf.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let got = read_file_list(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }
}
