//! Command-line argument definitions.

use clap::{Args, Parser, Subcommand, ValueEnum};

pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1_073_741_824;
pub const DEFAULT_MAX_LISTS: usize = 128;
pub const DEFAULT_MAX_TOTAL_ITEMS: usize = 5_000_000;
pub const DEFAULT_MAX_COMBINATIONS: u128 = 10_000_000;
pub const DEFAULT_MAX_JOIN_RECORDS: usize = 100_000;
pub const DEFAULT_MAX_JOIN_KEY_FANOUT: u128 = 10_000;
pub const HARD_MAX_OUTPUT_BYTES: u64 = DEFAULT_MAX_OUTPUT_BYTES;
pub const HARD_MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const HARD_MAX_ITEM_BYTES: usize = 1024 * 1024;
pub const HARD_MAX_ITEMS_PER_LIST: usize = 1_000_000;
pub const HARD_MAX_LISTS: usize = DEFAULT_MAX_LISTS;
pub const HARD_MAX_TOTAL_ITEMS: usize = DEFAULT_MAX_TOTAL_ITEMS;
pub const HARD_MAX_COMBINATIONS: u128 = DEFAULT_MAX_COMBINATIONS;
pub const HARD_MAX_JOIN_RECORDS: usize = 250_000;
pub const HARD_MAX_JOIN_KEY_FANOUT: u128 = 100_000;
pub const HARD_MAX_TIMEOUT_MS: u64 = 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutFormat {
    Text,
    Jsonl,
    Json,
    Csv,
    Tsv,
    Nul,
}

/// CLI-facing mirror of `combinator_app::FormulaPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormulaPolicyArg {
    Allow,
    Warn,
    Reject,
}

