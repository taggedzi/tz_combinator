use std::fmt::Write as _;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use std::{
    fs,
    path::{Path, PathBuf},
};

use combinator_app::{
    about_text, ensure_output_parent, join_plan, join_preview, join_stream, plan, preview,
    read_input_source, stream, validate_resource_limits, AppError, AppOperation, CancellationToken,
    ExecutionPlan, FileSink, Format, FormulaPolicy, InputFormat, InputLimits, InputSource,
    JoinFormat, JoinKind, JoinPlan, JoinRequest, OutputRecord, OutputSink, ProductRequest,
    ProgressEvent, ResourceLimits, UnequalPolicy, DEFAULT_MAX_COMBINATIONS,
    DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_ITEMS_PER_LIST, DEFAULT_MAX_ITEM_BYTES,
    DEFAULT_MAX_JOIN_KEY_FANOUT, DEFAULT_MAX_JOIN_RECORDS, DEFAULT_MAX_LISTS,
    DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_MAX_TOTAL_ITEMS,
};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};

const PREVIEW_LIMIT: u128 = 20;
const PROFILE_VERSION: u32 = 1;
const MAX_PROFILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Combine,
    Join,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    ListValue,
    FilePath,
    FileMode,
    ListDelimiter,
    Separator,
    Operation,
    ZipPolicy,
    Format,
    FormulaPolicy,
    InputFormat,
    Choose,
    Length,
    Template,
    TemplateFileMode,
    TemplateFile,
    Transforms,
    Filters,
    Names,
    Offset,
    Limit,
    MaxCombinations,
    MaxOutputBytes,
    MaxInputBytes,
    MaxItemBytes,
    MaxItemsPerList,
    MaxTotalItems,
    MaxLists,
    Timeout,
    Reverse,
    ReverseFields,
    LeanJsonl,
    ShardIndex,
    ShardCount,
    DefaultOutputDirectory,
    JoinLeft,
    JoinRight,
    JoinLeftKey,
    JoinRightKey,
    JoinOffset,
    JoinLimit,
    JoinFormat,
    JoinKind,
    JoinMaxRecords,
    JoinFanout,
    OutputPath,
    ProfilePath,
    Overwrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    AddList,
    RemoveList,
    Preview,
    Generate,
    Cancel,
    New,
    OpenProfile,
    SaveProfile,
    SaveAsProfile,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Page(Page),
    Field(Field),
    Action(Action),
}

#[derive(Clone, Debug)]
struct Source {
    value: String,
    file_mode: bool,
    file_path: String,
    format: InputFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Profile {
    version: u32,
    active_mode: String,
    combine: CombineProfile,
    join: JoinProfile,
    output_path: String,
    overwrite: bool,
    limits: LimitsProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CombineProfile {
    sources: Vec<String>,
    file_sources: Vec<Option<String>>,
    file_formats: Vec<String>,
    list_delimiter: String,
    #[serde(default)]
    field_separator: String,
    template: String,
    template_file: String,
    template_file_mode: bool,
    transforms: String,
    filters: String,
    names: String,
    offset: String,
    limit: String,
    choose: String,
    length: String,
    operation: String,
    format: String,
    #[serde(default)]
    formula_policy: String,
    zip_policy: String,
    reverse: bool,
    reverse_fields: bool,
    lean_jsonl: bool,
    shard_index: String,
    shard_count: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JoinProfile {
    left_path: String,
    right_path: String,
    left_key: String,
    right_key: String,
    format: String,
    kind: String,
    offset: String,
    limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LimitsProfile {
    max_combinations: String,
    max_output_bytes: String,
    max_input_bytes: String,
    max_item_bytes: String,
    max_items_per_list: String,
    max_total_items: String,
    max_lists: String,
    timeout_ms: String,
    join_max_records: String,
    join_fanout: String,
}

impl Default for LimitsProfile {
    fn default() -> Self {
        let limits = ResourceLimits::default();
        Self {
            max_combinations: limits.max_combinations.to_string(),
            max_output_bytes: limits.max_output_bytes.to_string(),
            max_input_bytes: limits.max_input_bytes.to_string(),
            max_item_bytes: limits.max_item_bytes.to_string(),
            max_items_per_list: limits.max_items_per_list.to_string(),
            max_total_items: limits.max_total_items.to_string(),
            max_lists: limits.max_lists.to_string(),
            timeout_ms: String::new(),
            join_max_records: limits.max_join_records.to_string(),
            join_fanout: limits.max_join_key_fanout.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Preferences {
    recent_profiles: Vec<String>,
    last_profile: Option<String>,
    default_output_directory: Option<String>,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            value: String::new(),
            file_mode: false,
            file_path: String::new(),
            format: InputFormat::Lines,
        }
    }
}

enum WorkerMessage {
    Progress(ProgressEvent),
    Finished(Result<ProgressEvent, AppError>),
}

struct ProgressSink {
    sink: FileSink,
    messages: Sender<WorkerMessage>,
}

impl OutputSink for ProgressSink {
    fn record(&mut self, record: OutputRecord) -> Result<(), AppError> {
        self.sink.record(record)
    }

    fn progress(&mut self, event: ProgressEvent) -> Result<(), AppError> {
        self.messages
            .send(WorkerMessage::Progress(event))
            .map_err(|_| AppError {
                code: "CANCELLED",
                message: "TUI worker was disconnected".into(),
            })
    }
}

struct App {
    page: Page,
    focus: Focus,
    editing: Option<Field>,
    sources: Vec<Source>,
    selected_source: usize,
    list_delimiter: String,
    operation: AppOperation,
    format: Format,
    formula_policy: FormulaPolicy,
    field_separator: String,
    zip_policy: UnequalPolicy,
    reverse_fields: bool,
    choose: String,
    length: String,
    template: String,
    template_file: String,
    template_file_mode: bool,
    transforms: String,
    filters: String,
    names: String,
    offset: String,
    limit: String,
    max_combinations: String,
    max_output_bytes: String,
    max_input_bytes: String,
    max_item_bytes: String,
    max_items_per_list: String,
    max_total_items: String,
    max_lists: String,
    timeout: String,
    reverse: bool,
    lean_jsonl: bool,
    shard_index: String,
    shard_count: String,
    output_path: String,
    profile_path: String,
    preferences: Preferences,
    overwrite: bool,
    join_left: String,
    join_right: String,
    join_left_key: String,
    join_right_key: String,
    join_format: JoinFormat,
    join_kind: JoinKind,
    join_offset: String,
    join_limit: String,
    join_max_records: String,
    join_fanout: String,
    request: ProductRequest,
    join_request: JoinRequest,
    plan: Option<ExecutionPlan>,
    join_plan: Option<JoinPlan>,
    records: Vec<OutputRecord>,
    preview_scroll: u16,
    status: String,
    error: Option<String>,
    running: bool,
    cancellation: Option<CancellationToken>,
    worker: Option<Receiver<WorkerMessage>>,
    progress: Option<ProgressEvent>,
    about_open: bool,
}

impl Default for App {
    fn default() -> Self {
        let preferences = load_preferences();
        let output_path = preferences
            .default_output_directory
            .as_deref()
            .map(|directory| {
                std::path::Path::new(directory)
                    .join("output.txt")
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "output.txt".into());
        let mut app = Self {
            page: Page::Combine,
            focus: Focus::Page(Page::Combine),
            editing: None,
            sources: vec![Source::default()],
            selected_source: 0,
            list_delimiter: ",".into(),
            operation: AppOperation::default(),
            format: Format::Text,
            formula_policy: FormulaPolicy::Warn,
            field_separator: String::new(),
            zip_policy: UnequalPolicy::Error,
            reverse_fields: false,
            choose: "2".into(),
            length: "2".into(),
            template: String::new(),
            template_file: String::new(),
            template_file_mode: false,
            transforms: String::new(),
            filters: String::new(),
            names: String::new(),
            offset: "0".into(),
            limit: String::new(),
            max_combinations: DEFAULT_MAX_COMBINATIONS.to_string(),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES.to_string(),
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES.to_string(),
            max_item_bytes: DEFAULT_MAX_ITEM_BYTES.to_string(),
            max_items_per_list: DEFAULT_MAX_ITEMS_PER_LIST.to_string(),
            max_total_items: DEFAULT_MAX_TOTAL_ITEMS.to_string(),
            max_lists: DEFAULT_MAX_LISTS.to_string(),
            timeout: String::new(),
            reverse: false,
            lean_jsonl: false,
            shard_index: String::new(),
            shard_count: String::new(),
            output_path,
            profile_path: "combinator-profile.json".into(),
            preferences,
            overwrite: false,
            join_left: String::new(),
            join_right: String::new(),
            join_left_key: String::new(),
            join_right_key: String::new(),
            join_format: JoinFormat::Csv,
            join_kind: JoinKind::Inner,
            join_offset: "0".into(),
            join_limit: String::new(),
            join_max_records: DEFAULT_MAX_JOIN_RECORDS.to_string(),
            join_fanout: DEFAULT_MAX_JOIN_KEY_FANOUT.to_string(),
            request: ProductRequest::default(),
            join_request: JoinRequest::default(),
            plan: None,
            join_plan: None,
            records: Vec::new(),
            preview_scroll: 0,
            status: "Add values to begin".into(),
            error: None,
            running: false,
            cancellation: None,
            worker: None,
            progress: None,
            about_open: false,
        };
        if app.sync_requests() {
            app.refresh_plan();
        }
        app
    }
}

fn main() -> io::Result<()> {
    ratatui::run(run)
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();
    loop {
        app.poll_worker();
        terminal.draw(|frame| app.draw(frame))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && app.handle_key(key) {
                    return Ok(());
                }
            }
        }
    }
}

impl App {
    fn draw(&self, frame: &mut Frame<'_>) {
        if frame.area().width < 100 || frame.area().height < 28 {
            frame.render_widget(
                Paragraph::new(format!(
                    "Resize the terminal to at least 100 columns × 28 rows.\nCurrent size: {} × {}\nPress q to quit.",
                    frame.area().width,
                    frame.area().height
                ))
                .block(
                    Block::default()
                        .title(" Combinator — terminal too small ")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true }),
                frame.area(),
            );
            return;
        }
        let page =
            Layout::vertical([Constraint::Length(4), Constraint::Min(8)]).split(frame.area());
        frame.render_widget(self.header(), page[0]);
        if self.page == Page::Settings {
            frame.render_widget(self.settings_panel(page[1].height), page[1]);
            self.draw_about(frame);
            return;
        }
        let columns = Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(page[1]);
        let left = Layout::vertical([Constraint::Min(8), Constraint::Length(5)]).split(columns[0]);
        let right = Layout::vertical([
            Constraint::Min(8),
            Constraint::Length(9),
            Constraint::Length(7),
        ])
        .split(columns[1]);
        if self.page == Page::Combine {
            frame.render_widget(self.inputs_panel(left[0].height), left[0]);
            frame.render_widget(self.plan_panel(left[1].width), left[1]);
            frame.render_widget(self.options_panel(right[0].height), right[0]);
        } else {
            frame.render_widget(self.join_panel(left[0].height), left[0]);
            frame.render_widget(self.plan_panel(left[1].width), left[1]);
        }
        frame.render_widget(self.preview_panel(), right[1]);
        frame.render_widget(self.actions_panel(), right[2]);
        self.draw_about(frame);
    }

    fn draw_about(&self, frame: &mut Frame<'_>) {
        if !self.about_open {
            return;
        }
        let area = frame.area();
        let width = area.width.saturating_sub(8).min(88);
        let height = area.height.saturating_sub(6).min(18);
        let modal = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width.max(1),
            height.max(1),
        );
        frame.render_widget(Clear, modal);
        frame.render_widget(
            Paragraph::new(about_text())
                .block(
                    Block::default()
                        .title(" About tz_combinator ")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true }),
            modal,
        );
    }

    fn header(&self) -> Paragraph<'static> {
        let tab = |page: Page, label: &str| {
            if self.focus == Focus::Page(page) {
                format!("[▶ {label}]")
            } else if self.page == page {
                format!("[● {label}]")
            } else {
                format!("[  {label} ]")
            }
        };
        let new = if self.focus == Focus::Action(Action::New) {
            "[▶ New]"
        } else {
            "[  New ]"
        };
        let profile_button = |action: Action, label: &str| {
            if self.focus == Focus::Action(action) {
                format!("[▶ {label}]")
            } else {
                format!("[  {label} ]")
            }
        };
        let active = match self.page {
            Page::Combine => "Combine",
            Page::Join => "Join",
            Page::Settings => "Settings",
        };
        Paragraph::new(format!(
            "{}   {}   {}   {}   {}   {}   {}   {}   Focus: {}\nTab navigate • Enter edit/activate • [/] list • Ctrl+O/S/N profile • 1/2/3 pages • q quit",
            tab(Page::Combine, "Combine"),
            tab(Page::Join, "Join"),
            tab(Page::Settings, "Settings"),
            new,
            profile_button(Action::OpenProfile, "Open…"),
            profile_button(Action::SaveProfile, "Save"),
            profile_button(Action::SaveAsProfile, "Save as…"),
            profile_button(Action::About, "About"),
            focus_label(self.focus)
        ))
        .style(Style::default().fg(Color::White))
        .block(Block::default().title(format!(" Combinator — {active} ")).borders(Borders::ALL))
    }

