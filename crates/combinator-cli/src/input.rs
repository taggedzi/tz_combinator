//! CLI filesystem adapter for core's generic bounded readers.

use crate::cli::InputFormat;
use crate::error::AppError;
use std::io::Read;

pub const MAX_DELIM_BYTES: usize = 4096;
pub const MAX_TEMPLATE_BYTES: usize = 1024 * 1024;
pub use combinator_core::input::{InputBudget, InputLimits};

pub fn validate_delims(field_sep: &str, rec_sep: &str, list_delim: &str) -> Result<(), AppError> {
    for (name, d) in [
        ("--sep", field_sep),
        ("--rec-sep", rec_sep),
        ("--list-delim", list_delim),
    ] {
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
pub fn split_inline_bounded(
    v: &str,
    d: &str,
    l: InputLimits,
    b: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    combinator_core::input::split_inline(v, d, l, b)
}
pub fn split_escaped_inline_bounded(
    v: &str,
    d: &str,
    l: InputLimits,
    b: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    combinator_core::input::split_escaped_inline(v, d, l, b)
}
pub fn read_file_list_bounded(
    path: &str,
    l: InputLimits,
    b: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if path == "-" {
        combinator_core::input::read_lines(std::io::stdin().lock(), path, l, b)
    } else {
        let f = std::fs::File::open(path).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?;
        combinator_core::input::read_lines(f, path, l, b)
    }
}
pub fn read_file_list_format_bounded(
    path: &str,
    format: InputFormat,
    l: InputLimits,
    b: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if format == InputFormat::Inline {
        return Err(AppError::usage(
            "INPUT_FORMAT_INVALID",
            "inline input format requires --list",
        ));
    }
    if format == InputFormat::Lines {
        return read_file_list_bounded(path, l, b);
    }
    let f: Box<dyn Read> = if path == "-" {
        Box::new(std::io::stdin().lock())
    } else {
        Box::new(std::fs::File::open(path).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?)
    };
    combinator_core::input::read_formatted(f, path, format.into(), l, b)
}
pub fn read_template_bounded(path: &str, max: usize) -> Result<String, AppError> {
    let f = std::fs::File::open(path).map_err(|e| {
        AppError::usage(
            "TEMPLATE_FILE_UNREADABLE",
            format!("could not read template file: {e}"),
        )
        .with("path", path)
    })?;
    let mut bytes = Vec::new();
    f.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            AppError::usage(
                "TEMPLATE_FILE_UNREADABLE",
                format!("could not read template file: {e}"),
            )
            .with("path", path)
        })?;
    if bytes.len() > max {
        return Err(AppError::usage(
            "TEMPLATE_TOO_LARGE",
            "template exceeds the configured template byte limit",
        )
        .with("observed", bytes.len())
        .with("limit", max)
        .with("path", path));
    }
    String::from_utf8(bytes).map_err(|_| {
        AppError::usage(
            "TEMPLATE_FILE_UNREADABLE",
            "template file is not valid UTF-8",
        )
        .with("path", path)
    })
}

impl From<InputFormat> for combinator_core::input::InputFormat {
    fn from(v: InputFormat) -> Self {
        match v {
            InputFormat::Lines => Self::Lines,
            InputFormat::Csv => Self::Csv,
            InputFormat::Tsv => Self::Tsv,
            InputFormat::Nul => Self::Nul,
            InputFormat::Inline => Self::Lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_delimiter_limits_and_empty_list_delimiter() {
        assert!(validate_delims("-", "\n", ",").is_ok());
        assert_eq!(
            validate_delims("-", "\n", "").unwrap_err().code,
            "BAD_DELIMITER"
        );
        assert_eq!(
            validate_delims(&"x".repeat(MAX_DELIM_BYTES + 1), "\n", ",")
                .unwrap_err()
                .code,
            "BAD_DELIMITER"
        );
    }

    #[test]
    fn rejects_inline_file_format_and_missing_sources() {
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_file_list_format_bounded(
                "missing",
                InputFormat::Inline,
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "INPUT_FORMAT_INVALID"
        );
        assert_eq!(
            read_file_list_bounded("missing", InputLimits::default(), &mut budget)
                .unwrap_err()
                .code,
            "FILE_UNREADABLE"
        );
        assert_eq!(
            read_template_bounded("missing", MAX_TEMPLATE_BYTES)
                .unwrap_err()
                .code,
            "TEMPLATE_FILE_UNREADABLE"
        );
    }
}