impl From<FormulaPolicyArg> for combinator_app::FormulaPolicy {
    fn from(value: FormulaPolicyArg) -> Self {
        match value {
            FormulaPolicyArg::Allow => Self::Allow,
            FormulaPolicyArg::Warn => Self::Warn,
            FormulaPolicyArg::Reject => Self::Reject,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    Text,
    Json,
}

impl From<OutFormat> for combinator_codecs::Format {
    fn from(value: OutFormat) -> Self {
        match value {
            OutFormat::Text | OutFormat::Json => Self::Text,
            OutFormat::Jsonl => Self::Jsonl,
            OutFormat::Csv => Self::Csv,
            OutFormat::Tsv => Self::Tsv,
            OutFormat::Nul => Self::Nul,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    Lines,
    Csv,
    Tsv,
    Nul,
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum JoinFormat {
    Csv,
    Tsv,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum JoinTypeArg {
    Inner,
    Left,
    Full,
    Anti,
}

/// CLI-facing mirror of `combinator_core::UnequalPolicy` (clap's `ValueEnum`
/// derive cannot target a foreign type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UnequalPolicyArg {
    Error,
    Truncate,
    Cycle,
}

impl From<UnequalPolicyArg> for combinator_core::UnequalPolicy {
    fn from(value: UnequalPolicyArg) -> Self {
        match value {
            UnequalPolicyArg::Error => combinator_core::UnequalPolicy::Error,
            UnequalPolicyArg::Truncate => combinator_core::UnequalPolicy::Truncate,
            UnequalPolicyArg::Cycle => combinator_core::UnequalPolicy::Cycle,
        }
    }
}

/// Flags shared by every operation mode.
#[derive(Debug, Args)]
pub struct CommonArgs {
    /// Inline list, split by --list-delim. Repeatable; order is field order.
    /// Mutually exclusive with --file.
    #[arg(long)]
    pub list: Vec<String>,

    /// File list, one item per line (path `-` reads stdin). Repeatable; order
    /// is field order. Mutually exclusive with --list.
    #[arg(long)]
    pub file: Vec<String>,

    /// Input record format. The default keeps legacy inline-comma and line-file behavior.
    #[arg(long = "input-format", value_enum)]
    pub input_format: Option<InputFormat>,

    /// Permit combining --list and --file sources; inline lists precede files.
    #[arg(long = "allow-mixed-inputs")]
    pub allow_mixed_inputs: bool,

    /// Literal template for rendering each output value.
    #[arg(long)]
    pub template: Option<String>,

    /// UTF-8 file containing the output template.
    #[arg(long = "template-file")]
    pub template_file: Option<String>,

    /// Field name aligned with each input list; repeat once per list.
    #[arg(long = "name")]
    pub names: Vec<String>,

    /// Record separator between combinations (text mode only).
    #[arg(long = "rec-sep", default_value = "\n")]
    pub rec_sep: String,

    /// Delimiter for splitting inline --list values.
    #[arg(long = "list-delim", default_value = ",")]
    pub list_delim: String,

    /// Per-list normalization transform. Applied left-to-right. Repeatable.
    /// Supported forms: trim, skip-empty, deduplicate, reject-duplicates,
    /// sort, lower, upper, filter=GLOB, replace=FROM=>TO, prefix=VALUE,
    /// suffix=VALUE.
    #[arg(long = "transform")]
    pub transforms: Vec<String>,

    /// Typed candidate filter. Forms: eq:N=VALUE, neq:N=VALUE, prefix:N=VALUE,
    /// suffix:N=VALUE, glob:N=PATTERN, length:N=MIN..MAX. Repeatable; all
    /// filters must match.
    #[arg(long = "filter")]
    pub filters: Vec<String>,

    /// Emit combinations in reverse of the default order.
    #[arg(long)]
    pub reverse: bool,

    /// Skip this many leading combinations.
    #[arg(long, default_value_t = 0)]
    pub offset: u128,

    /// Emit at most this many combinations.
    #[arg(long)]
    pub limit: Option<u128>,

    /// Zero-based contiguous shard number.
    #[arg(long = "shard-index", requires = "shard_count")]
    pub shard_index: Option<u128>,

    /// Total number of contiguous shards.
    #[arg(long = "shard-count", requires = "shard_index")]
    pub shard_count: Option<u128>,

    /// Print only the total count, generating nothing.
    #[arg(long = "count-only")]
    pub count_only: bool,

    /// Print a validated execution summary without generating records.
    #[arg(long)]
    pub explain: bool,

    /// Validate the request and limits without generating records.
    #[arg(long)]
    pub dry_run: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutFormat::Text)]
    pub format: OutFormat,

    /// Handle recognized formula-like fields in CSV/TSV output.
    #[arg(long = "formula-policy", value_enum)]
    pub formula_policy: Option<FormulaPolicyArg>,

    /// In JSONL mode, emit only the value (as a JSON string) per line.
    #[arg(long = "lean-output")]
    pub lean_output: bool,

    /// Write to this file instead of stdout.
    #[arg(long, short = 'o')]
    pub output: Option<String>,

    /// Permit control characters or NUL records to be written directly to a terminal.
    #[arg(long = "allow-unsafe-terminal-output")]
    pub allow_unsafe_terminal_output: bool,

    /// Overwrite the output file if it exists.
    #[arg(long, visible_alias = "force", short = 'f')]
    pub overwrite: bool,

    /// Optional filesystem max file size (bytes) for preflight.
    #[arg(long = "max-file-size")]
    pub max_file_size: Option<u64>,

    /// Maximum output bytes for any invocation, including stdout.
    #[arg(long = "max-output-bytes", default_value_t = DEFAULT_MAX_OUTPUT_BYTES)]
    pub max_output_bytes: u64,

    /// Maximum bytes read from each file, stdin stream, or inline list.
    #[arg(long = "max-input-bytes", default_value_t = 64 * 1024 * 1024)]
    pub max_input_bytes: usize,

    /// Maximum UTF-8 bytes in one list item.
    #[arg(long = "max-item-bytes", default_value_t = 1024 * 1024)]
    pub max_item_bytes: usize,

    /// Maximum items accepted from one list.
    #[arg(long = "max-items-per-list", default_value_t = 1_000_000)]
    pub max_items_per_list: usize,

    /// Maximum number of lists accepted.
    #[arg(long = "max-lists", default_value_t = DEFAULT_MAX_LISTS)]
    pub max_lists: usize,

    /// Maximum total items across all lists.
    #[arg(long = "max-total-items", default_value_t = DEFAULT_MAX_TOTAL_ITEMS)]
    pub max_total_items: usize,

    /// Maximum combinations generated unless --count-only is used.
    #[arg(long = "max-combinations", default_value_t = DEFAULT_MAX_COMBINATIONS)]
    pub max_combinations: u128,

    /// Skip preflight validation for file output.
    #[arg(long = "no-preflight")]
    pub no_preflight: bool,

    /// Suppress non-fatal warnings.
    #[arg(long)]
    pub quiet: bool,

    /// Treat non-fatal warnings as runtime errors.
    #[arg(long = "warnings-as-errors")]
    pub warnings_as_errors: bool,

    /// Print a one-line record/byte summary to stderr after successful output.
    #[arg(long)]
    pub summary: bool,

    /// Enable opt-in operational logging. The default is off; COMBINATOR_LOG
    /// is used only when this option is omitted.
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevel>,

    /// Operational log framing written to stderr.
    #[arg(long = "log-format", value_enum, default_value_t = LogFormat::Text)]
    pub log_format: LogFormat,

    /// Resolved CLI-owned logging state; not a command-line argument.
    #[arg(skip)]
    pub resolved_log: Option<crate::logging::ResolvedLogConfig>,

    /// Cancel execution after this many milliseconds.
    #[arg(long = "timeout-ms")]
    pub timeout_ms: Option<u64>,
}

/// Ordered Cartesian product of the input lists (the default operation).
#[derive(Debug, Args)]
pub struct ProductArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Vary the leftmost list fastest instead of the rightmost.
    #[arg(long = "reverse-fields")]
    pub reverse_fields: bool,
}

/// Positional pairing of the input lists.
#[derive(Debug, Args)]
pub struct ZipArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Policy when input lists have unequal lengths.
    #[arg(long = "on-unequal", value_enum, default_value_t = UnequalPolicyArg::Error)]
    pub on_unequal: UnequalPolicyArg,
}

/// Sequential concatenation of the input lists.
#[derive(Debug, Args)]
pub struct ConcatArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Debug, Args)]
pub struct PermutationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Debug, Args)]
pub struct CombinationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(long = "choose")]
    pub choose: usize,
}