    fn inputs_panel(&self, height: u16) -> Paragraph<'static> {
        let mut lines = Vec::new();
        lines.push(self.field_line(
            Focus::Field(Field::ListDelimiter),
            "Inline delimiter",
            &self.list_delimiter,
        ));
        for (index, source) in self.sources.iter().enumerate() {
            let selected = if index == self.selected_source {
                "•"
            } else {
                " "
            };
            let mode = if source.file_mode { "file" } else { "inline" };
            let detail = if source.file_mode {
                source.file_path.as_str()
            } else {
                source.value.as_str()
            };
            lines.push(format!(
                "{selected} List {} ({mode}): {}",
                index + 1,
                terminal_text(detail, 160)
            ));
        }
        lines.push(String::new());
        lines.push(format!("Selected list {}", self.selected_source + 1));
        lines.push(self.field_line(
            Focus::Field(Field::FileMode),
            "File source",
            checkbox(self.sources[self.selected_source].file_mode),
        ));
        let source = &self.sources[self.selected_source];
        if source.file_mode {
            lines.push(self.field_line(
                Focus::Field(Field::InputFormat),
                "File delimiter",
                input_format_label(source.format),
            ));
            lines.push(self.field_line(
                Focus::Field(Field::FilePath),
                "File path",
                &source.file_path,
            ));
        } else {
            lines.push(self.field_line(
                Focus::Field(Field::ListValue),
                "Values separated by delimiter",
                &source.value,
            ));
        }
        lines.push(String::new());
        lines.push(self.action_line(Action::AddList, "Add list"));
        if self.sources.len() > 1 {
            lines.push(self.action_line(Action::RemoveList, "Remove selected list"));
        } else {
            lines.push("[  Remove selected list (disabled) ]".into());
        }
        lines.push("File sources support bounded Lines, CSV, TSV, and NUL input.".into());
        let scroll = panel_scroll(&lines, height);
        Paragraph::new(lines.join("\n"))
            .block(Block::default().title(" Inputs ").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
    }

    fn join_panel(&self, height: u16) -> Paragraph<'static> {
        let lines = vec![
            "Structured join".into(),
            self.field_line(
                Focus::Field(Field::JoinLeft),
                "Left CSV/TSV/JSONL path",
                &self.join_left,
            ),
            self.field_line(
                Focus::Field(Field::JoinRight),
                "Right CSV/TSV/JSONL path",
                &self.join_right,
            ),
            self.field_line(
                Focus::Field(Field::JoinLeftKey),
                "Left key",
                &self.join_left_key,
            ),
            self.field_line(
                Focus::Field(Field::JoinRightKey),
                "Right key",
                &self.join_right_key,
            ),
            self.field_line(Focus::Field(Field::JoinOffset), "Offset", &self.join_offset),
            self.field_line(
                Focus::Field(Field::JoinLimit),
                "Limit (optional)",
                &self.join_limit,
            ),
            self.field_line(
                Focus::Field(Field::JoinFormat),
                "Format",
                join_format_label(self.join_format),
            ),
            self.field_line(
                Focus::Field(Field::JoinKind),
                "Type",
                join_kind_label(self.join_kind),
            ),
            self.field_line(
                Focus::Field(Field::OutputPath),
                "Output file",
                &self.output_path,
            ),
        ];
        let scroll = panel_scroll(&lines, height);
        Paragraph::new(lines.join("\n"))
            .block(Block::default().title(" Join ").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
    }

