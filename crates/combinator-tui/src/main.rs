use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use combinator_app::{
    join_plan, join_preview, join_stream, plan, preview, read_input_source, stream, AppOperation,
    CancellationToken, ExecutionPlan, FileSink, Format, InputFormat, InputLimits, InputSource,
    JoinFormat, JoinKind, JoinPlan, JoinRequest, OutputRecord, OutputSink, PreviewRecord,
    ProductRequest, ProgressEvent, UnequalPolicy,
};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

const PREVIEW_LIMIT: u128 = 20;

enum WorkerMessage {
    Progress(ProgressEvent),
    Finished(Result<ProgressEvent, combinator_app::AppError>),
}

#[derive(Clone, Copy)]
enum EditTarget {
    List,
    File,
    Output,
    MaxCombinations,
    MaxOutputBytes,
    Template,
    Transforms,
    Names,
    Offset,
    Limit,
    Choose,
    Length,
    Delimiter,
    TemplateFile,
    Filters,
    Timeout,
    ShardIndex,
    ShardCount,
    ResourceLimits,
    JoinLeft,
    JoinRight,
    JoinLeftKey,
    JoinRightKey,
    JoinFanout,
}

struct ProgressFileSink {
    sink: FileSink,
    messages: Sender<WorkerMessage>,
}

impl OutputSink for ProgressFileSink {
    fn record(&mut self, record: OutputRecord) -> Result<(), combinator_app::AppError> {
        self.sink.record(record)
    }

    fn progress(&mut self, event: ProgressEvent) -> Result<(), combinator_app::AppError> {
        self.messages
            .send(WorkerMessage::Progress(event))
            .map_err(|error| combinator_app::AppError {
                code: "CANCELLED",
                message: error.to_string(),
            })
    }
}

fn main() -> io::Result<()> {
    ratatui::run(run)
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = TuiApp::default();
    loop {
        app.poll_worker();
        terminal.draw(|frame| app.draw(frame))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && app.handle_key(key.code) {
                    return Ok(());
                }
            }
        }
    }
}

struct TuiApp {
    join_mode: bool,
    request: ProductRequest,
    join_request: JoinRequest,
    sources: Vec<String>,
    file_sources: Vec<Option<String>>,
    file_formats: Vec<InputFormat>,
    list_delimiter: String,
    selected: usize,
    editing: bool,
    edit_target: EditTarget,
    output_path: String,
    overwrite: bool,
    max_combinations: String,
    max_output_bytes: String,
    timeout_ms: String,
    shard_index: String,
    shard_count: String,
    template: String,
    transforms: String,
    template_file: String,
    filters: String,
    names: String,
    offset: String,
    limit: String,
    choose: String,
    length: String,
    resource_limits: String,
    lean_jsonl: bool,
    join_fanout: String,
    running: bool,
    cancellation: Option<CancellationToken>,
    worker: Option<Receiver<WorkerMessage>>,
    progress: Option<ProgressEvent>,
    list_state: ListState,
    plan: Option<ExecutionPlan>,
    join_plan: Option<JoinPlan>,
    records: Vec<PreviewRecord>,
    status: String,
    error: Option<String>,
}

impl Default for TuiApp {
    fn default() -> Self {
        let mut app = Self {
            join_mode: false,
            request: ProductRequest::default(),
            join_request: JoinRequest::default(),
            sources: vec![String::new()],
            file_sources: vec![None],
            file_formats: vec![InputFormat::Lines],
            list_delimiter: ",".into(),
            selected: 0,
            editing: true,
            edit_target: EditTarget::List,
            output_path: "output.txt".to_string(),
            overwrite: false,
            max_combinations: "10000000".to_string(),
            max_output_bytes: "1073741824".to_string(),
            timeout_ms: String::new(),
            shard_index: String::new(),
            shard_count: String::new(),
            template: String::new(),
            transforms: String::new(),
            template_file: String::new(),
            filters: String::new(),
            names: String::new(),
            offset: "0".into(),
            limit: String::new(),
            choose: "2".into(),
            length: "2".into(),
            resource_limits: "67108864;1048576;1000000;5000000;128".into(),
            lean_jsonl: false,
            join_fanout: "10000".into(),
            running: false,
            cancellation: None,
            worker: None,
            progress: None,
            list_state: ListState::default().with_selected(Some(0)),
            plan: None,
            join_plan: None,
            records: Vec::new(),
            status: "Type values into the selected list".to_string(),
            error: None,
        };
        app.refresh_plan();
        app
    }
}