#[derive(Debug, Args)]
pub struct VariationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(long = "length")]
    pub length: usize,
}

#[derive(Debug, Args)]
pub struct JoinArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[arg(long)]
    pub left: String,
    #[arg(long)]
    pub right: String,
    #[arg(long = "left-key")]
    pub left_key: String,
    #[arg(long = "right-key")]
    pub right_key: String,
    #[arg(long = "type", value_enum, default_value_t = JoinTypeArg::Inner)]
    pub join_type: JoinTypeArg,
    #[arg(long = "join-format", value_enum, default_value_t = JoinFormat::Csv)]
    pub join_format: JoinFormat,
    /// Maximum records read from either join input.
    #[arg(long = "max-join-records", default_value_t = DEFAULT_MAX_JOIN_RECORDS)]
    pub max_join_records: usize,
    /// Maximum duplicate-key expansion for one matching key.
    #[arg(long = "max-join-key-fanout", default_value_t = DEFAULT_MAX_JOIN_KEY_FANOUT)]
    pub max_join_key_fanout: u128,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    /// Ordered Cartesian product (the default when no subcommand is given).
    Product(ProductArgs),
    /// Positional pairing of the input lists.
    Zip(ZipArgs),
    /// Sequential concatenation of the input lists.
    Concat(ConcatArgs),
    /// Generate all orderings of one input list.
    Permutations(PermutationsArgs),
    /// Generate unordered selections of a fixed size.
    Combinations(CombinationsArgs),
    /// Generate ordered selections without replacement.
    Variations(VariationsArgs),
    /// Keyed relational join of two structured files.
    Join(JoinArgs),
    /// Generate shell completion script.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Generate a roff man page for the CLI.
    Man,
}

/// Streams combinations of text lists: product (default), zip, concat.
#[derive(Debug, Parser)]
#[command(
    name = "combinator",
    version,
    about = combinator_app::PROJECT_DESCRIPTION,
    long_about = combinator_app::ABOUT_HELP
)]
pub struct Cli {
    /// Print project information and troubleshooting guidance.
    #[arg(long)]
    pub about: bool,