    fn options_panel(&self, height: u16) -> Paragraph<'static> {
        let mut lines = vec![
            "Data Selection and pre-processing".into(),
            self.field_line(
                Focus::Field(Field::Operation),
                "Operation",
                operation_label(self.operation),
            ),
        ];
        if matches!(self.operation, AppOperation::Zip { .. }) {
            lines.push(self.field_line(
                Focus::Field(Field::ZipPolicy),
                "Unequal list lengths",
                zip_policy_label(self.zip_policy),
            ));
        }
        lines.extend([
            self.field_line(Focus::Field(Field::Offset), "Offset", &self.offset),
            self.field_line(Focus::Field(Field::Limit), "Limit (optional)", &self.limit),
        ]);
        match self.operation {
            AppOperation::Product { .. } => lines.push(self.field_line(
                Focus::Field(Field::ReverseFields),
                "Leftmost first",
                checkbox(self.reverse_fields),
            )),
            AppOperation::Combinations { .. } => {
                lines.push(self.field_line(Focus::Field(Field::Choose), "Choose", &self.choose))
            }
            AppOperation::Variations { .. } => {
                lines.push(self.field_line(Focus::Field(Field::Length), "Length", &self.length))
            }
            _ => {}
        }
        lines.push(self.field_line(
            Focus::Field(Field::Filters),
            "Filters (semicolon-separated)",
            &self.filters,
        ));
        lines.extend([
            String::new(),
            "Output Options".into(),
            self.field_line(
                Focus::Field(Field::Reverse),
                "Reverse output",
                checkbox(self.reverse),
            ),
            self.field_line(
                Focus::Field(Field::Transforms),
                "Transforms (semicolon-separated)",
                &self.transforms,
            ),
            self.field_line(
                Focus::Field(Field::TemplateFileMode),
                "Template file",
                checkbox(self.template_file_mode),
            ),
        ]);
        if self.template_file_mode {
            lines.push(self.field_line(
                Focus::Field(Field::TemplateFile),
                "Template file path (optional)",
                &self.template_file,
            ));
        } else {
            lines.push(self.field_line(
                Focus::Field(Field::Template),
                "Template (optional)",
                &self.template,
            ));
        }
        lines.push(self.field_line(
            Focus::Field(Field::Format),
            "Output format",
            format_label(self.format),
        ));
        if matches!(self.format, Format::Csv | Format::Tsv) {
            lines.push(self.field_line(
                Focus::Field(Field::FormulaPolicy),
                "Formula-like field policy",
                formula_policy_label(self.formula_policy),
            ));
            lines.push(
                "  CSV/TSV preserves content; downstream consumers may reinterpret formula-like fields."
                    .into(),
            );
        }
        if format_uses_field_separator(self.format) {
            lines.push(self.field_line(
                Focus::Field(Field::Separator),
                "Field separator",
                &self.field_separator,
            ));
        }
        if self.format == Format::Jsonl {
            lines.push(self.field_line(
                Focus::Field(Field::LeanJsonl),
                "Lean JSONL (omit metadata)",
                checkbox(self.lean_jsonl),
            ));
            lines.push(self.field_line(
                Focus::Field(Field::Names),
                "Field names (semicolon-separated)",
                &self.names,
            ));
        }
        lines.extend([
            String::new(),
            "Sharding".into(),
            self.field_line(
                Focus::Field(Field::ShardIndex),
                "Shard index",
                &self.shard_index,
            ),
            self.field_line(
                Focus::Field(Field::ShardCount),
                "Shard count",
                &self.shard_count,
            ),
            self.field_line(
                Focus::Field(Field::OutputPath),
                "Output file path",
                &self.output_path,
            ),
        ]);
        let scroll = panel_scroll(&lines, height);
        Paragraph::new(lines.join("\n"))
            .block(
                Block::default()
                    .title(" Combine options ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
    }

    fn action_line(&self, action: Action, label: &str) -> String {
        if self.focus == Focus::Action(action) {
            format!("[▶ {label}]")
        } else {
            format!("[  {label} ]")
        }
    }

    fn settings_panel(&self, height: u16) -> Paragraph<'static> {
        let mut lines = vec![
            "Shared execution policy and safety limits used by Combine and Join.".into(),
            String::new(),
            "Application preferences".into(),
            self.field_line(
                Focus::Field(Field::DefaultOutputDirectory),
                "Default output directory",
                self.preferences
                    .default_output_directory
                    .as_deref()
                    .unwrap_or_default(),
            ),
            self.field_line(
                Focus::Field(Field::ProfilePath),
                "Profile file path (used by Open/Save)",
                &self.profile_path,
            ),
        ];
        if self.preferences.recent_profiles.is_empty() {
            lines.push("  Recent profiles: none saved yet".into());
        } else {
            lines.push("  Recent profiles:".into());
            lines.extend(
                self.preferences
                    .recent_profiles
                    .iter()
                    .take(3)
                    .map(|path| format!("    {}", terminal_text(path, 256))),
            );
        }
        lines.extend([
            String::new(),
            "Shared limits".into(),
            self.field_line(
                Focus::Field(Field::MaxOutputBytes),
                "Maximum output bytes",
                &self.max_output_bytes,
            ),
            self.field_line(
                Focus::Field(Field::MaxInputBytes),
                "Maximum input bytes per source",
                &self.max_input_bytes,
            ),
            self.field_line(
                Focus::Field(Field::MaxItemBytes),
                "Maximum item bytes",
                &self.max_item_bytes,
            ),
            self.field_line(
                Focus::Field(Field::Timeout),
                "Timeout in milliseconds (optional)",
                &self.timeout,
            ),
            self.field_line(
                Focus::Field(Field::Overwrite),
                "Overwrite existing output file",
                checkbox(self.overwrite),
            ),
            String::new(),
            "Combine limits".into(),
            self.field_line(
                Focus::Field(Field::MaxCombinations),
                "Maximum combinations",
                &self.max_combinations,
            ),
            self.field_line(
                Focus::Field(Field::MaxItemsPerList),
                "Max items/list",
                &self.max_items_per_list,
            ),
            self.field_line(Focus::Field(Field::MaxLists), "Max lists", &self.max_lists),
            self.field_line(
                Focus::Field(Field::MaxTotalItems),
                "Max total items",
                &self.max_total_items,
            ),
            String::new(),
            "Join limits".into(),
            self.field_line(
                Focus::Field(Field::JoinMaxRecords),
                "Max join records",
                &self.join_max_records,
            ),
            self.field_line(
                Focus::Field(Field::JoinFanout),
                "Max key fanout",
                &self.join_fanout,
            ),
        ]);
        let scroll = panel_scroll(&lines, height);
        Paragraph::new(lines.join("\n"))
            .block(Block::default().title(" Settings ").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
    }

    fn field_line(&self, target: Focus, label: &str, value: &str) -> String {
        let marker = if self.focus == target { "▶" } else { " " };
        let editing = if self.editing == field_from_focus(target) {
            "  (editing; Enter to finish)"
        } else {
            ""
        };
        format!("{marker} {label}: [{}]{editing}", terminal_text(value, 512))
    }

    fn plan_panel(&self, width: u16) -> Paragraph<'static> {
        let content_width = usize::from(width.saturating_sub(2));
        let body = if self.page == Page::Join {
            match &self.join_plan {
                Some(plan) => [
                    plan_row(
                        &[
                            ("Left records", plan.left_records.to_string()),
                            ("Right records", plan.right_records.to_string()),
                        ],
                        content_width,
                    ),
                    plan_row(
                        &[
                            ("Join records", plan.total_records.to_string()),
                            ("Selected", plan.records_to_emit.to_string()),
                        ],
                        content_width,
                    ),
                ]
                .join("\n"),
                None => "Enter valid join paths and keys".into(),
            }
        } else {
            match &self.plan {
                Some(plan) => {
                    let mut rows = vec![
                        plan_row(
                            &[
                                ("Lists", plan.list_lengths.len().to_string()),
                                ("Items", format!("{:?}", plan.list_lengths)),
                                ("Combos", format!("{:?}", plan.total_combinations)),
                            ],
                            content_width,
                        ),
                        plan_row(
                            &[
                                ("Selected", plan.records_to_emit.to_string()),
                                ("Bytes", format!("{:?}", plan.estimated_output_bytes)),
                                ("Warnings", plan.warnings.len().to_string()),
                            ],
                            content_width,
                        ),
                    ];
                    rows.extend(
                        plan.warnings
                            .iter()
                            .map(|warning| format!("{}: {}", warning.code, warning.message)),
                    );
                    rows.join("\n")
                }
                None => "Enter at least one input value".into(),
            }
        };
        Paragraph::new(body).block(
            Block::default()
                .title(" Execution plan ")
                .borders(Borders::ALL),
        )
    }

    fn preview_panel(&self) -> Paragraph<'static> {
        let body = if self.records.is_empty() {
            "No preview records. Focus Preview and press Enter, or press p.".into()
        } else {
            self.records
                .iter()
                .map(|r| format!("{}  {}", r.ordinal, terminal_text(r.value.trim_end(), 512)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Paragraph::new(body)
            .block(
                Block::default()
                    .title(" Preview — first 20 ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true })
            .scroll((self.preview_scroll, 0))
    }

    fn actions_panel(&self) -> Paragraph<'static> {
        let button = |target: Focus, text: &str| {
            if self.focus == target {
                format!("[▶ {text}]")
            } else {
                format!("[  {text} ]")
            }
        };
        let status = self
            .error
            .as_deref()
            .map(|e| format!("Error: {}", terminal_text(e, 512)))
            .unwrap_or_else(|| terminal_text(&self.status, 512));
        let generation = if self.running {
            button(Focus::Action(Action::Cancel), "Cancel")
        } else {
            button(Focus::Action(Action::Generate), "Generate file")
        };
        let list_shortcuts = if self.page == Page::Combine {
            "\nLists: [ previous • ] next • a add • d remove"
        } else {
            ""
        };
        Paragraph::new(format!(
            "{}   {}\n{}{}\nEnter activate • p preview • g generate • c cancel • PgUp/PgDn preview",
            button(Focus::Action(Action::Preview), "Preview first 20"),
            generation,
            status,
            list_shortcuts,
        ))
        .block(Block::default().title(" Actions ").borders(Borders::ALL))
    }

    fn focus_order(&self) -> Vec<Focus> {
        let mut order = vec![
            Focus::Page(Page::Combine),
            Focus::Page(Page::Join),
            Focus::Page(Page::Settings),
            Focus::Action(Action::New),
            Focus::Action(Action::OpenProfile),
            Focus::Action(Action::SaveProfile),
            Focus::Action(Action::SaveAsProfile),
            Focus::Action(Action::About),
        ];
        match self.page {
            Page::Combine => {
                order.push(Focus::Field(Field::ListDelimiter));
                order.push(Focus::Field(Field::FileMode));
                if self.sources[self.selected_source].file_mode {
                    order.push(Focus::Field(Field::InputFormat));
                    order.push(Focus::Field(Field::FilePath));
                } else {
                    order.push(Focus::Field(Field::ListValue));
                }
                order.push(Focus::Action(Action::AddList));
                if self.sources.len() > 1 {
                    order.push(Focus::Action(Action::RemoveList));
                }
                order.push(Focus::Field(Field::Operation));
                if matches!(self.operation, AppOperation::Zip { .. }) {
                    order.push(Focus::Field(Field::ZipPolicy));
                }
                order.extend([Focus::Field(Field::Offset), Focus::Field(Field::Limit)]);
                match self.operation {
                    AppOperation::Product { .. } => order.push(Focus::Field(Field::ReverseFields)),
                    AppOperation::Combinations { .. } => order.push(Focus::Field(Field::Choose)),
                    AppOperation::Variations { .. } => order.push(Focus::Field(Field::Length)),
                    _ => {}
                }
                order.extend([
                    Focus::Field(Field::Filters),
                    Focus::Field(Field::Reverse),
                    Focus::Field(Field::Transforms),
                    Focus::Field(Field::TemplateFileMode),
                ]);
                if self.template_file_mode {
                    order.push(Focus::Field(Field::TemplateFile));
                } else {
                    order.push(Focus::Field(Field::Template));
                }
                order.push(Focus::Field(Field::Format));
                if matches!(self.format, Format::Csv | Format::Tsv) {
                    order.push(Focus::Field(Field::FormulaPolicy));
                }
                if format_uses_field_separator(self.format) {
                    order.push(Focus::Field(Field::Separator));
                }
                if self.format == Format::Jsonl {
                    order.extend([Focus::Field(Field::LeanJsonl), Focus::Field(Field::Names)]);
                }
                order.extend([
                    Focus::Field(Field::ShardIndex),
                    Focus::Field(Field::ShardCount),
                    Focus::Field(Field::OutputPath),
                    Focus::Action(Action::Preview),
                ]);
                order.push(if self.running {
                    Focus::Action(Action::Cancel)
                } else {
                    Focus::Action(Action::Generate)
                });
            }
            Page::Join => order.extend([
                Focus::Field(Field::JoinLeft),
                Focus::Field(Field::JoinRight),
                Focus::Field(Field::JoinLeftKey),
                Focus::Field(Field::JoinRightKey),
                Focus::Field(Field::JoinOffset),
                Focus::Field(Field::JoinLimit),
                Focus::Field(Field::JoinFormat),
                Focus::Field(Field::JoinKind),
                Focus::Field(Field::OutputPath),
                Focus::Action(Action::Preview),
                if self.running {
                    Focus::Action(Action::Cancel)
                } else {
                    Focus::Action(Action::Generate)
                },
            ]),
            Page::Settings => order.extend([
                Focus::Field(Field::DefaultOutputDirectory),
                Focus::Field(Field::ProfilePath),
                Focus::Field(Field::MaxOutputBytes),
                Focus::Field(Field::MaxInputBytes),
                Focus::Field(Field::MaxItemBytes),
                Focus::Field(Field::Timeout),
                Focus::Field(Field::Overwrite),
                Focus::Field(Field::MaxCombinations),
                Focus::Field(Field::MaxItemsPerList),
                Focus::Field(Field::MaxLists),
                Focus::Field(Field::MaxTotalItems),
                Focus::Field(Field::JoinMaxRecords),
                Focus::Field(Field::JoinFanout),
            ]),
        }
        order
    }

    fn move_focus(&mut self, direction: i32) {
        let order = self.focus_order();
        let index = order
            .iter()
            .position(|target| *target == self.focus)
            .unwrap_or(0);
        let next = if direction < 0 {
            (index + order.len() - 1) % order.len()
        } else {
            (index + 1) % order.len()
        };
        self.focus = order[next];
    }

    fn activate_focus(&mut self) {
        match self.focus {
            Focus::Page(page) => {
                self.page = page;
                self.focus = Focus::Page(page);
                self.editing = None;
                self.refresh_plan();
            }
            Focus::Field(field) => {
                if is_text_field(field) {
                    self.editing = Some(field);
                } else {
                    self.toggle_field(field);
                }
            }
            Focus::Action(action) => match action {
                Action::AddList => self.add_list(),
                Action::RemoveList => self.remove_list(),
                Action::Preview => self.run_preview(),
                Action::Generate => self.start_generation(),
                Action::Cancel => self.cancel_generation(),
                Action::New => *self = Self::default(),
                Action::OpenProfile => self.open_profile(),
                Action::SaveProfile => self.save_profile(),
                Action::SaveAsProfile => {
                    self.page = Page::Settings;
                    self.focus = Focus::Field(Field::ProfilePath);
                    self.editing = Some(Field::ProfilePath);
                    self.status = "Enter a new profile path, then activate Save".into();
                }
                Action::About => self.about_open = true,
            },
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.about_open {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.about_open = false;
            }
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.cancel_generation();
            return false;
        }
        if let Some(field) = self.editing {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('s') => {
                        self.finish_edit(field);
                        self.save_profile();
                    }
                    KeyCode::Char('o') => {
                        self.finish_edit(field);
                        self.open_profile();
                    }
                    KeyCode::Char('u') => self.set_field_value(field, String::new()),
                    _ => {}
                }
                return false;
            }
            match key.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Enter => self.finish_edit(field),
                KeyCode::Tab => {
                    self.finish_edit(field);
                    self.move_focus(1);
                }
                KeyCode::BackTab => {
                    self.finish_edit(field);
                    self.move_focus(-1);
                }
                KeyCode::Backspace => self.edit_char(field, None),
                KeyCode::Char(c) => self.edit_char(field, Some(c)),
                _ => {}
            }
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('o') => self.open_profile(),
                KeyCode::Char('s') => self.save_profile(),
                KeyCode::Char('n') => *self = Self::default(),
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Tab | KeyCode::Down => self.move_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(-1),
            KeyCode::Enter | KeyCode::Right => self.activate_focus(),
            KeyCode::Char('1') => {
                self.page = Page::Combine;
                self.focus = Focus::Page(Page::Combine);
                self.refresh_plan();
            }
            KeyCode::Char('2') => {
                self.page = Page::Join;
                self.focus = Focus::Page(Page::Join);
                self.refresh_plan();
            }
            KeyCode::Char('3') => {
                self.page = Page::Settings;
                self.focus = Focus::Page(Page::Settings);
            }
            KeyCode::Char('p') if self.page != Page::Settings => self.run_preview(),
            KeyCode::Char('g') if self.page != Page::Settings => self.start_generation(),
            KeyCode::Char('c') => self.cancel_generation(),
            KeyCode::PageUp => self.preview_scroll = self.preview_scroll.saturating_sub(5),
            KeyCode::PageDown => {
                let last = self.records.len().saturating_sub(1);
                self.preview_scroll = self
                    .preview_scroll
                    .saturating_add(5)
                    .min(last.try_into().unwrap_or(u16::MAX));
            }
            KeyCode::Char('a') if self.page == Page::Combine => self.add_list(),
            KeyCode::Char('d') if self.page == Page::Combine => self.remove_list(),
            KeyCode::Left | KeyCode::Char('[') if self.page == Page::Combine => {
                self.select_previous_source()
            }
            KeyCode::Char(']') if self.page == Page::Combine => self.select_next_source(),
            _ => {}
        }
        false
    }