impl TuiApp {
    fn draw(&mut self, frame: &mut Frame<'_>) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(frame.area());

        let list_items = self.sources.iter().enumerate().map(|(index, source)| {
            let value = self.file_sources[index]
                .as_deref()
                .map(|path| format!("file: {path}"))
                .unwrap_or_else(|| format!("inline: {source}"));
            ListItem::new(Line::from(format!("List {}: {}", index + 1, value)))
        });
        let inputs = List::new(list_items)
            .block(Block::default().title(" Inputs ").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(inputs, layout[0], &mut self.list_state);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Length(7),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(layout[1]);
        frame.render_widget(self.plan_widget(), right[0]);
        frame.render_widget(self.controls_widget(), right[1]);
        frame.render_widget(self.preview_widget(), right[2]);
        frame.render_widget(self.status_widget(), right[3]);
    }

    fn plan_widget(&self) -> Paragraph<'static> {
        if self.join_mode {
            let body = match &self.join_plan {
                Some(plan) => format!(
                    "Left records: {}\nRight records: {}\nJoin records: {}\nSelected: {}",
                    plan.left_records, plan.right_records, plan.total_records, plan.records_to_emit
                ),
                None => "No valid join plan".to_string(),
            };
            return Paragraph::new(body)
                .block(Block::default().title(" Join plan ").borders(Borders::ALL));
        }
        let body = match &self.plan {
            Some(plan) => format!(
                "Lists: {}\nItems: {:?}\nCombinations: {:?}\nSelected: {}\nWarnings: {}",
                plan.list_lengths.len(),
                plan.list_lengths,
                plan.total_combinations,
                plan.records_to_emit,
                plan.warnings.len()
            ),
            None => "No valid plan".to_string(),
        };
        Paragraph::new(body).block(Block::default().title(" Plan ").borders(Borders::ALL))
    }

