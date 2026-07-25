//! Command-line argument definitions.

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutFormat {
    Text,
    Jsonl,
}

/// Streams the ordered Cartesian product of text lists.
#[derive(Debug, Parser)]
#[command(name = "combinator", version, about)]
pub struct Cli {
    /// Inline list, split by --list-delim. Repeatable; order is field order.
    /// Mutually exclusive with --file.
    #[arg(long)]
    pub list: Vec<String>,

    /// File list, one item per line (path `-` reads stdin). Repeatable; order
    /// is field order. Mutually exclusive with --list.
    #[arg(long)]
    pub file: Vec<String>,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Record separator between combinations (text mode only).
    #[arg(long = "rec-sep", default_value = "\n")]
    pub rec_sep: String,

    /// Delimiter for splitting inline --list values.
    #[arg(long = "list-delim", default_value = ",")]
    pub list_delim: String,

    /// Vary the leftmost list fastest instead of the rightmost.
    #[arg(long)]
    pub reverse: bool,

    /// Skip this many leading combinations.
    #[arg(long, default_value_t = 0)]
    pub offset: u128,

    /// Emit at most this many combinations.
    #[arg(long)]
    pub limit: Option<u128>,

    /// Print only the total count, generating nothing.
    #[arg(long = "count-only")]
    pub count_only: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutFormat::Text)]
    pub format: OutFormat,

    /// In JSONL mode, emit only the value (as a JSON string) per line.
    #[arg(long = "lean-output")]
    pub lean_output: bool,

    /// Write to this file instead of stdout.
    #[arg(long, short = 'o')]
    pub output: Option<String>,

    /// Overwrite the output file if it exists.
    #[arg(long, visible_alias = "force", short = 'f')]
    pub overwrite: bool,

    /// Optional filesystem max file size (bytes) for pre-flight.
    #[arg(long = "max-file-size")]
    pub max_file_size: Option<u64>,

    /// Skip pre-flight validation for file output.
    #[arg(long = "no-preflight")]
    pub no_preflight: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn defaults_are_sane() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b"]);
        assert_eq!(cli.sep, "");
        assert_eq!(cli.rec_sep, "\n");
        assert_eq!(cli.list_delim, ",");
        assert!(!cli.reverse);
        assert_eq!(cli.offset, 0);
        assert!(cli.limit.is_none());
        assert!(matches!(cli.format, OutFormat::Text));
    }

    #[test]
    fn overwrite_alias_force_works() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "-o", "x.txt", "-f"]);
        assert!(cli.overwrite);
    }

    #[test]
    fn parses_repeated_lists_and_files() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "--list", "b", "--file", "f.txt"]);
        assert_eq!(cli.list, vec!["a", "b"]);
        assert_eq!(cli.file, vec!["f.txt"]);
    }
}