    fn finish_edit(&mut self, field: Field) {
        self.editing = None;
        if field == Field::DefaultOutputDirectory {
            self.preferences.default_output_directory =
                (!self.field_value(field).trim().is_empty()).then(|| self.field_value(field));
            if let Err(error) = save_preferences(&self.preferences) {
                self.error = Some(format!("PREFERENCES_SAVE_FAILED: {error}"));
            }
        }
    }

    fn edit_char(&mut self, field: Field, character: Option<char>) {
        let mut value = self.field_value(field);
        if let Some(c) = character {
            value.push(c);
        } else {
            value.pop();
        }
        self.set_field_value(field, value);
    }

    fn field_value(&self, field: Field) -> String {
        match field {
            Field::ListValue => self.sources[self.selected_source].value.clone(),
            Field::FilePath => self.sources[self.selected_source].file_path.clone(),
            Field::ListDelimiter => self.list_delimiter.clone(),
            Field::Separator => self.field_separator.clone(),
            Field::Choose => self.choose.clone(),
            Field::Length => self.length.clone(),
            Field::Template => self.template.clone(),
            Field::TemplateFile => self.template_file.clone(),
            Field::Transforms => self.transforms.clone(),
            Field::Filters => self.filters.clone(),
            Field::Names => self.names.clone(),
            Field::Offset => self.offset.clone(),
            Field::Limit => self.limit.clone(),
            Field::MaxCombinations => self.max_combinations.clone(),
            Field::MaxOutputBytes => self.max_output_bytes.clone(),
            Field::MaxInputBytes => self.max_input_bytes.clone(),
            Field::MaxItemBytes => self.max_item_bytes.clone(),
            Field::MaxItemsPerList => self.max_items_per_list.clone(),
            Field::MaxTotalItems => self.max_total_items.clone(),
            Field::MaxLists => self.max_lists.clone(),
            Field::Timeout => self.timeout.clone(),
            Field::ShardIndex => self.shard_index.clone(),
            Field::ShardCount => self.shard_count.clone(),
            Field::DefaultOutputDirectory => self
                .preferences
                .default_output_directory
                .clone()
                .unwrap_or_default(),
            Field::JoinLeft => self.join_left.clone(),
            Field::JoinRight => self.join_right.clone(),
            Field::JoinLeftKey => self.join_left_key.clone(),
            Field::JoinRightKey => self.join_right_key.clone(),
            Field::JoinOffset => self.join_offset.clone(),
            Field::JoinLimit => self.join_limit.clone(),
            Field::JoinMaxRecords => self.join_max_records.clone(),
            Field::JoinFanout => self.join_fanout.clone(),
            Field::OutputPath => self.output_path.clone(),
            Field::ProfilePath => self.profile_path.clone(),
            _ => String::new(),
        }
    }

    fn set_field_value(&mut self, field: Field, value: String) {
        match field {
            Field::ListValue => self.sources[self.selected_source].value = value,
            Field::FilePath => self.sources[self.selected_source].file_path = value,
            Field::ListDelimiter => self.list_delimiter = value,
            Field::Separator => self.field_separator = value,
            Field::Choose => self.choose = value,
            Field::Length => self.length = value,
            Field::Template => self.template = value,
            Field::TemplateFile => self.template_file = value,
            Field::Transforms => self.transforms = value,
            Field::Filters => self.filters = value,
            Field::Names => self.names = value,
            Field::Offset => self.offset = value,
            Field::Limit => self.limit = value,
            Field::MaxCombinations => self.max_combinations = value,
            Field::MaxOutputBytes => self.max_output_bytes = value,
            Field::MaxInputBytes => self.max_input_bytes = value,
            Field::MaxItemBytes => self.max_item_bytes = value,
            Field::MaxItemsPerList => self.max_items_per_list = value,
            Field::MaxTotalItems => self.max_total_items = value,
            Field::MaxLists => self.max_lists = value,
            Field::Timeout => self.timeout = value,
            Field::ShardIndex => self.shard_index = value,
            Field::ShardCount => self.shard_count = value,
            Field::DefaultOutputDirectory => {
                self.preferences.default_output_directory = Some(value)
            }
            Field::JoinLeft => self.join_left = value,
            Field::JoinRight => self.join_right = value,
            Field::JoinLeftKey => self.join_left_key = value,
            Field::JoinRightKey => self.join_right_key = value,
            Field::JoinOffset => self.join_offset = value,
            Field::JoinLimit => self.join_limit = value,
            Field::JoinMaxRecords => self.join_max_records = value,
            Field::JoinFanout => self.join_fanout = value,
            Field::OutputPath => self.output_path = value,
            Field::ProfilePath => self.profile_path = value,
            _ => return,
        }
        if self.sync_requests() {
            self.refresh_plan();
        }
    }

    fn toggle_field(&mut self, field: Field) {
        match field {
            Field::FileMode => {
                self.sources[self.selected_source].file_mode =
                    !self.sources[self.selected_source].file_mode
            }
            Field::InputFormat => {
                self.sources[self.selected_source].format =
                    next_input_format(self.sources[self.selected_source].format)
            }
            Field::Operation => self.operation = next_operation(self.operation),
            Field::ZipPolicy => self.zip_policy = next_zip_policy(self.zip_policy),
            Field::Format => self.format = next_format(self.format),
            Field::FormulaPolicy => self.formula_policy = next_formula_policy(self.formula_policy),
            Field::TemplateFileMode => {
                self.template_file_mode = !self.template_file_mode;
                if self.template_file_mode {
                    self.template.clear();
                } else {
                    self.template_file.clear();
                }
            }
            Field::JoinFormat => self.join_format = next_join_format(self.join_format),
            Field::JoinKind => self.join_kind = next_join_kind(self.join_kind),
            Field::Reverse => self.reverse = !self.reverse,
            Field::ReverseFields => self.reverse_fields = !self.reverse_fields,
            Field::LeanJsonl => self.lean_jsonl = !self.lean_jsonl,
            Field::Overwrite => self.overwrite = !self.overwrite,
            _ => return,
        }
        if self.sync_requests() {
            self.refresh_plan();
        }
    }

    fn add_list(&mut self) {
        self.sources.push(Source::default());
        self.selected_source = self.sources.len() - 1;
        self.refresh_plan();
    }

    fn remove_list(&mut self) {
        if self.sources.len() > 1 {
            self.sources.remove(self.selected_source);
            self.selected_source = self.selected_source.min(self.sources.len() - 1);
            if self.sources.len() == 1 && self.focus == Focus::Action(Action::RemoveList) {
                self.focus = Focus::Action(Action::AddList);
            }
            self.refresh_plan();
        }
    }

    fn select_previous_source(&mut self) {
        self.selected_source = self
            .selected_source
            .checked_sub(1)
            .unwrap_or(self.sources.len() - 1);
    }