    fn controls_widget(&self) -> Paragraph<'static> {
        let mode = if self.editing { "EDIT" } else { "COMMAND" };
        if self.join_mode {
            return Paragraph::new(format!(
                "Mode: {mode} JOIN\nLeft: {}  Right: {}\nKeys: {} / {}\nJ left  R right  K left-key  O right-key  Y format  U type  m max-records  b max-bytes  F fanout  v product  p preview  g generate  c cancel  Esc command  q quit",
                self.join_request.left_path,
                self.join_request.right_path,
                self.join_request.left_key,
                self.join_request.right_key,
            ))
            .block(Block::default().title(" Join controls ").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        }
        Paragraph::new(format!(
                "Mode: {mode}\nOperation: {}  Format: {:?}\nOutput: {} ({})\nEnter list  f file  o output  m combos  b bytes  t template  j template-file  x transforms  X filters  n names  e delimiter  T timeout  I shard-index  C shard-count  N resource-limits  L lean-jsonl  V join  v operation  z format  y input format  r reverse  i offset  l limit  Esc command  a add  d remove  Tab/↑↓ select  p preview  g generate  c cancel  w overwrite  q quit",
            operation_label(self.request.operation),
            self.request.format,
            self.output_path,
            if self.overwrite { "overwrite" } else { "no overwrite" }
        ))
            .block(Block::default().title(" Controls ").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
    }

    fn preview_widget(&self) -> Paragraph<'static> {
        let body = if self.records.is_empty() {
            "No preview records".to_string()
        } else {
            self.records
                .iter()
                .map(|record| format!("{}  {}", record.ordinal, record.value.trim_end()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Paragraph::new(body)
            .block(Block::default().title(" Preview ").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
    }

    fn status_widget(&self) -> Paragraph<'static> {
        let body = match &self.error {
            Some(error) => format!("Error: {error}"),
            None => self.status.clone(),
        };
        Paragraph::new(body).block(Block::default().title(" Status ").borders(Borders::ALL))
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.editing {
            match code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.editing = false;
                }
                KeyCode::Backspace => match self.edit_target {
                    EditTarget::List => self.edit_selected(|source| {
                        source.pop();
                    }),
                    EditTarget::File => {
                        if let Some(Some(path)) = self.file_sources.get_mut(self.selected) {
                            path.pop();
                        }
                        self.refresh_plan();
                    }
                    EditTarget::Output => {
                        self.output_path.pop();
                    }
                    EditTarget::MaxCombinations => {
                        self.max_combinations.pop();
                        if self.join_mode {
                            self.join_request.max_join_records =
                                self.max_combinations.parse().unwrap_or(0);
                            self.refresh_plan();
                        } else {
                            self.update_limit(true);
                        }
                    }
                    EditTarget::MaxOutputBytes => {
                        self.max_output_bytes.pop();
                        if self.join_mode {
                            self.join_request.max_output_bytes =
                                self.max_output_bytes.parse().unwrap_or(0);
                            self.refresh_plan();
                        } else {
                            self.update_limit(false);
                        }
                    }
                    EditTarget::Template => {
                        self.template.pop();
                        self.request.template =
                            (!self.template.is_empty()).then_some(self.template.clone());
                        self.refresh_plan();
                    }
                    EditTarget::Transforms => {
                        self.transforms.pop();
                        self.request.transforms = self
                            .transforms
                            .split(';')
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string)
                            .collect();
                        self.refresh_plan();
                    }
                    EditTarget::Names => {
                        self.names.pop();
                        self.update_names();
                    }
                    EditTarget::Offset => {
                        self.offset.pop();
                        self.update_paging();
                    }
                    EditTarget::Limit => {
                        self.limit.pop();
                        self.update_paging();
                    }
                    EditTarget::Choose => {
                        self.choose.pop();
                        self.update_selection();
                    }
                    EditTarget::Length => {
                        self.length.pop();
                        self.update_selection();
                    }
                    EditTarget::Delimiter => {
                        self.list_delimiter.pop();
                        self.refresh_plan();
                    }
                    EditTarget::TemplateFile => {
                        self.template_file.pop();
                        self.request.template_file =
                            (!self.template_file.is_empty()).then_some(self.template_file.clone());
                        self.refresh_plan();
                    }
                    EditTarget::Filters => {
                        self.filters.pop();
                        self.update_filters();
                    }
                    EditTarget::Timeout => {
                        self.timeout_ms.pop();
                        self.request.timeout_ms = self.timeout_ms.parse().ok();
                        self.join_request.timeout_ms = self.request.timeout_ms;
                    }
                    EditTarget::ShardIndex => {
                        self.shard_index.pop();
                        self.request.shard_index = self.shard_index.parse().ok();
                        self.refresh_plan();
                    }
                    EditTarget::ShardCount => {
                        self.shard_count.pop();
                        self.request.shard_count = self.shard_count.parse().ok();
                        self.refresh_plan();
                    }
                    EditTarget::ResourceLimits => {
                        self.resource_limits.pop();
                        self.update_resource_limits();
                    }
                    EditTarget::JoinLeft => {
                        self.join_request.left_path.pop();
                        self.refresh_plan();
                    }
                    EditTarget::JoinRight => {
                        self.join_request.right_path.pop();
                        self.refresh_plan();
                    }
                    EditTarget::JoinLeftKey => {
                        self.join_request.left_key.pop();
                        self.refresh_plan();
                    }
                    EditTarget::JoinRightKey => {
                        self.join_request.right_key.pop();
                        self.refresh_plan();
                    }
                    EditTarget::JoinFanout => {
                        self.join_fanout.pop();
                        self.join_request.max_join_key_fanout =
                            self.join_fanout.parse().unwrap_or(0);
                        self.refresh_plan();
                    }
                },
                KeyCode::Char(value) => match self.edit_target {
                    EditTarget::List => self.edit_selected(|source| source.push(value)),
                    EditTarget::File => {
                        if let Some(Some(path)) = self.file_sources.get_mut(self.selected) {
                            path.push(value);
                        }
                        self.refresh_plan();
                    }
                    EditTarget::Output => self.output_path.push(value),
                    EditTarget::MaxCombinations => {
                        self.max_combinations.push(value);
                        if self.join_mode {
                            self.join_request.max_join_records =
                                self.max_combinations.parse().unwrap_or(0);
                            self.refresh_plan();
                        } else {
                            self.update_limit(true);
                        }
                    }
                    EditTarget::MaxOutputBytes => {
                        self.max_output_bytes.push(value);
                        if self.join_mode {
                            self.join_request.max_output_bytes =
                                self.max_output_bytes.parse().unwrap_or(0);
                            self.refresh_plan();
                        } else {
                            self.update_limit(false);
                        }
                    }
                    EditTarget::Template => {
                        self.template.push(value);
                        self.request.template = Some(self.template.clone());
                        self.refresh_plan();
                    }
                    EditTarget::Transforms => {
                        self.transforms.push(value);
                        self.request.transforms = self
                            .transforms
                            .split(';')
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string)
                            .collect();
                        self.refresh_plan();
                    }
                    EditTarget::Names => {
                        self.names.push(value);
                        self.update_names();
                    }
                    EditTarget::Offset => {
                        self.offset.push(value);
                        self.update_paging();
                    }
                    EditTarget::Limit => {
                        self.limit.push(value);
                        self.update_paging();
                    }
                    EditTarget::Choose => {
                        self.choose.push(value);
                        self.update_selection();
                    }
                    EditTarget::Length => {
                        self.length.push(value);
                        self.update_selection();
                    }
                    EditTarget::Delimiter => {
                        self.list_delimiter.push(value);
                        self.refresh_plan();
                    }
                    EditTarget::TemplateFile => {
                        self.template_file.push(value);
                        self.request.template_file = Some(self.template_file.clone());
                        self.request.template = None;
                        self.template.clear();
                        self.refresh_plan();
                    }
                    EditTarget::Filters => {
                        self.filters.push(value);
                        self.update_filters();
                    }
                    EditTarget::Timeout => {
                        self.timeout_ms.push(value);
                        self.request.timeout_ms = self.timeout_ms.parse().ok();
                        self.join_request.timeout_ms = self.request.timeout_ms;
                    }
                    EditTarget::ShardIndex => {
                        self.shard_index.push(value);
                        self.request.shard_index = self.shard_index.parse().ok();
                        self.refresh_plan();
                    }
                    EditTarget::ShardCount => {
                        self.shard_count.push(value);
                        self.request.shard_count = self.shard_count.parse().ok();
                        self.refresh_plan();
                    }
                    EditTarget::ResourceLimits => {
                        self.resource_limits.push(value);
                        self.update_resource_limits();
                    }
                    EditTarget::JoinLeft => {
                        self.join_request.left_path.push(value);
                        self.refresh_plan();
                    }
                    EditTarget::JoinRight => {
                        self.join_request.right_path.push(value);
                        self.refresh_plan();
                    }
                    EditTarget::JoinLeftKey => {
                        self.join_request.left_key.push(value);
                        self.refresh_plan();
                    }
                    EditTarget::JoinRightKey => {
                        self.join_request.right_key.push(value);
                        self.refresh_plan();
                    }
                    EditTarget::JoinFanout => {
                        self.join_fanout.push(value);
                        self.join_request.max_join_key_fanout =
                            self.join_fanout.parse().unwrap_or(0);
                        self.refresh_plan();
                    }
                },
                _ => {}
            }
            return false;
        }

        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Enter => {
                self.editing = true;
                self.edit_target = EditTarget::List;
            }
            KeyCode::Char('o') => {
                self.editing = true;
                self.edit_target = EditTarget::Output;
            }
            KeyCode::Char('f') => {
                if self.file_sources[self.selected].is_none() {
                    self.file_sources[self.selected] = Some(String::new());
                }
                self.editing = true;
                self.edit_target = EditTarget::File;
                self.refresh_plan();
            }
            KeyCode::Char('m') => {
                self.editing = true;
                self.edit_target = EditTarget::MaxCombinations;
            }
            KeyCode::Char('b') => {
                self.editing = true;
                self.edit_target = EditTarget::MaxOutputBytes;
            }
            KeyCode::Char('t') => {
                self.editing = true;
                self.edit_target = EditTarget::Template;
            }
            KeyCode::Char('x') => {
                self.editing = true;
                self.edit_target = EditTarget::Transforms;
            }
            KeyCode::Char('n') => {
                self.editing = true;
                self.edit_target = EditTarget::Names;
            }
            KeyCode::Char('e') => {
                self.editing = true;
                self.edit_target = EditTarget::Delimiter;
            }
            KeyCode::Char('j') => {
                self.editing = true;
                self.edit_target = EditTarget::TemplateFile;
            }
            KeyCode::Char('X') => {
                self.editing = true;
                self.edit_target = EditTarget::Filters;
            }
            KeyCode::Char('T') => {
                self.editing = true;
                self.edit_target = EditTarget::Timeout;
            }
            KeyCode::Char('I') => {
                self.editing = true;
                self.edit_target = EditTarget::ShardIndex;
            }
            KeyCode::Char('C') => {
                self.editing = true;
                self.edit_target = EditTarget::ShardCount;
            }
            KeyCode::Char('N') => {
                self.editing = true;
                self.edit_target = EditTarget::ResourceLimits;
            }
            KeyCode::Char('L') => {
                self.lean_jsonl = !self.lean_jsonl;
                self.request.lean_jsonl = self.lean_jsonl;
                self.refresh_plan();
            }
            KeyCode::Char('J') if self.join_mode => {
                self.editing = true;
                self.edit_target = EditTarget::JoinLeft;
            }
            KeyCode::Char('R') if self.join_mode => {
                self.editing = true;
                self.edit_target = EditTarget::JoinRight;
            }
            KeyCode::Char('K') if self.join_mode => {
                self.editing = true;
                self.edit_target = EditTarget::JoinLeftKey;
            }
            KeyCode::Char('O') if self.join_mode => {
                self.editing = true;
                self.edit_target = EditTarget::JoinRightKey;
            }
            KeyCode::Char('F') if self.join_mode => {
                self.editing = true;
                self.edit_target = EditTarget::JoinFanout;
            }
            KeyCode::Char('Y') if self.join_mode => {
                self.join_request.format = next_join_format(self.join_request.format);
                self.refresh_plan();
            }
            KeyCode::Char('U') if self.join_mode => {
                self.join_request.kind = next_join_kind(self.join_request.kind);
                self.refresh_plan();
            }
            KeyCode::Char('v') if self.join_mode => {
                self.join_mode = false;
                self.refresh_plan();
            }
            KeyCode::Char('V') => {
                self.join_mode = true;
                self.refresh_plan();
            }
            KeyCode::Char('v') => {
                self.request.operation = next_operation(self.request.operation);
                self.refresh_plan();
            }
            KeyCode::Char('z') => {
                self.request.format = next_format(self.request.format);
                self.refresh_plan();
            }
            KeyCode::Char('y') => {
                if self.file_sources[self.selected].is_some() {
                    self.file_formats[self.selected] =
                        next_input_format(self.file_formats[self.selected]);
                    self.refresh_plan();
                }
            }
            KeyCode::Char('r') => {
                self.request.options.reverse = !self.request.options.reverse;
                self.refresh_plan();
            }
            KeyCode::Char('i') => {
                self.editing = true;
                self.edit_target = EditTarget::Offset;
            }
            KeyCode::Char('l') => {
                self.editing = true;
                self.edit_target = EditTarget::Limit;
            }
            KeyCode::Char('k') => {
                if matches!(self.request.operation, AppOperation::Combinations { .. }) {
                    self.editing = true;
                    self.edit_target = EditTarget::Choose;
                }
            }
            KeyCode::Char('h') => {
                if matches!(self.request.operation, AppOperation::Variations { .. }) {
                    self.editing = true;
                    self.edit_target = EditTarget::Length;
                }
            }
            KeyCode::Char('u') => {
                if let AppOperation::Zip { on_unequal } = self.request.operation {
                    self.request.operation = AppOperation::Zip {
                        on_unequal: next_zip_policy(on_unequal),
                    };
                    self.refresh_plan();
                }
            }
            KeyCode::Char('a') => self.add_list(),
            KeyCode::Char('d') => self.remove_list(),
            KeyCode::Tab | KeyCode::Down => self.select_next(),
            KeyCode::BackTab | KeyCode::Up => self.select_previous(),
            KeyCode::Char('p') => {
                if self.join_mode {
                    self.run_join_preview(PREVIEW_LIMIT, "Join preview ready");
                } else {
                    self.run_preview(PREVIEW_LIMIT, "Preview ready");
                }
            }
            KeyCode::Char('g') => self.start_generation(),
            KeyCode::Char('c') => {
                if let Some(token) = &self.cancellation {
                    token.cancel();
                    self.status = "Cancellation requested...".to_string();
                }
            }
            KeyCode::Char('w') => self.overwrite = !self.overwrite,
            _ => {}
        }
        false
    }

    fn add_list(&mut self) {
        self.sources.push(String::new());
        self.file_sources.push(None);
        self.file_formats.push(InputFormat::Lines);
        self.selected = self.sources.len() - 1;
        self.list_state.select(Some(self.selected));
        self.refresh_plan();
    }

    fn remove_list(&mut self) {
        if self.sources.len() <= 1 {
            return;
        }
        self.sources.remove(self.selected);
        self.file_sources.remove(self.selected);
        self.file_formats.remove(self.selected);
        self.selected = self.selected.min(self.sources.len() - 1);
        self.list_state.select(Some(self.selected));
        self.refresh_plan();
    }

    fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.sources.len();
        self.list_state.select(Some(self.selected));
    }

    fn select_previous(&mut self) {
        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or(self.sources.len() - 1);
        self.list_state.select(Some(self.selected));
    }

    fn edit_selected<F>(&mut self, edit: F)
    where
        F: FnOnce(&mut String),
    {
        edit(&mut self.sources[self.selected]);
        self.refresh_plan();
    }

    fn refresh_plan(&mut self) {
        if self.join_mode {
            match join_plan(&self.join_request) {
                Ok(plan) => {
                    self.join_plan = Some(plan);
                    self.error = None;
                    self.status = "Join ready".into();
                }
                Err(error) => {
                    self.join_plan = None;
                    self.error = Some(format!("{}: {}", error.code, error.message));
                }
            }
            return;
        }
        let limits = InputLimits {
            max_input_bytes: self.request.max_input_bytes,
            max_item_bytes: self.request.max_item_bytes,
            max_items_per_list: self.request.max_items_per_list,
        };
        let mut lists = Vec::with_capacity(self.sources.len());
        for (index, source) in self.sources.iter().enumerate() {
            let input = match &self.file_sources[index] {
                Some(path) => InputSource::File {
                    path: path.clone(),
                    format: self.file_formats[index],
                },
                None => InputSource::Inline {
                    value: source.clone(),
                    delimiter: self.list_delimiter.clone(),
                },
            };
            match read_input_source(&input, limits) {
                Ok(list) => lists.push(list),
                Err(error) => {
                    self.plan = None;
                    self.error = Some(format!("{}: {}", error.code, error.message));
                    return;
                }
            }
        }
        self.request.lists = lists;
        match plan(&self.request) {
            Ok(plan) => {
                self.plan = Some(plan);
                self.error = None;
                self.status = "Ready".to_string();
            }
            Err(error) => {
                self.plan = None;
                self.error = Some(format!("{}: {}", error.code, error.message));
            }
        }
    }

    fn run_preview(&mut self, limit: u128, status: &str) {
        self.refresh_plan();
        match preview(&self.request, limit) {
            Ok(records) => {
                self.records = records;
                self.status = status.to_string();
                self.error = None;
            }
            Err(error) => {
                self.records.clear();
                self.error = Some(format!("{}: {}", error.code, error.message));
            }
        }
    }

    fn run_join_preview(&mut self, limit: u128, status: &str) {
        self.refresh_plan();
        match join_preview(&self.join_request, limit) {
            Ok(records) => {
                self.records = records;
                self.status = status.into();
                self.error = None;
            }
            Err(error) => {
                self.records.clear();
                self.error = Some(format!("{}: {}", error.code, error.message));
            }
        }
    }

    fn start_generation(&mut self) {
        if self.running {
            return;
        }
        self.refresh_plan();
        if (!self.join_mode && self.plan.is_none()) || (self.join_mode && self.join_plan.is_none())
        {
            return;
        }
        let sink = match FileSink::open(&self.output_path, self.overwrite) {
            Ok(sink) => sink,
            Err(error) => {
                self.error = Some(format!("{}: {}", error.code, error.message));
                return;
            }
        };
        let (messages, worker) = mpsc::channel();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        self.running = true;
        self.cancellation = Some(token);
        self.worker = Some(worker);
        self.progress = Some(ProgressEvent {
            records: 0,
            bytes: 0,
        });
        self.status = "Generating...".to_string();
        self.error = None;
        if self.join_mode {
            let request = self.join_request.clone();
            thread::spawn(move || {
                let mut sink = ProgressFileSink { sink, messages };
                let cancelled = || worker_token.is_cancelled();
                let result = match join_stream(&request, &mut sink, Some(&cancelled)) {
                    Ok(progress) => sink.sink.commit().map(|_| progress),
                    Err(error) => Err(error),
                };
                let _ = sink.messages.send(WorkerMessage::Finished(result));
            });
            return;
        }
        let request = self.request.clone();
        thread::spawn(move || {
            let mut sink = ProgressFileSink { sink, messages };
            let cancelled = || worker_token.is_cancelled();
            let result = match stream(&request, &mut sink, Some(&cancelled)) {
                Ok(progress) => sink.sink.commit().map(|_| progress),
                Err(error) => Err(error),
            };
            let _ = sink.messages.send(WorkerMessage::Finished(result));
        });
    }

    fn poll_worker(&mut self) {
        let mut finished = None;
        if let Some(worker) = &self.worker {
            for message in worker.try_iter() {
                match message {
                    WorkerMessage::Progress(progress) => {
                        self.progress = Some(progress);
                        self.status = format!(
                            "Generating... {} records ({} bytes)",
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
                        "Wrote {} records ({} bytes) to {}",
                        progress.records, progress.bytes, self.output_path
                    );
                    self.error = None;
                }
                Err(error) => {
                    self.error = Some(format!("{}: {}", error.code, error.message));
                }
            }
        }
    }

    fn update_limit(&mut self, combinations: bool) {
        let text = if combinations {
            &self.max_combinations
        } else {
            &self.max_output_bytes
        };
        match text.parse::<u128>() {
            Ok(value) if value > 0 => {
                if combinations {
                    self.request.max_combinations = value;
                } else {
                    self.request.max_output_bytes = value;
                }
                self.refresh_plan();
            }
            _ => self.error = Some("LIMIT_INVALID: enter a positive integer".to_string()),
        }
    }

    fn update_names(&mut self) {
        self.request.names = self
            .names
            .split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        self.refresh_plan();
    }

    fn update_resource_limits(&mut self) {
        let values = self
            .resource_limits
            .split(';')
            .map(str::trim)
            .collect::<Vec<_>>();
        if values.len() != 5 {
            return;
        }
        if let (Ok(input), Ok(item), Ok(per_list), Ok(total), Ok(lists)) = (
            values[0].parse(),
            values[1].parse(),
            values[2].parse(),
            values[3].parse(),
            values[4].parse(),
        ) {
            self.request.max_input_bytes = input;
            self.request.max_item_bytes = item;
            self.request.max_items_per_list = per_list;
            self.request.max_total_items = total;
            self.request.max_lists = lists;
            self.refresh_plan();
        }
    }

    fn update_filters(&mut self) {
        self.request.filters = self
            .filters
            .split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        self.refresh_plan();
    }

    fn update_paging(&mut self) {
        if self.join_mode {
            self.join_request.offset = self.offset.parse().unwrap_or(0);
            self.join_request.limit = if self.limit.is_empty() {
                None
            } else {
                self.limit.parse().ok()
            };
            self.refresh_plan();
            return;
        }
        if let Ok(value) = self.offset.parse() {
            self.request.options.offset = value;
        }
        self.request.options.limit = if self.limit.is_empty() {
            None
        } else {
            self.limit.parse().ok()
        };
        self.refresh_plan();
    }

    fn update_selection(&mut self) {
        match self.request.operation {
            AppOperation::Combinations { .. } => {
                if let Ok(choose) = self.choose.parse() {
                    self.request.operation = AppOperation::Combinations { choose };
                }
            }
            AppOperation::Variations { .. } => {
                if let Ok(length) = self.length.parse() {
                    self.request.operation = AppOperation::Variations { length };
                }
            }
            _ => {}
        }
        self.refresh_plan();
    }
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

fn next_operation(operation: AppOperation) -> AppOperation {
    match operation {
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

fn next_format(format: Format) -> Format {
    match format {
        Format::Text => Format::Jsonl,
        Format::Jsonl => Format::Csv,
        Format::Csv => Format::Tsv,
        Format::Tsv => Format::Nul,
        Format::Nul => Format::Text,
    }
}

fn next_zip_policy(policy: UnequalPolicy) -> UnequalPolicy {
    match policy {
        UnequalPolicy::Error => UnequalPolicy::Truncate,
        UnequalPolicy::Truncate => UnequalPolicy::Cycle,
        UnequalPolicy::Cycle => UnequalPolicy::Error,
    }
}

fn next_input_format(format: InputFormat) -> InputFormat {
    match format {
        InputFormat::Lines => InputFormat::Csv,
        InputFormat::Csv => InputFormat::Tsv,
        InputFormat::Tsv => InputFormat::Nul,
        InputFormat::Nul => InputFormat::Lines,
    }
}

fn next_join_format(format: JoinFormat) -> JoinFormat {
    match format {
        JoinFormat::Csv => JoinFormat::Tsv,
        JoinFormat::Tsv => JoinFormat::Jsonl,
        JoinFormat::Jsonl => JoinFormat::Csv,
    }
}

fn next_join_kind(kind: JoinKind) -> JoinKind {
    match kind {
        JoinKind::Inner => JoinKind::Left,
        JoinKind::Left => JoinKind::Full,
        JoinKind::Full => JoinKind::Anti,
        JoinKind::Anti => JoinKind::Inner,
    }
}