    #[command(subcommand)]
    pub command: Option<Mode>,

    #[command(flatten)]
    pub product: ProductArgs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn defaults_are_sane() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b"]);
        assert!(!cli.about);
        assert_eq!(cli.product.sep, "");
        assert_eq!(cli.product.common.rec_sep, "\n");
        assert_eq!(cli.product.common.list_delim, ",");
        assert!(!cli.product.common.reverse);
        assert!(!cli.product.reverse_fields);
        assert_eq!(cli.product.common.offset, 0);
        assert!(cli.product.common.limit.is_none());
        assert!(matches!(cli.product.common.format, OutFormat::Text));
        assert!(cli.product.common.formula_policy.is_none());
        assert!(!cli.product.common.allow_unsafe_terminal_output);
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_formula_policy_without_assigning_it_to_other_formats() {
        let cli = Cli::parse_from([
            "combinator",
            "--list",
            "=2+3",
            "--format",
            "csv",
            "--formula-policy",
            "reject",
        ]);
        assert_eq!(
            cli.product.common.formula_policy,
            Some(FormulaPolicyArg::Reject)
        );
    }

    #[test]
    fn unsafe_terminal_output_requires_an_explicit_flag() {
        let cli = Cli::parse_from([
            "combinator",
            "--list",
            "a",
            "--allow-unsafe-terminal-output",
        ]);
        assert!(cli.product.common.allow_unsafe_terminal_output);
    }

    #[test]
    fn parses_about_flag_without_an_operation() {
        let cli = Cli::parse_from(["combinator", "--about"]);
        assert!(cli.about);
        assert!(cli.command.is_none());
    }

    #[test]
    fn overwrite_alias_force_works() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "-o", "x.txt", "-f"]);
        assert!(cli.product.common.overwrite);
    }

    #[test]
    fn parses_repeated_lists_and_files() {
        let cli = Cli::parse_from([
            "combinator",
            "--list",
            "a",
            "--list",
            "b",
            "--file",
            "f.txt",
        ]);
        assert_eq!(cli.product.common.list, vec!["a", "b"]);
        assert_eq!(cli.product.common.file, vec!["f.txt"]);
    }

    #[test]
    fn parses_reverse_modes() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b", "--reverse-fields"]);
        assert!(!cli.product.common.reverse);
        assert!(cli.product.reverse_fields);
    }

    #[test]
    fn explicit_product_subcommand_parses_same_shape() {
        let cli = Cli::parse_from(["combinator", "product", "--list", "a,b", "--sep", "-"]);
        match cli.command {
            Some(Mode::Product(args)) => {
                assert_eq!(args.common.list, vec!["a,b"]);
                assert_eq!(args.sep, "-");
            }
            other => panic!("expected explicit product subcommand, got {other:?}"),
        }
    }

    #[test]
    fn zip_subcommand_parses_with_default_policy() {
        let cli = Cli::parse_from(["combinator", "zip", "--list", "a,b", "--list", "c,d"]);
        match cli.command {
            Some(Mode::Zip(args)) => {
                assert_eq!(args.on_unequal, UnequalPolicyArg::Error);
                assert_eq!(args.common.list, vec!["a,b", "c,d"]);
            }
            other => panic!("expected zip subcommand, got {other:?}"),
        }
    }

    #[test]
    fn zip_rejects_reverse_fields_flag() {
        let result =
            Cli::try_parse_from(["combinator", "zip", "--list", "a,b", "--reverse-fields"]);
        assert!(result.is_err());
    }

    #[test]
    fn concat_subcommand_has_no_sep_field() {
        let result = Cli::try_parse_from(["combinator", "concat", "--list", "a,b", "--sep", "-"]);
        assert!(result.is_err());
    }

    #[test]
    fn concat_subcommand_parses() {
        let cli = Cli::parse_from(["combinator", "concat", "--list", "a,b", "--list", "c,d"]);
        match cli.command {
            Some(Mode::Concat(args)) => {
                assert_eq!(args.common.list, vec!["a,b", "c,d"]);
            }
            other => panic!("expected concat subcommand, got {other:?}"),
        }
    }
}