    fn select_next_source(&mut self) {
        self.selected_source = (self.selected_source + 1) % self.sources.len();
    }

    fn sync_requests(&mut self) -> bool {
        let limits = match parse_profile_limits(&self.limits_profile()) {
            Ok(limits) => limits,
            Err(error) => {
                self.set_error(error);
                return false;
            }
        };
        self.request.operation = match self.operation {
            AppOperation::Product { .. } => AppOperation::Product {
                reverse_fields: self.reverse_fields,
            },
            AppOperation::Zip { .. } => AppOperation::Zip {
                on_unequal: self.zip_policy,
            },
            AppOperation::Combinations { .. } => AppOperation::Combinations {
                choose: self.choose.parse().unwrap_or(0),
            },
            AppOperation::Variations { .. } => AppOperation::Variations {
                length: self.length.parse().unwrap_or(0),
            },
            operation => operation,
        };
        self.request.format = self.format;
        self.request.formula_policy = self.formula_policy;
        self.request.lean_jsonl = self.lean_jsonl;
        self.request.field_separator = self.field_separator.clone();
        self.request.names = split_values(&self.names);
        self.request.transforms = split_values(&self.transforms);
        self.request.filters = split_values(&self.filters);
        self.request.template =
            (!self.template_file_mode && !self.template.is_empty()).then(|| self.template.clone());
        self.request.template_file = (self.template_file_mode && !self.template_file.is_empty())
            .then(|| self.template_file.clone());
        self.request.options.reverse = self.reverse;
        self.request.options.offset = self.offset.parse().unwrap_or(0);
        self.request.options.limit =
            (!self.limit.is_empty()).then(|| self.limit.parse().unwrap_or(0));
        self.request.max_combinations = limits.max_combinations;
        self.request.max_output_bytes = limits.max_output_bytes;
        self.request.timeout_ms = limits.timeout_ms;
        self.request.shard_index =
            (!self.shard_index.is_empty()).then(|| self.shard_index.parse().unwrap_or(0));
        self.request.shard_count =
            (!self.shard_count.is_empty()).then(|| self.shard_count.parse().unwrap_or(0));
        self.request.max_input_bytes = limits.max_input_bytes;
        self.request.max_item_bytes = limits.max_item_bytes;
        self.request.max_items_per_list = limits.max_items_per_list;
        self.request.max_total_items = limits.max_total_items;
        self.request.max_lists = limits.max_lists;
        self.request.lists = self
            .sources
            .iter()
            .map(|source| {
                if source.file_mode {
                    Vec::new()
                } else {
                    read_input_source(
                        &InputSource::Inline {
                            value: source.value.clone(),
                            delimiter: self.list_delimiter.clone(),
                        },
                        InputLimits {
                            max_input_bytes: self.request.max_input_bytes,
                            max_item_bytes: self.request.max_item_bytes,
                            max_items_per_list: self.request.max_items_per_list,
                        },
                    )
                    .unwrap_or_default()
                }
            })
            .collect();
        self.join_request.left_path = self.join_left.clone();
        self.join_request.right_path = self.join_right.clone();
        self.join_request.left_key = self.join_left_key.clone();
        self.join_request.right_key = self.join_right_key.clone();
        self.join_request.format = self.join_format;
        self.join_request.kind = self.join_kind;
        self.join_request.offset = self.join_offset.parse().unwrap_or(0);
        self.join_request.limit =
            (!self.join_limit.is_empty()).then(|| self.join_limit.parse().unwrap_or(0));
        self.join_request.max_join_records = limits.max_join_records;
        self.join_request.max_join_key_fanout = limits.max_join_key_fanout;
        self.join_request.max_output_bytes = self.request.max_output_bytes;
        self.join_request.max_input_bytes = self.request.max_input_bytes;
        self.join_request.max_item_bytes = self.request.max_item_bytes;
        self.join_request.timeout_ms = self.request.timeout_ms;
        true
    }

