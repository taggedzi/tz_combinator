//! CLI-owned policy for writing attacker-controlled records to a terminal.

use combinator_codecs::Template;

use crate::cli::{CommonArgs, OutFormat};
use crate::error::AppError;

pub(crate) fn validate_terminal_output(
    common: &CommonArgs,
    lists: &[Vec<String>],
    field_separator: &str,
    template: Option<&Template>,
    stdout_is_terminal: bool,
) -> Result<(), AppError> {
    if !stdout_is_terminal
        || common.output.is_some()
        || common.allow_unsafe_terminal_output
        || common.count_only
        || common.explain
        || common.dry_run
        || matches!(common.format, OutFormat::Json | OutFormat::Jsonl)
    {
        return Ok(());
    }

    if common.format == OutFormat::Nul {
        return Err(unsafe_terminal_error(
            "NUL output is intended for files or pipes, not an interactive terminal",
            "format",
            None,
        ));
    }

    for (list_index, list) in lists.iter().enumerate() {
        for (item_index, item) in list.iter().enumerate() {
            if let Some(character) = first_control(item) {
                return Err(unsafe_terminal_error(
                    "an input value contains a terminal control character",
                    "input",
                    Some(character),
                )
                .with("list_index", list_index)
                .with("item_index", item_index));
            }
        }
    }

    if common.format == OutFormat::Text {
        if let Some(character) = first_control(field_separator) {
            return Err(unsafe_terminal_error(
                "the field separator contains a terminal control character",
                "field_separator",
                Some(character),
            ));
        }

        if let Some(character) = common
            .rec_sep
            .chars()
            .find(|character| character.is_control() && !matches!(character, '\r' | '\n'))
        {
            return Err(unsafe_terminal_error(
                "the record separator contains a terminal control character other than CR or LF",
                "record_separator",
                Some(character),
            ));
        }

        if let Some(character) = template.and_then(Template::first_literal_control) {
            return Err(unsafe_terminal_error(
                "a template literal contains a terminal control character",
                "template",
                Some(character),
            ));
        }
    }

    Ok(())
}

fn first_control(value: &str) -> Option<char> {
    value.chars().find(|character| character.is_control())
}

fn unsafe_terminal_error(
    message: &'static str,
    source: &'static str,
    character: Option<char>,
) -> AppError {
    let mut error = AppError::usage("UNSAFE_TERMINAL_OUTPUT", message)
        .with("source", source)
        .with(
            "remediation",
            "use --format jsonl, redirect output, or pass --allow-unsafe-terminal-output",
        );
    if let Some(character) = character {
        error = error.with("codepoint", format!("U+{:04X}", character as u32));
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;

    fn common() -> CommonArgs {
        Cli::parse_from(["combinator", "--list", "safe"])
            .product
            .common
    }

    fn lists(value: &str) -> Vec<Vec<String>> {
        vec![vec![value.to_string()]]
    }

    #[test]
    fn clean_text_is_allowed_on_a_terminal() {
        assert!(validate_terminal_output(&common(), &lists("safe"), "-", None, true).is_ok());
    }

    #[test]
    fn controls_in_values_are_rejected_before_terminal_output() {
        for value in ["line\nforged", "ansi\u{1b}[31m", "bell\u{7}", "c1\u{9b}31m"] {
            let error =
                validate_terminal_output(&common(), &lists(value), "", None, true).unwrap_err();
            assert_eq!(error.code, "UNSAFE_TERMINAL_OUTPUT");
        }
    }

    #[test]
    fn text_framing_allows_line_endings_but_rejects_active_controls() {
        let mut args = common();
        args.rec_sep = "\r\n".into();
        assert!(validate_terminal_output(&args, &lists("safe"), "", None, true).is_ok());

        args.rec_sep = "\u{1b}[0m".into();
        let error = validate_terminal_output(&args, &lists("safe"), "", None, true).unwrap_err();
        assert_eq!(error.code, "UNSAFE_TERMINAL_OUTPUT");

        args.rec_sep = "\n".into();
        let error = validate_terminal_output(&args, &lists("safe"), "\t", None, true).unwrap_err();
        assert_eq!(error.code, "UNSAFE_TERMINAL_OUTPUT");
    }

    #[test]
    fn template_literal_controls_are_rejected() {
        let template = Template::parse("safe\u{1b}[31m{0}").unwrap();
        let error = validate_terminal_output(&common(), &lists("safe"), "", Some(&template), true)
            .unwrap_err();
        assert_eq!(error.code, "UNSAFE_TERMINAL_OUTPUT");
    }

    #[test]
    fn nul_output_requires_an_explicit_terminal_override() {
        let mut args = common();
        args.format = OutFormat::Nul;
        let error = validate_terminal_output(&args, &lists("safe"), "", None, true).unwrap_err();
        assert_eq!(error.code, "UNSAFE_TERMINAL_OUTPUT");

        args.allow_unsafe_terminal_output = true;
        assert!(validate_terminal_output(&args, &lists("safe"), "", None, true).is_ok());
    }

    #[test]
    fn machine_destinations_and_non_record_modes_preserve_existing_behavior() {
        let hostile = lists("safe\nforged\u{1b}[31m");
        assert!(validate_terminal_output(&common(), &hostile, "", None, false).is_ok());

        let mut file = common();
        file.output = Some("output.txt".into());
        assert!(validate_terminal_output(&file, &hostile, "", None, true).is_ok());

        let mut count = common();
        count.count_only = true;
        assert!(validate_terminal_output(&count, &hostile, "", None, true).is_ok());

        let mut jsonl = common();
        jsonl.format = OutFormat::Jsonl;
        assert!(validate_terminal_output(&jsonl, &hostile, "", None, true).is_ok());
    }

    #[test]
    fn explicit_override_preserves_intentional_raw_terminal_output() {
        let mut args = common();
        args.allow_unsafe_terminal_output = true;
        assert!(validate_terminal_output(
            &args,
            &lists("safe\nforged\u{1b}[31m"),
            "\t",
            None,
            true
        )
        .is_ok());
    }
}