    fn refresh_plan(&mut self) {
        if self.page == Page::Join {
            if self.join_left.is_empty()
                || self.join_right.is_empty()
                || self.join_left_key.is_empty()
                || self.join_right_key.is_empty()
            {
                self.join_plan = None;
                self.status = "Enter both paths and both keys".into();
                return;
            }
            match join_plan(&self.join_request) {
                Ok(value) => {
                    self.join_plan = Some(value);
                    self.error = None;
                    self.status = "Join ready".into();
                }
                Err(error) => {
                    self.join_plan = None;
                    self.set_error(error);
                }
            }
            return;
        }
        self.plan = None;
        let limits = InputLimits {
            max_input_bytes: self.request.max_input_bytes,
            max_item_bytes: self.request.max_item_bytes,
            max_items_per_list: self.request.max_items_per_list,
        };
        let mut lists = Vec::new();
        for source in &self.sources {
            let input = if source.file_mode {
                InputSource::File {
                    path: source.file_path.clone(),
                    format: source.format,
                }
            } else {
                InputSource::Inline {
                    value: source.value.clone(),
                    delimiter: self.list_delimiter.clone(),
                }
            };
            match read_input_source(&input, limits) {
                Ok(list) => lists.push(list),
                Err(error) => {
                    self.set_error(error);
                    return;
                }
            }
        }
        self.request.lists = lists;
        match plan(&self.request) {
            Ok(value) => {
                self.plan = Some(value);
                self.error = None;
                self.status = "Ready".into();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn set_error(&mut self, error: AppError) {
        self.error = Some(format!("{}: {}", error.code, error.message));
    }

    fn run_preview(&mut self) {
        if self.page == Page::Settings {
            return;
        }
        if !self.sync_requests() {
            return;
        }
        self.refresh_plan();
        if self.page == Page::Join {
            match join_preview(&self.join_request, PREVIEW_LIMIT) {
                Ok(records) => {
                    self.records = records;
                    self.preview_scroll = 0;
                    self.status = "Join preview ready".into();
                    self.error = None;
                }
                Err(error) => self.set_error(error),
            }
        } else {
            match preview(&self.request, PREVIEW_LIMIT) {
                Ok(records) => {
                    self.records = records;
                    self.preview_scroll = 0;
                    self.status = "Preview ready".into();
                    self.error = None;
                }
                Err(error) => self.set_error(error),
            }
        }
    }

    fn start_generation(&mut self) {
        if self.running || self.page == Page::Settings {
            return;
        }
        if !self.sync_requests() {
            return;
        }
        self.refresh_plan();
        if (self.page == Page::Join && self.join_plan.is_none())
            || (self.page == Page::Combine && self.plan.is_none())
        {
            return;
        }
        let sink = match FileSink::open(&self.output_path, self.overwrite) {
            Ok(sink) => sink,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let (messages, receiver) = mpsc::channel();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        self.running = true;
        self.cancellation = Some(token);
        self.worker = Some(receiver);
        self.progress = Some(ProgressEvent {
            records: 0,
            bytes: 0,
        });
        self.status = "Generating…".into();
        self.error = None;
        if self.page == Page::Join {
            let request = self.join_request.clone();
            thread::spawn(move || {
                let mut sink = ProgressSink { sink, messages };
                let result =
                    match join_stream(&request, &mut sink, Some(&|| worker_token.is_cancelled())) {
                        Ok(progress) => sink.sink.commit().map(|_| progress),
                        Err(error) => Err(error),
                    };
                let _ = sink.messages.send(WorkerMessage::Finished(result));
            });
        } else {
            let request = self.request.clone();
            thread::spawn(move || {
                let mut sink = ProgressSink { sink, messages };
                let result =
                    match stream(&request, &mut sink, Some(&|| worker_token.is_cancelled())) {
                        Ok(progress) => sink.sink.commit().map(|_| progress),
                        Err(error) => Err(error),
                    };
                let _ = sink.messages.send(WorkerMessage::Finished(result));
            });
        }
    }

    fn cancel_generation(&mut self) {
        if let Some(token) = &self.cancellation {
            token.cancel();
            self.status = "Cancellation requested…".into();
        }
    }

    fn save_profile(&mut self) {
        let path = PathBuf::from(self.profile_path.trim());
        if path.as_os_str().is_empty() {
            self.error = Some("PROFILE_SAVE_FAILED: enter a profile path in Settings".into());
            return;
        }
        match save_profile_file(&path, self.to_profile()) {
            Ok(()) => {
                self.profile_path = path.to_string_lossy().into_owned();
                remember_profile(&mut self.preferences, &path);
                if let Err(error) = save_preferences(&self.preferences) {
                    self.error = Some(format!("PREFERENCES_SAVE_FAILED: {error}"));
                    return;
                }
                self.error = None;
                self.status = format!("Saved profile {}", path.display());
            }
            Err(error) => self.error = Some(format!("PROFILE_SAVE_FAILED: {error}")),
        }
    }

    fn open_profile(&mut self) {
        let path = PathBuf::from(self.profile_path.trim());
        if path.as_os_str().is_empty() {
            self.error = Some("PROFILE_LOAD_FAILED: enter a profile path in Settings".into());
            return;
        }
        let display_path = path.display().to_string();
        match load_profile_file(&path) {
            Ok(profile) => {
                self.apply_profile(profile);
                self.profile_path = path.to_string_lossy().into_owned();
                remember_profile(&mut self.preferences, &path);
                let _ = save_preferences(&self.preferences);
                self.error = None;
                self.status = format!("Loaded profile {display_path}");
            }
            Err(error) => self.error = Some(format!("PROFILE_LOAD_FAILED: {error}")),
        }
    }

    fn limits_profile(&self) -> LimitsProfile {
        LimitsProfile {
            max_combinations: self.max_combinations.clone(),
            max_output_bytes: self.max_output_bytes.clone(),
            max_input_bytes: self.max_input_bytes.clone(),
            max_item_bytes: self.max_item_bytes.clone(),
            max_items_per_list: self.max_items_per_list.clone(),
            max_total_items: self.max_total_items.clone(),
            max_lists: self.max_lists.clone(),
            timeout_ms: self.timeout.clone(),
            join_max_records: self.join_max_records.clone(),
            join_fanout: self.join_fanout.clone(),
        }
    }

    fn to_profile(&self) -> Profile {
        Profile {
            version: PROFILE_VERSION,
            active_mode: match self.page {
                Page::Join => "join",
                Page::Combine | Page::Settings => "combine",
            }
            .into(),
            combine: CombineProfile {
                sources: self
                    .sources
                    .iter()
                    .map(|source| source.value.clone())
                    .collect(),
                file_sources: self
                    .sources
                    .iter()
                    .map(|source| source.file_mode.then(|| source.file_path.clone()))
                    .collect(),
                file_formats: self
                    .sources
                    .iter()
                    .map(|source| input_format_label(source.format).into())
                    .collect(),
                list_delimiter: self.list_delimiter.clone(),
                field_separator: self.field_separator.clone(),
                template: self.template.clone(),
                template_file: self.template_file.clone(),
                template_file_mode: self.template_file_mode,
                transforms: self.transforms.clone(),
                filters: self.filters.clone(),
                names: self.names.clone(),
                offset: self.offset.clone(),
                limit: self.limit.clone(),
                choose: self.choose.clone(),
                length: self.length.clone(),
                operation: operation_label(self.operation).into(),
                format: format_label(self.format).into(),
                formula_policy: formula_policy_label(self.formula_policy).into(),
                zip_policy: zip_policy_label(self.zip_policy).into(),
                reverse: self.reverse,
                reverse_fields: self.reverse_fields,
                lean_jsonl: self.lean_jsonl,
                shard_index: self.shard_index.clone(),
                shard_count: self.shard_count.clone(),
            },
            join: JoinProfile {
                left_path: self.join_left.clone(),
                right_path: self.join_right.clone(),
                left_key: self.join_left_key.clone(),
                right_key: self.join_right_key.clone(),
                format: join_format_label(self.join_format).into(),
                kind: join_kind_label(self.join_kind).into(),
                offset: self.join_offset.clone(),
                limit: self.join_limit.clone(),
            },
            output_path: self.output_path.clone(),
            overwrite: self.overwrite,
            limits: self.limits_profile(),
        }
    }

    fn apply_profile(&mut self, profile: Profile) {
        let combine = profile.combine;
        let join = profile.join;
        let limits = profile.limits;
        self.page = if profile.active_mode == "join" {
            Page::Join
        } else {
            Page::Combine
        };
        self.sources = combine
            .sources
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let file_path = combine
                    .file_sources
                    .get(index)
                    .and_then(Clone::clone)
                    .unwrap_or_default();
                Source {
                    value,
                    file_mode: !file_path.is_empty()
                        || combine.file_sources.get(index).is_some_and(Option::is_some),
                    file_path,
                    format: combine
                        .file_formats
                        .get(index)
                        .map(|format| parse_input_format(format))
                        .unwrap_or(InputFormat::Lines),
                }
            })
            .collect();
        if self.sources.is_empty() {
            self.sources.push(Source::default());
        }
        self.selected_source = 0;
        self.list_delimiter = combine.list_delimiter;
        self.operation = parse_operation(
            &combine.operation,
            combine.choose.parse().unwrap_or(2),
            combine.length.parse().unwrap_or(2),
        );
        self.format = parse_format(&combine.format);
        self.formula_policy = parse_formula_policy(&combine.formula_policy);
        self.field_separator = combine.field_separator;
        self.zip_policy = parse_zip_policy(&combine.zip_policy);
        self.reverse = combine.reverse;
        self.reverse_fields = combine.reverse_fields;
        self.lean_jsonl = combine.lean_jsonl;
        self.choose = combine.choose;
        self.length = combine.length;
        self.template = combine.template;
        self.template_file = combine.template_file;
        self.template_file_mode = combine.template_file_mode;
        self.transforms = combine.transforms;
        self.filters = combine.filters;
        self.names = combine.names;
        self.offset = combine.offset;
        self.limit = combine.limit;
        self.shard_index = combine.shard_index;
        self.shard_count = combine.shard_count;
        self.output_path = profile.output_path;
        self.overwrite = profile.overwrite;
        self.join_left = join.left_path;
        self.join_right = join.right_path;
        self.join_left_key = join.left_key;
        self.join_right_key = join.right_key;
        self.join_format = parse_join_format(&join.format);
        self.join_kind = parse_join_kind(&join.kind);
        self.join_offset = join.offset;
        self.join_limit = join.limit;
        self.join_max_records = limits.join_max_records;
        self.join_fanout = limits.join_fanout;
        self.max_combinations = limits.max_combinations;
        self.max_output_bytes = limits.max_output_bytes;
        self.max_input_bytes = limits.max_input_bytes;
        self.max_item_bytes = limits.max_item_bytes;
        self.max_items_per_list = limits.max_items_per_list;
        self.max_total_items = limits.max_total_items;
        self.max_lists = limits.max_lists;
        self.timeout = limits.timeout_ms;
        self.focus = Focus::Page(self.page);
        self.editing = None;
        self.records.clear();
        self.preview_scroll = 0;
        if self.sync_requests() {
            self.refresh_plan();
        }
    }
    fn poll_worker(&mut self) {
        let mut finished = None;
        if let Some(worker) = &self.worker {
            for message in worker.try_iter() {
                match message {
                    WorkerMessage::Progress(progress) => {
                        self.progress = Some(progress);
                        self.status = format!(
                            "Generating… {} records ({} bytes)",
                            progress.records, progress.bytes
                        );
                    }
                    WorkerMessage::Finished(result) => finished = Some(result),
                }
            }
        }
        if let Some(result) = finished {
            self.running = false;
            self.cancellation = None;
            self.worker = None;
            match result {
                Ok(progress) => {
                    self.status = format!(
                        "Wrote {} records ({} bytes)",
                        progress.records, progress.bytes
                    )
                }
                Err(error) => self.set_error(error),
            }
        }
    }
}

fn save_profile_file(path: &Path, mut profile: Profile) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    relativize_profile_paths(&mut profile, parent);
    let mut text = serde_json::to_string_pretty(&profile)
        .map_err(|error| format!("could not encode profile: {error}"))?;
    text.push('\n');
    if text.len() as u64 > MAX_PROFILE_BYTES {
        return Err(format!(
            "profile exceeds the {} byte limit",
            MAX_PROFILE_BYTES
        ));
    }
    atomic_write_text(path, &text)
}

fn load_profile_file(path: &Path) -> Result<Profile, String> {
    let text = read_bounded_utf8(path, MAX_PROFILE_BYTES)?;
    let mut profile: Profile = serde_json::from_str(&text)
        .map_err(|error| format!("profile is not valid JSON: {error}"))?;
    if profile.version != PROFILE_VERSION {
        return Err(format!(
            "unsupported profile version {}; expected {}",
            profile.version, PROFILE_VERSION
        ));
    }
    parse_profile_limits(&profile.limits)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    resolve_profile_paths(
        &mut profile,
        path.parent().unwrap_or_else(|| Path::new(".")),
    );
    Ok(profile)
}

fn parse_profile_limits(limits: &LimitsProfile) -> Result<ResourceLimits, AppError> {
    let parsed = ResourceLimits {
        max_output_bytes: parse_profile_u128(&limits.max_output_bytes, "max-output-bytes")?,
        max_input_bytes: parse_profile_usize(&limits.max_input_bytes, "max-input-bytes")?,
        max_item_bytes: parse_profile_usize(&limits.max_item_bytes, "max-item-bytes")?,
        max_items_per_list: parse_profile_usize(&limits.max_items_per_list, "max-items-per-list")?,
        max_lists: parse_profile_usize(&limits.max_lists, "max-lists")?,
        max_total_items: parse_profile_usize(&limits.max_total_items, "max-total-items")?,
        max_combinations: parse_profile_u128(&limits.max_combinations, "max-combinations")?,
        max_join_records: parse_profile_usize(&limits.join_max_records, "max-join-records")?,
        max_join_key_fanout: parse_profile_u128(&limits.join_fanout, "max-join-key-fanout")?,
        timeout_ms: if limits.timeout_ms.is_empty() {
            None
        } else {
            Some(parse_profile_u64(&limits.timeout_ms, "timeout-ms")?)
        },
    };
    validate_resource_limits(&parsed).map_err(|error| AppError {
        code: "RESOURCE_LIMIT_TOO_HIGH",
        message: error.to_string(),
    })?;
    Ok(parsed)
}

fn parse_profile_u128(value: &str, field: &str) -> Result<u128, AppError> {
    value.parse().map_err(|_| AppError {
        code: "LIMIT_INVALID",
        message: format!("{field} must be a non-negative integer"),
    })
}

fn parse_profile_u64(value: &str, field: &str) -> Result<u64, AppError> {
    value.parse().map_err(|_| AppError {
        code: "LIMIT_INVALID",
        message: format!("{field} must be a non-negative integer"),
    })
}

fn parse_profile_usize(value: &str, field: &str) -> Result<usize, AppError> {
    value.parse().map_err(|_| AppError {
        code: "LIMIT_INVALID",
        message: format!("{field} must be a non-negative integer"),
    })
}

fn read_bounded_utf8(path: &Path, max_bytes: u64) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("could not read file: {error}"))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read file: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("file exceeds the {max_bytes} byte limit"));
    }
    String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8".into())
}

fn resolve_profile_paths(profile: &mut Profile, base: &Path) {
    profile.output_path = resolve_path(&profile.output_path, base);
    profile.combine.template_file = resolve_path(&profile.combine.template_file, base);
    for path in profile.combine.file_sources.iter_mut().flatten() {
        *path = resolve_path(path, base);
    }
    profile.join.left_path = resolve_path(&profile.join.left_path, base);
    profile.join.right_path = resolve_path(&profile.join.right_path, base);
}

fn relativize_profile_paths(profile: &mut Profile, base: &Path) {
    profile.output_path = stored_path(&profile.output_path, base);
    profile.combine.template_file = stored_path(&profile.combine.template_file, base);
    for path in profile.combine.file_sources.iter_mut().flatten() {
        *path = stored_path(path, base);
    }
    profile.join.left_path = stored_path(&profile.join.left_path, base);
    profile.join.right_path = stored_path(&profile.join.right_path, base);
}

fn resolve_path(path: &str, base: &Path) -> String {
    if path.is_empty() {
        return String::new();
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        path.into()
    } else {
        base.join(candidate).to_string_lossy().into_owned()
    }
}

fn stored_path(path: &str, base: &Path) -> String {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        if let Ok(relative) = candidate.strip_prefix(base) {
            return relative.to_string_lossy().into_owned();
        }
    }
    path.into()
}

fn load_preferences() -> Preferences {
    preferences_path()
        .and_then(|path| read_bounded_utf8(&path, MAX_PROFILE_BYTES).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_preferences(preferences: &Preferences) -> Result<(), String> {
    let Some(path) = preferences_path() else {
        return Ok(());
    };
    ensure_output_parent(&path).map_err(|error| format!("{}: {}", error.code, error.message))?;
    let text = serde_json::to_string_pretty(preferences)
        .map_err(|error| format!("could not encode preferences: {error}"))?;
    atomic_write_text(&path, &format!("{text}\n"))
}

fn atomic_write_text(path: &Path, contents: &str) -> Result<(), String> {
    let mut sink =
        FileSink::open(path, true).map_err(|error| format!("{}: {}", error.code, error.message))?;
    sink.record(OutputRecord {
        ordinal: 0,
        value: contents.to_string(),
        fields: Vec::new(),
    })
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    sink.commit()
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

fn remember_profile(preferences: &mut Preferences, path: &Path) {
    let path = path.to_string_lossy().into_owned();
    preferences.recent_profiles.retain(|item| item != &path);
    preferences.recent_profiles.insert(0, path.clone());
    preferences.recent_profiles.truncate(8);
    preferences.last_profile = Some(path);
}

fn preferences_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(
            PathBuf::from(appdata)
                .join("Combinator")
                .join("preferences.json"),
        );
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|base| base.join("combinator").join("preferences.json"))
}

fn is_text_field(field: Field) -> bool {
    !matches!(
        field,
        Field::InputFormat
            | Field::FileMode
            | Field::Operation
            | Field::ZipPolicy
            | Field::Format
            | Field::FormulaPolicy
            | Field::TemplateFileMode
            | Field::JoinFormat
            | Field::JoinKind
            | Field::Reverse
            | Field::ReverseFields
            | Field::LeanJsonl
            | Field::Overwrite
    )
}

fn field_from_focus(focus: Focus) -> Option<Field> {
    match focus {
        Focus::Field(field) => Some(field),
        _ => None,
    }
}

fn panel_scroll(lines: &[String], height: u16) -> u16 {
    let content_height = usize::from(height.saturating_sub(2)).max(1);
    let focused = lines
        .iter()
        .position(|line| line.starts_with("▶ ") || line.starts_with("[▶"))
        .or_else(|| lines.iter().position(|line| line.starts_with("• ")));
    focused
        .map(|index| index.saturating_sub(content_height / 2))
        .unwrap_or(0)
        .try_into()
        .unwrap_or(u16::MAX)
}

fn terminal_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars().peekable();
    let mut rendered = String::new();
    for character in chars.by_ref().take(max_chars) {
        match character {
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(rendered, "\\u{{{:x}}}", u32::from(character));
            }
            character => rendered.push(character),
        }
    }
    if chars.peek().is_some() {
        rendered.push('…');
    }
    rendered
}

fn plan_row(metrics: &[(&str, String)], width: usize) -> String {
    const SEPARATOR: &str = " │ ";
    if metrics.is_empty() || width == 0 {
        return String::new();
    }
    let separators = SEPARATOR.chars().count().saturating_mul(metrics.len() - 1);
    let cell_width = width.saturating_sub(separators) / metrics.len();
    metrics
        .iter()
        .map(|(label, value)| {
            let text = format!("{label}: {value}");
            let mut chars = text.chars();
            let mut cell = chars.by_ref().take(cell_width).collect::<String>();
            if chars.next().is_some() && cell_width > 0 {
                cell.pop();
                cell.push('…');
            }
            format!("{cell:<cell_width$}")
        })
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

fn format_uses_field_separator(format: Format) -> bool {
    matches!(format, Format::Text | Format::Jsonl | Format::Nul)
}

fn checkbox(value: bool) -> &'static str {
    if value {
        "x"
    } else {
        " "
    }
}
fn split_values(value: &str) -> Vec<String> {
    value
        .split([';', '\n'])
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}
fn operation_label(operation: AppOperation) -> &'static str {
    match operation {
        AppOperation::Product { .. } => "Product",
        AppOperation::Zip { .. } => "Zip",
        AppOperation::Concat => "Concat",
        AppOperation::Permutations => "Permutations",
        AppOperation::Combinations { .. } => "Combinations",
        AppOperation::Variations { .. } => "Variations",
    }
}
fn format_label(format: Format) -> &'static str {
    match format {
        Format::Text => "Text",
        Format::Jsonl => "JSONL",
        Format::Csv => "CSV",
        Format::Tsv => "TSV",
        Format::Nul => "NUL",
    }
}
fn formula_policy_label(policy: FormulaPolicy) -> &'static str {
    match policy {
        FormulaPolicy::Allow => "Allow",
        FormulaPolicy::Warn => "Warn",
        FormulaPolicy::Reject => "Reject",
    }
}
fn zip_policy_label(policy: UnequalPolicy) -> &'static str {
    match policy {
        UnequalPolicy::Error => "Error",
        UnequalPolicy::Truncate => "Truncate",
        UnequalPolicy::Cycle => "Cycle",
    }
}
fn input_format_label(format: InputFormat) -> &'static str {
    match format {
        InputFormat::Lines => "Lines",
        InputFormat::Csv => "CSV",
        InputFormat::Tsv => "TSV",
        InputFormat::Nul => "NUL",
    }
}
fn join_format_label(format: JoinFormat) -> &'static str {
    match format {
        JoinFormat::Csv => "CSV",
        JoinFormat::Tsv => "TSV",
        JoinFormat::Jsonl => "JSONL",
    }
}
fn join_kind_label(kind: JoinKind) -> &'static str {
    match kind {
        JoinKind::Inner => "Inner",
        JoinKind::Left => "Left",
        JoinKind::Full => "Full",
        JoinKind::Anti => "Anti",
    }
}
fn focus_label(focus: Focus) -> &'static str {
    match focus {
        Focus::Page(Page::Combine) => "Combine tab",
        Focus::Page(Page::Join) => "Join tab",
        Focus::Page(Page::Settings) => "Settings tab",
        Focus::Action(Action::AddList) => "Add list",
        Focus::Action(Action::RemoveList) => "Remove list",
        Focus::Action(Action::Preview) => "Preview",
        Focus::Action(Action::Generate) => "Generate",
        Focus::Action(Action::Cancel) => "Cancel",
        Focus::Action(Action::New) => "New",
        Focus::Action(Action::OpenProfile) => "Open profile",
        Focus::Action(Action::SaveProfile) => "Save profile",
        Focus::Action(Action::SaveAsProfile) => "Save profile as",
        Focus::Action(Action::About) => "About",
        Focus::Field(_) => "form field",
    }
}
fn next_operation(value: AppOperation) -> AppOperation {
    match value {
        AppOperation::Product { .. } => AppOperation::Zip {
            on_unequal: UnequalPolicy::Error,
        },
        AppOperation::Zip { .. } => AppOperation::Concat,
        AppOperation::Concat => AppOperation::Permutations,
        AppOperation::Permutations => AppOperation::Combinations { choose: 2 },
        AppOperation::Combinations { .. } => AppOperation::Variations { length: 2 },
        AppOperation::Variations { .. } => AppOperation::Product {
            reverse_fields: false,
        },
    }
}
fn next_format(value: Format) -> Format {
    match value {
        Format::Text => Format::Jsonl,
        Format::Jsonl => Format::Csv,
        Format::Csv => Format::Tsv,
        Format::Tsv => Format::Nul,
        Format::Nul => Format::Text,
    }
}
fn next_formula_policy(value: FormulaPolicy) -> FormulaPolicy {
    match value {
        FormulaPolicy::Warn => FormulaPolicy::Reject,
        FormulaPolicy::Reject => FormulaPolicy::Allow,
        FormulaPolicy::Allow => FormulaPolicy::Warn,
    }
}
fn next_zip_policy(value: UnequalPolicy) -> UnequalPolicy {
    match value {
        UnequalPolicy::Error => UnequalPolicy::Truncate,
        UnequalPolicy::Truncate => UnequalPolicy::Cycle,
        UnequalPolicy::Cycle => UnequalPolicy::Error,
    }
}
fn next_input_format(value: InputFormat) -> InputFormat {
    match value {
        InputFormat::Lines => InputFormat::Csv,
        InputFormat::Csv => InputFormat::Tsv,
        InputFormat::Tsv => InputFormat::Nul,
        InputFormat::Nul => InputFormat::Lines,
    }
}
fn next_join_format(value: JoinFormat) -> JoinFormat {
    match value {
        JoinFormat::Csv => JoinFormat::Tsv,
        JoinFormat::Tsv => JoinFormat::Jsonl,
        JoinFormat::Jsonl => JoinFormat::Csv,
    }
}
fn next_join_kind(value: JoinKind) -> JoinKind {
    match value {
        JoinKind::Inner => JoinKind::Left,
        JoinKind::Left => JoinKind::Full,
        JoinKind::Full => JoinKind::Anti,
        JoinKind::Anti => JoinKind::Inner,
    }
}

fn parse_input_format(value: &str) -> InputFormat {
    match value {
        "CSV" => InputFormat::Csv,
        "TSV" => InputFormat::Tsv,
        "NUL" => InputFormat::Nul,
        _ => InputFormat::Lines,
    }
}
fn parse_format(value: &str) -> Format {
    match value {
        "JSONL" => Format::Jsonl,
        "CSV" => Format::Csv,
        "TSV" => Format::Tsv,
        "NUL" => Format::Nul,
        _ => Format::Text,
    }
}
fn parse_formula_policy(value: &str) -> FormulaPolicy {
    match value {
        "Allow" => FormulaPolicy::Allow,
        "Reject" => FormulaPolicy::Reject,
        _ => FormulaPolicy::Warn,
    }
}
fn parse_operation(value: &str, choose: usize, length: usize) -> AppOperation {
    match value {
        "Zip" => AppOperation::Zip {
            on_unequal: UnequalPolicy::Error,
        },
        "Concat" => AppOperation::Concat,
        "Permutations" => AppOperation::Permutations,
        "Combinations" => AppOperation::Combinations { choose },
        "Variations" => AppOperation::Variations { length },
        _ => AppOperation::Product {
            reverse_fields: false,
        },
    }
}
fn parse_zip_policy(value: &str) -> UnequalPolicy {
    match value {
        "Truncate" => UnequalPolicy::Truncate,
        "Cycle" => UnequalPolicy::Cycle,
        _ => UnequalPolicy::Error,
    }
}
fn parse_join_format(value: &str) -> JoinFormat {
    match value {
        "TSV" => JoinFormat::Tsv,
        "JSONL" => JoinFormat::Jsonl,
        _ => JoinFormat::Csv,
    }
}
fn parse_join_kind(value: &str) -> JoinKind {
    match value {
        "Left" => JoinKind::Left,
        "Full" => JoinKind::Full,
        "Anti" => JoinKind::Anti,
        _ => JoinKind::Inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn pages_render_at_standard_terminal_size() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut app = App::default();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let combine_text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(combine_text.contains("Inputs"));
        assert!(combine_text.contains("Data Selection and pre-processing"));
        assert!(combine_text.contains("Execution plan"));
        assert!(combine_text.contains("Lists: 1"));
        assert!(combine_text.contains("Combos: Exact(1)"));
        assert!(combine_text.contains("Selected: 1"));
        assert!(combine_text.contains("Bytes: Bytes(1)"));
        assert!(combine_text.contains("Lists: [ previous"));
        assert!(combine_text.contains("] next"));
        app.page = Page::Join;
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let join_text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(join_text.contains("Structured join"));
        assert!(join_text.contains("Preview"));
        app.page = Page::Settings;
        terminal.draw(|frame| app.draw(frame)).unwrap();
    }

    #[test]
    fn about_action_opens_and_escape_closes_modal() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut app = App {
            focus: Focus::Action(Action::About),
            ..App::default()
        };
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.about_open);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("About tz_combinator"));
        assert!(text.contains("Bug reports:"));

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.about_open);
    }

    #[test]
    fn small_terminal_renders_resize_guidance() {
        let mut terminal = Terminal::new(TestBackend::new(60, 15)).unwrap();
        let app = App::default();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("terminal too small"));
        assert!(text.contains("100 columns"));
    }

    #[test]
    fn tab_order_reaches_selector_and_button_controls() {
        let mut app = App::default();
        assert_eq!(app.focus, Focus::Page(Page::Combine));
        let format_index = app
            .focus_order()
            .iter()
            .position(|focus| *focus == Focus::Field(Field::Format))
            .unwrap();
        for _ in 0..format_index {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        assert_eq!(app.focus, Focus::Field(Field::Format));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.format, Format::Jsonl);
        app.focus = Focus::Action(Action::AddList);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.sources.len(), 2);
    }

    #[test]
    fn combine_form_drives_shared_plan_and_preview_workflow() {
        let mut app = App::default();
        app.sources[0].value = "red,blue".into();
        app.add_list();
        app.sources[1].value = "car,bike".into();
        app.field_separator = "-".into();
        app.sync_requests();
        app.refresh_plan();
        app.run_preview();
        assert_eq!(app.plan.as_ref().map(|plan| plan.records_to_emit), Some(4));
        assert_eq!(app.records.len(), 4);
        assert_eq!(app.records[0].value, "red-car\n");
    }

    #[test]
    fn profile_actions_round_trip_keyboard_state() {
        let path = std::env::temp_dir().join(format!(
            "combinator-tui-profile-{}.json",
            std::process::id()
        ));
        let mut app = App::default();
        app.sources[0].value = "one,two".into();
        app.field_separator = "|".into();
        app.formula_policy = FormulaPolicy::Reject;
        save_profile_file(&path, app.to_profile()).unwrap();

        let mut loaded = App::default();
        loaded.apply_profile(load_profile_file(&path).unwrap());
        assert_eq!(loaded.sources[0].value, "one,two");
        assert_eq!(loaded.field_separator, "|");
        assert_eq!(loaded.formula_policy, FormulaPolicy::Reject);
        assert!(loaded.error.is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn excessive_limits_do_not_mutate_requests_or_load_from_profiles() {
        let mut app = App::default();
        let original_combinations = app.request.max_combinations;
        app.max_combinations = (combinator_app::HARD_MAX_COMBINATIONS + 1).to_string();
        assert!(!app.sync_requests());
        assert_eq!(app.request.max_combinations, original_combinations);
        assert!(app.error.as_deref().is_some_and(|error| {
            error.starts_with("RESOURCE_LIMIT_TOO_HIGH:") && error.contains("max-combinations")
        }));

        let path = std::env::temp_dir().join(format!(
            "combinator-tui-profile-limits-{}.json",
            std::process::id()
        ));
        let mut profile = App::default().to_profile();
        profile.limits.join_fanout = (combinator_app::HARD_MAX_JOIN_KEY_FANOUT + 1).to_string();
        save_profile_file(&path, profile).expect("save hostile profile");
        let error = load_profile_file(&path).unwrap_err();
        assert!(error.starts_with("RESOURCE_LIMIT_TOO_HIGH:"));
        assert!(error.contains("max-join-key-fanout"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn focus_order_contains_only_controls_visible_for_current_choices() {
        let mut app = App::default();
        assert!(!app.focus_order().contains(&Focus::Field(Field::ZipPolicy)));
        assert!(app
            .focus_order()
            .contains(&Focus::Field(Field::ReverseFields)));
        assert!(!app.focus_order().contains(&Focus::Field(Field::LeanJsonl)));
        assert!(!app
            .focus_order()
            .contains(&Focus::Field(Field::FormulaPolicy)));

        app.operation = AppOperation::Zip {
            on_unequal: UnequalPolicy::Error,
        };
        app.format = Format::Jsonl;
        let order = app.focus_order();
        assert!(order.contains(&Focus::Field(Field::ZipPolicy)));
        assert!(!order.contains(&Focus::Field(Field::ReverseFields)));
        assert!(order.contains(&Focus::Field(Field::LeanJsonl)));
        assert!(order.contains(&Focus::Field(Field::Names)));

        app.format = Format::Csv;
        let order = app.focus_order();
        assert!(order.contains(&Focus::Field(Field::FormulaPolicy)));
    }

    #[test]
    fn reject_policy_does_not_open_generation_destination() {
        let path = std::env::temp_dir().join(format!(
            "combinator-tui-formula-reject-{}.csv",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut app = App::default();
        app.sources[0].value = "=hostile".into();
        app.format = Format::Csv;
        app.formula_policy = FormulaPolicy::Reject;
        app.output_path = path.to_string_lossy().into_owned();
        app.start_generation();
        assert!(!path.exists());
        assert!(app
            .error
            .as_deref()
            .is_some_and(|error| error.contains("DOWNSTREAM_INTERPRETATION_RISK")));
    }

    #[test]
    fn terminal_text_escapes_controls_and_bounds_rendering() {
        assert_eq!(
            terminal_text("safe\x1b[31m\nnext", 64),
            "safe\\u{1b}[31m\\nnext"
        );
        assert_eq!(terminal_text("abcdef", 3), "abc…");
    }

    #[test]
    fn keyboard_editing_updates_input_and_plan() {
        let mut app = App {
            focus: Focus::Field(Field::ListValue),
            ..App::default()
        };
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for character in ['r', 'e', 'd', ',', 'b', 'l', 'u', 'e'] {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.sources[0].value, "red,blue");
        assert_eq!(app.request.lists[0], vec!["red", "blue"]);
        assert_eq!(app.plan.as_ref().map(|plan| plan.records_to_emit), Some(2));
        assert!(app.editing.is_none());
    }

    #[test]
    fn keyboard_navigation_changes_pages_and_quits() {
        let mut app = App::default();
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)));
        assert_eq!(app.page, Page::Join);
        assert_eq!(app.focus, Focus::Page(Page::Join));
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)));
        assert_eq!(app.page, Page::Settings);
        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
        assert!(app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn editing_shortcuts_clear_and_finish_fields() {
        let mut app = App::default();
        app.sources[0].value = "old".into();
        app.focus = Focus::Field(Field::ListValue);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(app.sources[0].value, "");
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.editing.is_none());
    }

    #[test]
    fn operation_controls_change_request_shape_and_visible_focus() {
        let mut app = App {
            operation: AppOperation::Combinations { choose: 2 },
            choose: "3".into(),
            ..App::default()
        };
        app.sync_requests();
        assert_eq!(
            app.request.operation,
            AppOperation::Combinations { choose: 3 }
        );
        assert!(app.focus_order().contains(&Focus::Field(Field::Choose)));

        app.operation = AppOperation::Variations { length: 2 };
        app.length = "4".into();
        app.sync_requests();
        assert_eq!(
            app.request.operation,
            AppOperation::Variations { length: 4 }
        );

        app.operation = AppOperation::Zip {
            on_unequal: UnequalPolicy::Cycle,
        };
        app.zip_policy = UnequalPolicy::Cycle;
        app.sync_requests();
        assert_eq!(app.request.operation, app.operation);
        assert!(app.focus_order().contains(&Focus::Field(Field::ZipPolicy)));
    }

    #[test]
    fn invalid_input_stays_fail_closed() {
        let mut app = App {
            focus: Focus::Field(Field::Offset),
            ..App::default()
        };
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.request.options.offset, 0);
        assert_eq!(app.offset, "0x");

        app.page = Page::Join;
        app.refresh_plan();
        assert!(app.join_plan.is_none());
        assert_eq!(app.status, "Enter both paths and both keys");
    }

    #[test]
    fn rendering_exposes_active_state_without_snapshot_lock_in() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut app = App::default();
        app.sources[0].value = "red,blue".into();
        app.sync_requests();
        app.run_preview();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(text.contains("Preview"));
        assert!(text.contains("red"));
        assert!(text.contains("blue"));
    }

    #[test]
    fn supported_labels_and_parsers_are_consistent() {
        for operation in [
            AppOperation::Product {
                reverse_fields: false,
            },
            AppOperation::Zip {
                on_unequal: UnequalPolicy::Error,
            },
            AppOperation::Concat,
            AppOperation::Permutations,
            AppOperation::Combinations { choose: 2 },
            AppOperation::Variations { length: 2 },
        ] {
            assert_eq!(parse_operation(operation_label(operation), 2, 2), operation);
        }
        for format in [
            Format::Text,
            Format::Jsonl,
            Format::Csv,
            Format::Tsv,
            Format::Nul,
        ] {
            assert_eq!(parse_format(format_label(format)), format);
        }
        for policy in [
            FormulaPolicy::Allow,
            FormulaPolicy::Warn,
            FormulaPolicy::Reject,
        ] {
            assert_eq!(parse_formula_policy(formula_policy_label(policy)), policy);
        }
        for kind in [
            JoinKind::Inner,
            JoinKind::Left,
            JoinKind::Full,
            JoinKind::Anti,
        ] {
            assert_eq!(parse_join_kind(join_kind_label(kind)), kind);
        }
    }
}
