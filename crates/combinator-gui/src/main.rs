use combinator_app::{
    join_plan, join_preview, join_stream, plan, preview, read_input_source, stream, AppOperation,
    CancellationToken, ExecutionPlan, FileSink, Format, InputFormat, InputLimits, InputSource,
    JoinFormat, JoinKind, JoinPlan, JoinRequest, OutputRecord, OutputSink, PreviewRecord,
    ProductRequest, ProgressEvent, UnequalPolicy,
};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Subscription, Task};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PREVIEW_LIMIT: u128 = 20;

struct ProgressFileSink {
    sink: FileSink,
    progress: Arc<Mutex<ProgressEvent>>,
}

impl OutputSink for ProgressFileSink {
    fn record(&mut self, record: OutputRecord) -> Result<(), combinator_app::AppError> {
        self.sink.record(record)
    }

    fn progress(&mut self, event: ProgressEvent) -> Result<(), combinator_app::AppError> {
        if let Ok(mut progress) = self.progress.lock() {
            *progress = event;
        }
        Ok(())
    }
}

fn main() -> iced::Result {
    iced::application(CombinatorGui::default, update, view)
        .title("Combinator")
        .centered()
        .subscription(subscription)
        .run()
}

struct CombinatorGui {
    join_mode: bool,
    request: ProductRequest,
    join_request: JoinRequest,
    sources: Vec<String>,
    file_sources: Vec<Option<String>>,
    file_formats: Vec<InputFormat>,
    list_delimiter: String,
    template: String,
    transforms: String,
    transforms_editor: text_editor::Content,
    template_file: String,
    template_file_mode: bool,
    filters: String,
    filters_editor: text_editor::Content,
    names: String,
    names_editor: text_editor::Content,
    offset: String,
    limit: String,
    choose: String,
    length: String,
    plan: Option<ExecutionPlan>,
    join_plan: Option<JoinPlan>,
    settings_mode: bool,
    records: Vec<PreviewRecord>,
    output_path: String,
    overwrite: bool,
    max_combinations: String,
    max_output_bytes: String,
    max_input_bytes: String,
    max_item_bytes: String,
    max_items_per_list: String,
    max_total_items: String,
    max_lists: String,
    lean_jsonl: bool,
    timeout_ms: String,
    shard_index: String,
    shard_count: String,
    join_max_records: String,
    join_fanout: String,
    running: bool,
    cancellation: Option<CancellationToken>,
    progress: Option<Arc<Mutex<ProgressEvent>>>,
    status: String,
    error: Option<String>,
}

impl Default for CombinatorGui {
    fn default() -> Self {
        let mut state = Self {
            join_mode: false,
            request: ProductRequest::default(),
            join_request: JoinRequest::default(),
            sources: vec![String::new()],
            file_sources: vec![None],
            file_formats: vec![InputFormat::Lines],
            list_delimiter: ",".into(),
            template: String::new(),
            transforms: String::new(),
            transforms_editor: text_editor::Content::new(),
            template_file: String::new(),
            template_file_mode: false,
            filters: String::new(),
            filters_editor: text_editor::Content::new(),
            names: String::new(),
            names_editor: text_editor::Content::new(),
            offset: "0".into(),
            limit: String::new(),
            choose: "2".into(),
            length: "2".into(),
            plan: None,
            join_plan: None,
            settings_mode: false,
            records: Vec::new(),
            output_path: "output.txt".to_string(),
            overwrite: false,
            max_combinations: "10000000".to_string(),
            max_output_bytes: "1073741824".to_string(),
            max_input_bytes: "67108864".into(),
            max_item_bytes: "1048576".into(),
            max_items_per_list: "1000000".into(),
            max_total_items: "5000000".into(),
            max_lists: "128".into(),
            lean_jsonl: false,
            timeout_ms: String::new(),
            shard_index: String::new(),
            shard_count: String::new(),
            join_max_records: "100000".into(),
            join_fanout: "10000".into(),
            running: false,
            cancellation: None,
            progress: None,
            status: "Add values to begin".to_string(),
            error: None,
        };
        state.refresh_plan();
        state
    }
}

#[derive(Debug, Clone)]
enum Message {
    AddList,
    RemoveList(usize),
    SourceChanged(usize, String),
    FileModeChanged(usize, bool),
    FilePathChanged(usize, String),
    BrowseFile(usize),
    FilePicked(usize, Option<String>),
    BrowseTemplate,
    TemplatePicked(Option<String>),
    BrowseOutput,
    OutputPicked(Option<String>),
    BrowseJoinLeft,
    JoinLeftPicked(Option<String>),
    BrowseJoinRight,
    JoinRightPicked(Option<String>),
    FileFormatChanged(usize, InputFormat),
    ListDelimiterChanged(String),
    SeparatorChanged(String),
    TemplateChanged(String),
    TransformsChanged(text_editor::Action),
    TemplateFileChanged(String),
    TemplateFileModeChanged(bool),
    FiltersChanged(text_editor::Action),
    NamesChanged(text_editor::Action),
    SelectCombine,
    SelectJoin,
    SelectSettings,
    OperationChanged(AppOperation),
    JoinLeftChanged(String),
    JoinRightChanged(String),
    JoinLeftKeyChanged(String),
    JoinRightKeyChanged(String),
    JoinFormatChanged(JoinFormat),
    JoinKindChanged(JoinKind),
    JoinOffsetChanged(String),
    JoinLimitChanged(String),
    FormatChanged(Format),
    ZipPolicyChanged(UnequalPolicy),
    ReverseChanged(bool),
    ReverseFieldsChanged(bool),
    OffsetChanged(String),
    LimitChanged(String),
    ChooseChanged(String),
    LengthChanged(String),
    OutputPathChanged(String),
    OverwriteChanged(bool),
    MaxCombinationsChanged(String),
    MaxOutputBytesChanged(String),
    TimeoutChanged(String),
    ShardIndexChanged(String),
    ShardCountChanged(String),
    LeanJsonlChanged(bool),
    MaxInputBytesChanged(String),
    MaxItemBytesChanged(String),
    MaxItemsPerListChanged(String),
    MaxTotalItemsChanged(String),
    MaxListsChanged(String),
    JoinMaxRecordsChanged(String),
    JoinFanoutChanged(String),
    Preview,
    Generate,
    Cancel,
    Tick,
    GenerationFinished(Result<ProgressEvent, combinator_app::AppError>),
}

fn update(state: &mut CombinatorGui, message: Message) -> Task<Message> {
    match message {
        Message::AddList => {
            state.sources.push(String::new());
            state.file_sources.push(None);
            state.file_formats.push(InputFormat::Lines);
            state.refresh_plan();
            Task::none()
        }
        Message::RemoveList(index) => {
            if state.sources.len() > 1 {
                state.sources.remove(index);
                state.file_sources.remove(index);
                state.file_formats.remove(index);
                state.refresh_plan();
            }
            Task::none()
        }
        Message::SourceChanged(index, value) => {
            if let Some(source) = state.sources.get_mut(index) {
                *source = value;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::FileModeChanged(index, enabled) => {
            if let Some(source) = state.file_sources.get_mut(index) {
                *source = enabled.then(String::new);
                state.refresh_plan();
            }
            Task::none()
        }
        Message::FilePathChanged(index, value) => {
            if let Some(Some(path)) = state.file_sources.get_mut(index) {
                *path = value;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::BrowseFile(index) => Task::perform(
            async move {
                rfd::FileDialog::new()
                    .set_title("Select input file")
                    .pick_file()
                    .map(|path| path.to_string_lossy().into_owned())
            },
            move |path| Message::FilePicked(index, path),
        ),
        Message::FilePicked(index, path) => {
            if let Some(path) = path {
                if let Some(Some(current)) = state.file_sources.get_mut(index) {
                    *current = path;
                    state.refresh_plan();
                }
            }
            Task::none()
        }
        Message::BrowseTemplate => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select template file")
                    .pick_file()
                    .map(|path| path.to_string_lossy().into_owned())
            },
            Message::TemplatePicked,
        ),
        Message::TemplatePicked(path) => {
            if let Some(path) = path {
                state.template_file = path.clone();
                state.request.template_file = Some(path);
                state.request.template = None;
                state.template.clear();
                state.template_file_mode = true;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::BrowseOutput => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select output file")
                    .save_file()
                    .map(|path| path.to_string_lossy().into_owned())
            },
            Message::OutputPicked,
        ),
        Message::OutputPicked(path) => {
            if let Some(path) = path {
                state.output_path = path;
            }
            Task::none()
        }
        Message::BrowseJoinLeft => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select left join file")
                    .pick_file()
                    .map(|path| path.to_string_lossy().into_owned())
            },
            Message::JoinLeftPicked,
        ),
        Message::JoinLeftPicked(path) => {
            if let Some(path) = path {
                state.join_request.left_path = path;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::BrowseJoinRight => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select right join file")
                    .pick_file()
                    .map(|path| path.to_string_lossy().into_owned())
            },
            Message::JoinRightPicked,
        ),
        Message::JoinRightPicked(path) => {
            if let Some(path) = path {
                state.join_request.right_path = path;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::FileFormatChanged(index, selected_format) => {
            if let Some(format) = state.file_formats.get_mut(index) {
                *format = selected_format;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::ListDelimiterChanged(value) => {
            state.list_delimiter = value;
            state.refresh_plan();
            Task::none()
        }
        Message::SeparatorChanged(value) => {
            state.request.field_separator = value;
            state.refresh_plan();
            Task::none()
        }
        Message::TemplateChanged(value) => {
            state.template = value.clone();
            state.request.template = (!value.is_empty()).then_some(value);
            if state.request.template.is_some() {
                state.template_file.clear();
                state.request.template_file = None;
            }
            state.refresh_plan();
            Task::none()
        }
        Message::TransformsChanged(action) => {
            state.transforms_editor.perform(action);
            state.transforms = state.transforms_editor.text();
            state.request.transforms = state
                .transforms
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
            state.refresh_plan();
            Task::none()
        }
        Message::TemplateFileChanged(value) => {
            state.template_file = value.clone();
            state.request.template_file = (!value.is_empty()).then_some(value);
            if state.request.template_file.is_some() {
                state.template.clear();
                state.request.template = None;
            }
            state.refresh_plan();
            Task::none()
        }
        Message::TemplateFileModeChanged(enabled) => {
            state.template_file_mode = enabled;
            if enabled {
                state.request.template = None;
                state.request.template_file =
                    (!state.template_file.is_empty()).then(|| state.template_file.clone());
            } else {
                state.request.template_file = None;
                state.request.template =
                    (!state.template.is_empty()).then(|| state.template.clone());
            }
            state.refresh_plan();
            Task::none()
        }
        Message::FiltersChanged(action) => {
            state.filters_editor.perform(action);
            state.filters = state.filters_editor.text();
            state.request.filters = state
                .filters
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
            state.refresh_plan();
            Task::none()
        }
        Message::NamesChanged(action) => {
            state.names_editor.perform(action);
            state.names = state.names_editor.text();
            state.request.names = state
                .names
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
            state.refresh_plan();
            Task::none()
        }
        Message::SelectCombine => {
            state.settings_mode = false;
            state.join_mode = false;
            state.refresh_plan();
            Task::none()
        }
        Message::SelectJoin => {
            state.settings_mode = false;
            state.join_mode = true;
            state.refresh_plan();
            Task::none()
        }
        Message::SelectSettings => {
            state.settings_mode = true;
            Task::none()
        }
        Message::OperationChanged(operation) => {
            state.request.operation = operation;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinLeftChanged(value) => {
            state.join_request.left_path = value;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinRightChanged(value) => {
            state.join_request.right_path = value;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinLeftKeyChanged(value) => {
            state.join_request.left_key = value;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinRightKeyChanged(value) => {
            state.join_request.right_key = value;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinFormatChanged(format) => {
            state.join_request.format = format;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinKindChanged(kind) => {
            state.join_request.kind = kind;
            state.refresh_plan();
            Task::none()
        }
        Message::JoinOffsetChanged(value) => {
            state.offset = value.clone();
            state.join_request.offset = value.parse().unwrap_or(0);
            state.refresh_plan();
            Task::none()
        }
        Message::JoinLimitChanged(value) => {
            state.limit = value.clone();
            state.join_request.limit = if value.is_empty() {
                None
            } else {
                value.parse().ok()
            };
            state.refresh_plan();
            Task::none()
        }
        Message::FormatChanged(format) => {
            state.request.format = format;
            if state.request.format != Format::Jsonl {
                state.request.lean_jsonl = false;
            }
            state.refresh_plan();
            Task::none()
        }
        Message::LeanJsonlChanged(value) => {
            state.lean_jsonl = value;
            state.request.lean_jsonl = value;
            state.refresh_plan();
            Task::none()
        }
        Message::MaxInputBytesChanged(value) => {
            state.max_input_bytes = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.max_input_bytes = parsed;
                state.join_request.max_input_bytes = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::MaxItemBytesChanged(value) => {
            state.max_item_bytes = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.max_item_bytes = parsed;
                state.join_request.max_item_bytes = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::MaxItemsPerListChanged(value) => {
            state.max_items_per_list = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.max_items_per_list = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::MaxTotalItemsChanged(value) => {
            state.max_total_items = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.max_total_items = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::MaxListsChanged(value) => {
            state.max_lists = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.max_lists = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::ZipPolicyChanged(policy) => {
            if matches!(state.request.operation, AppOperation::Zip { .. }) {
                state.request.operation = AppOperation::Zip { on_unequal: policy };
                state.refresh_plan();
            }
            Task::none()
        }
        Message::ReverseChanged(value) => {
            state.request.options.reverse = value;
            state.refresh_plan();
            Task::none()
        }
        Message::ReverseFieldsChanged(value) => {
            if let AppOperation::Product { .. } = state.request.operation {
                state.request.operation = AppOperation::Product {
                    reverse_fields: value,
                };
                state.refresh_plan();
            }
            Task::none()
        }
        Message::OffsetChanged(value) => {
            state.offset = value.clone();
            if let Ok(parsed) = value.parse() {
                state.request.options.offset = parsed;
                state.refresh_plan();
            } else {
                state.error = Some("OFFSET_INVALID: enter a non-negative integer".into());
            }
            Task::none()
        }
        Message::LimitChanged(value) => {
            state.limit = value.clone();
            if value.is_empty() {
                state.request.options.limit = None;
                state.refresh_plan();
            } else if let Ok(parsed) = value.parse() {
                state.request.options.limit = Some(parsed);
                state.refresh_plan();
            } else {
                state.error = Some("LIMIT_INVALID: enter a non-negative integer".into());
            }
            Task::none()
        }
        Message::ChooseChanged(value) => {
            state.choose = value.clone();
            if let Ok(parsed) = value.parse() {
                if matches!(state.request.operation, AppOperation::Combinations { .. }) {
                    state.request.operation = AppOperation::Combinations { choose: parsed };
                    state.refresh_plan();
                }
            }
            Task::none()
        }
        Message::LengthChanged(value) => {
            state.length = value.clone();
            if let Ok(parsed) = value.parse() {
                if matches!(state.request.operation, AppOperation::Variations { .. }) {
                    state.request.operation = AppOperation::Variations { length: parsed };
                    state.refresh_plan();
                }
            }
            Task::none()
        }
        Message::OutputPathChanged(value) => {
            state.output_path = value;
            Task::none()
        }
        Message::OverwriteChanged(value) => {
            state.overwrite = value;
            Task::none()
        }
        Message::MaxCombinationsChanged(value) => {
            state.max_combinations = value.clone();
            state.update_limit(value, true);
            Task::none()
        }
        Message::MaxOutputBytesChanged(value) => {
            state.max_output_bytes = value.clone();
            state.join_request.max_output_bytes = value.parse().unwrap_or(0);
            state.update_limit(value, false);
            Task::none()
        }
        Message::TimeoutChanged(value) => {
            state.timeout_ms = value.clone();
            state.request.timeout_ms = if value.is_empty() {
                None
            } else {
                value.parse().ok()
            };
            state.join_request.timeout_ms = state.request.timeout_ms;
            state.refresh_plan();
            Task::none()
        }
        Message::ShardIndexChanged(value) => {
            state.shard_index = value.clone();
            state.request.shard_index = if value.is_empty() {
                None
            } else {
                value.parse().ok()
            };
            state.refresh_plan();
            Task::none()
        }
        Message::ShardCountChanged(value) => {
            state.shard_count = value.clone();
            state.request.shard_count = if value.is_empty() {
                None
            } else {
                value.parse().ok()
            };
            state.refresh_plan();
            Task::none()
        }
        Message::JoinMaxRecordsChanged(value) => {
            state.join_max_records = value.clone();
            if let Ok(parsed) = value.parse() {
                state.join_request.max_join_records = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::JoinFanoutChanged(value) => {
            state.join_fanout = value.clone();
            if let Ok(parsed) = value.parse() {
                state.join_request.max_join_key_fanout = parsed;
                state.refresh_plan();
            }
            Task::none()
        }
        Message::Preview => {
            if state.join_mode {
                state.run_join_preview(PREVIEW_LIMIT, "Join preview ready");
            } else {
                state.run_preview(PREVIEW_LIMIT, "Preview ready");
            }
            Task::none()
        }
        Message::Generate => state.start_generation(),
        Message::Cancel => {
            if let Some(token) = &state.cancellation {
                token.cancel();
                state.status = "Cancellation requested...".to_string();
            }
            Task::none()
        }
        Message::Tick => {
            state.refresh_progress();
            Task::none()
        }
        Message::GenerationFinished(result) => {
            state.running = false;
            state.cancellation = None;
            match result {
                Ok(progress) => {
                    state.status = format!(
                        "Wrote {} records ({} bytes) to {}",
                        progress.records, progress.bytes, state.output_path
                    );
                    state.error = None;
                }
                Err(error) => {
                    state.error = Some(format!("{}: {}", error.code, error.message));
                }
            }
            Task::none()
        }
    }
}

fn subscription(state: &CombinatorGui) -> Subscription<Message> {
    if state.running {
        iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    } else {
        Subscription::none()
    }
}

fn view(state: &CombinatorGui) -> Element<'_, Message> {
    let combine_tab = if !state.join_mode && !state.settings_mode {
        button("Combine").style(iced::widget::button::primary)
    } else {
        button("Combine").on_press(Message::SelectCombine)
    };
    let join_tab = if state.join_mode {
        if state.settings_mode {
            button("Join").on_press(Message::SelectJoin)
        } else {
            button("Join").style(iced::widget::button::primary)
        }
    } else {
        button("Join").on_press(Message::SelectJoin)
    };
    let settings_tab = if state.settings_mode {
        button("Settings").style(iced::widget::button::primary)
    } else {
        button("Settings").on_press(Message::SelectSettings)
    };
    let content: Element<'_, Message> = if state.settings_mode {
        settings_view(state)
    } else if state.join_mode {
        join_view(state)
    } else {
        combine_view(state)
    };
    container(
        column![
            row![combine_tab, join_tab, settings_tab].spacing(8),
            content
        ]
        .spacing(12),
    )
    .padding(24)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn combine_view(state: &CombinatorGui) -> Element<'_, Message> {
    let inputs = state
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let remove = if state.sources.len() > 1 {
                button(text("✕"))
                    .on_press(Message::RemoveList(index))
                    .style(iced::widget::button::danger)
            } else {
                button(text("✕")).style(iced::widget::button::danger)
            };
            let is_file = state.file_sources[index].is_some();
            let editor: Element<'_, Message> = if is_file {
                row![
                    labeled_text_input(
                        "File path",
                        state.file_sources[index].as_deref().unwrap_or_default(),
                        move |value| Message::FilePathChanged(index, value),
                    ),
                    button("Browse…").on_press(Message::BrowseFile(index)),
                ]
                .spacing(8)
                .align_y(Alignment::End)
                .width(Length::Fill)
                .into()
            } else {
                labeled_text_input("Values separated by commas", source, move |value| {
                    Message::SourceChanged(index, value)
                })
            };
            let format_control: Element<'_, Message> = if is_file {
                column![
                    text("File Delimiter").size(13),
                    pick_list(
                        FILE_FORMAT_OPTIONS,
                        Some(input_format_label(state.file_formats[index])),
                        move |label| Message::FileFormatChanged(index, parse_input_format(label)),
                    )
                    .placeholder("Input format")
                    .width(Length::Fixed(140.0)),
                ]
                .spacing(4)
                .into()
            } else {
                text("Inline input").into()
            };
            container(
                column![
                    row![
                        text(format!("List {}", index + 1)).size(16),
                        Space::new().width(Length::Fill),
                        remove,
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    row![
                        checkbox(is_file)
                            .on_toggle(move |value| Message::FileModeChanged(index, value)),
                        text("File source"),
                        format_control,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    editor,
                ]
                .spacing(6),
            )
            .padding(10)
            .style(list_card_style)
            .width(Length::Fill)
        })
        .collect::<Vec<_>>();

    let input_panel = column![
        text("Inputs").size(24),
        labeled_text_input(
            "Inline delimiter",
            &state.list_delimiter,
            Message::ListDelimiterChanged,
        ),
        scrollable(column(inputs.into_iter().map(Into::into)).spacing(10)).height(Length::Fill),
        row![button("Add list").on_press(Message::AddList)].spacing(10),
        text("File sources support bounded Lines, CSV, TSV, and NUL input.").size(13),
        text("────────────────────────────────────────").size(12),
        text("Execution plan").size(18),
        plan_view(state.plan.as_ref()),
    ]
    .spacing(12)
    .width(Length::FillPortion(3))
    .height(Length::Fill);

    let template_control: Element<'_, Message> = if state.template_file_mode {
        column![
            row![
                checkbox(true).on_toggle(Message::TemplateFileModeChanged),
                text("Template file"),
            ]
            .spacing(8),
            row![
                labeled_text_input(
                    "Template file path (optional)",
                    &state.template_file,
                    Message::TemplateFileChanged,
                ),
                button("Browse…")
                    .on_press(Message::BrowseTemplate)
                    .width(Length::Shrink),
            ]
            .spacing(8)
            .align_y(Alignment::End)
            .width(Length::Fill),
        ]
        .spacing(6)
        .into()
    } else {
        column![
            row![
                checkbox(false).on_toggle(Message::TemplateFileModeChanged),
                text("Template file"),
            ]
            .spacing(8),
            labeled_text_input(
                "Template (optional)",
                &state.template,
                Message::TemplateChanged
            ),
        ]
        .spacing(6)
        .into()
    };

    let operation_control = column![
        text("Operation").size(13),
        pick_list(
            OPERATION_OPTIONS,
            Some(operation_label(state.request.operation)),
            move |label| {
                Message::OperationChanged(parse_operation(
                    label,
                    state.choose.parse().unwrap_or(2),
                    state.length.parse().unwrap_or(2),
                ))
            },
        )
        .width(Length::Fill),
    ]
    .spacing(4)
    .width(Length::Fill);

    let format_control = column![
        text("Output format").size(13),
        pick_list(
            FORMAT_OPTIONS,
            Some(format_label(state.request.format)),
            |label| Message::FormatChanged(parse_format(label)),
        )
        .width(Length::Fill),
    ]
    .spacing(4)
    .width(Length::Fill);

    let lean_jsonl_control: Element<'_, Message> = if state.request.format == Format::Jsonl {
        row![
            checkbox(state.lean_jsonl).on_toggle(Message::LeanJsonlChanged),
            text("Lean JSONL (omit metadata fields)"),
        ]
        .spacing(8)
        .into()
    } else {
        text("").into()
    };
    let names_control: Element<'_, Message> = if state.request.format == Format::Jsonl {
        labeled_text_editor(
            "Field names, one per line",
            &state.names_editor,
            Message::NamesChanged,
        )
    } else {
        text("").into()
    };
    let field_separator_control: Element<'_, Message> =
        if format_uses_field_separator(state.request.format) {
            labeled_text_input(
                "Field separator",
                &state.request.field_separator,
                Message::SeparatorChanged,
            )
        } else {
            text("").into()
        };
    let unequal_control: Element<'_, Message> =
        if let AppOperation::Zip { on_unequal } = state.request.operation {
            column![
                text("Unequal list lengths").size(13),
                pick_list(
                    ZIP_POLICY_OPTIONS,
                    Some(zip_policy_label(on_unequal)),
                    |label| Message::ZipPolicyChanged(parse_zip_policy(label)),
                )
                .width(Length::Fill),
            ]
            .spacing(4)
            .width(Length::Fill)
            .into()
        } else {
            text("").into()
        };

    let plan_content = column![
        text("Data Selection and pre-processing").size(20),
        operation_control,
        unequal_control,
        operation_controls(state),
        labeled_text_editor(
            "Filters, one per line (eq, neq, prefix, suffix, glob, length)",
            &state.filters_editor,
            Message::FiltersChanged,
        ),
        text("Output Options").size(20),
        row![
            checkbox(state.request.options.reverse).on_toggle(Message::ReverseChanged),
            text("Reverse output"),
        ]
        .spacing(8),
        labeled_text_editor(
            "Transforms, one per line",
            &state.transforms_editor,
            Message::TransformsChanged,
        ),
        template_control,
        row![format_control].width(Length::Fill),
        field_separator_control,
        lean_jsonl_control,
        names_control,
        text("Sharding").size(16),
        row![
            labeled_text_input(
                "Shard index",
                &state.shard_index,
                Message::ShardIndexChanged
            ),
            labeled_text_input(
                "Shard count",
                &state.shard_count,
                Message::ShardCountChanged
            ),
        ]
        .spacing(8)
        .width(Length::Fill),
        row![
            labeled_text_input(
                "Output file path",
                &state.output_path,
                Message::OutputPathChanged,
            ),
            button("Browse…")
                .on_press(Message::BrowseOutput)
                .width(Length::Shrink),
        ]
        .spacing(8)
        .align_y(Alignment::End)
        .width(Length::Fill),
        status_view(state),
        preview_view(&state.records),
    ]
    .spacing(12)
    .padding(Padding {
        top: 0.0,
        right: 24.0,
        bottom: 0.0,
        left: 0.0,
    })
    .width(Length::Fill);

    let plan_panel = column![
        scrollable(plan_content)
            .width(Length::Fill)
            .height(Length::Fill),
        row![
            button("Preview first 20").on_press(Message::Preview),
            generation_button(state)
        ]
        .spacing(10),
    ]
    .spacing(12)
    .width(Length::FillPortion(2))
    .height(Length::Fill);

    container(row![input_panel, plan_panel].spacing(24))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn settings_view(state: &CombinatorGui) -> Element<'_, Message> {
    let content = column![
        text("Settings").size(24),
        text("Shared execution policy and safety limits used by Combine and Join."),
        text("Shared limits").size(18),
        labeled_text_input(
            "Maximum output bytes",
            &state.max_output_bytes,
            Message::MaxOutputBytesChanged,
        ),
        labeled_text_input(
            "Maximum input bytes per source",
            &state.max_input_bytes,
            Message::MaxInputBytesChanged,
        ),
        labeled_text_input(
            "Maximum item bytes",
            &state.max_item_bytes,
            Message::MaxItemBytesChanged,
        ),
        labeled_text_input(
            "Timeout in milliseconds (optional)",
            &state.timeout_ms,
            Message::TimeoutChanged,
        ),
        row![
            checkbox(state.overwrite).on_toggle(Message::OverwriteChanged),
            text("Overwrite existing output file"),
        ]
        .spacing(8),
        text("Combine limits").size(18),
        labeled_text_input(
            "Maximum combinations",
            &state.max_combinations,
            Message::MaxCombinationsChanged,
        ),
        row![
            labeled_text_input(
                "Max items/list",
                &state.max_items_per_list,
                Message::MaxItemsPerListChanged,
            ),
            labeled_text_input("Max lists", &state.max_lists, Message::MaxListsChanged),
        ]
        .spacing(8)
        .width(Length::Fill),
        labeled_text_input(
            "Max total items",
            &state.max_total_items,
            Message::MaxTotalItemsChanged,
        ),
        text("Join limits").size(18),
        row![
            labeled_text_input(
                "Max join records",
                &state.join_max_records,
                Message::JoinMaxRecordsChanged,
            ),
            labeled_text_input(
                "Max key fanout",
                &state.join_fanout,
                Message::JoinFanoutChanged,
            ),
        ]
        .spacing(8)
        .width(Length::Fill),
    ]
    .spacing(12)
    .padding(Padding {
        top: 0.0,
        right: 24.0,
        bottom: 0.0,
        left: 0.0,
    })
    .width(Length::Fill);
    container(scrollable(content).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn generation_button(state: &CombinatorGui) -> iced::widget::Button<'static, Message> {
    if state.running {
        button("Cancel").on_press(Message::Cancel)
    } else {
        button("Generate file").on_press(Message::Generate)
    }
}

fn list_card_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgb8(47, 49, 54))),
        border: Border {
            width: 1.0,
            radius: 6.0.into(),
            color: Color::from_rgb8(78, 81, 88),
        },
        ..Default::default()
    }
}

fn plan_view(plan: Option<&ExecutionPlan>) -> Element<'static, Message> {
    let content: Element<'static, Message> = match plan {
        Some(plan) => column![
            row![
                plan_metric("Lists", plan.list_lengths.len().to_string()),
                plan_metric("Items", format!("{:?}", plan.list_lengths)),
                plan_metric("Combinations", format!("{:?}", plan.total_combinations)),
            ]
            .spacing(12)
            .width(Length::Fill),
            row![
                plan_metric("Selected", plan.records_to_emit.to_string()),
                plan_metric(
                    "Estimated bytes",
                    format!("{:?}", plan.estimated_output_bytes)
                ),
                plan_metric("Warnings", plan.warnings.len().to_string()),
            ]
            .spacing(12)
            .width(Length::Fill),
        ]
        .spacing(6)
        .width(Length::Fill)
        .into(),
        None => text("No valid plan yet").into(),
    };
    container(content)
        .padding(8)
        .style(plan_card_style)
        .width(Length::Fill)
        .into()
}

fn plan_metric(label: &'static str, value: String) -> Element<'static, Message> {
    container(column![text(label).size(12), text(value).size(14)].spacing(2))
        .width(Length::Fill)
        .into()
}

fn plan_card_style(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgb8(43, 45, 50))),
        border: Border {
            width: 1.0,
            radius: 6.0.into(),
            color: Color::from_rgb8(88, 91, 99),
        },
        ..Default::default()
    }
}

fn status_view(state: &CombinatorGui) -> Element<'_, Message> {
    match &state.error {
        Some(error) => text(format!("Error: {error}")).size(15).into(),
        None => text(&state.status).size(15).into(),
    }
}

fn preview_view(records: &[PreviewRecord]) -> Element<'_, Message> {
    let rows = records
        .iter()
        .map(|record| text(format!("{}  {}", record.ordinal, record.value.trim_end())))
        .collect::<Vec<_>>();
    container(
        scrollable(
            column(rows.into_iter().map(Into::into))
                .spacing(4)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fixed(220.0)),
    )
    .padding(12)
    .width(Length::Fill)
    .into()
}

fn labeled_text_input<'a, F>(label: &'a str, value: &'a str, on_input: F) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    column![
        text(label).size(13),
        text_input("", value).on_input(on_input).width(Length::Fill),
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

fn labeled_text_editor<'a, F>(
    label: &'static str,
    content: &'a text_editor::Content,
    on_action: F,
) -> Element<'a, Message>
where
    F: Fn(text_editor::Action) -> Message + 'a,
{
    column![
        text(label).size(13),
        text_editor(content)
            .on_action(on_action)
            .height(Length::Fixed(96.0)),
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

fn join_plan_view(plan: Option<&JoinPlan>) -> Element<'static, Message> {
    let content: Element<'static, Message> = match plan {
        Some(plan) => column![
            row![
                plan_metric("Left records", plan.left_records.to_string()),
                plan_metric("Right records", plan.right_records.to_string()),
            ]
            .spacing(12)
            .width(Length::Fill),
            row![
                plan_metric("Join records", plan.total_records.to_string()),
                plan_metric("Selected", plan.records_to_emit.to_string()),
            ]
            .spacing(12)
            .width(Length::Fill),
        ]
        .spacing(6)
        .width(Length::Fill)
        .into(),
        None => text("No valid join plan").into(),
    };
    container(content)
        .padding(8)
        .style(plan_card_style)
        .width(Length::Fill)
        .into()
}

fn join_view(state: &CombinatorGui) -> Element<'_, Message> {
    let content = column![
        text("Structured join").size(24),
        row![
            labeled_text_input(
                "Left CSV/TSV/JSONL path",
                &state.join_request.left_path,
                Message::JoinLeftChanged,
            ),
            button("Browse…")
                .on_press(Message::BrowseJoinLeft)
                .width(Length::Shrink),
        ]
        .spacing(8)
        .align_y(Alignment::End)
        .width(Length::Fill),
        row![
            labeled_text_input(
                "Right CSV/TSV/JSONL path",
                &state.join_request.right_path,
                Message::JoinRightChanged,
            ),
            button("Browse…")
                .on_press(Message::BrowseJoinRight)
                .width(Length::Shrink),
        ]
        .spacing(8)
        .align_y(Alignment::End)
        .width(Length::Fill),
        row![
            labeled_text_input(
                "Left key",
                &state.join_request.left_key,
                Message::JoinLeftKeyChanged
            ),
            labeled_text_input(
                "Right key",
                &state.join_request.right_key,
                Message::JoinRightKeyChanged,
            ),
        ]
        .spacing(8)
        .width(Length::Fill),
        row![
            labeled_text_input("Offset", &state.offset, Message::JoinOffsetChanged),
            labeled_text_input("Limit (optional)", &state.limit, Message::JoinLimitChanged),
        ]
        .spacing(8)
        .width(Length::Fill),
        row![
            column![
                text("Format").size(13),
                pick_list(
                    JOIN_FORMAT_OPTIONS,
                    Some(join_format_label(state.join_request.format)),
                    |label| Message::JoinFormatChanged(parse_join_format(label)),
                )
                .width(Length::Fill),
            ]
            .spacing(4)
            .width(Length::Fill),
            column![
                text("Type").size(13),
                pick_list(
                    JOIN_KIND_OPTIONS,
                    Some(join_kind_label(state.join_request.kind)),
                    |label| Message::JoinKindChanged(parse_join_kind(label)),
                )
                .width(Length::Fill),
            ]
            .spacing(4)
            .width(Length::Fill),
        ]
        .spacing(8),
        row![
            labeled_text_input(
                "Output file path",
                &state.output_path,
                Message::OutputPathChanged,
            ),
            button("Browse…")
                .on_press(Message::BrowseOutput)
                .width(Length::Shrink),
        ]
        .spacing(8)
        .align_y(Alignment::End)
        .width(Length::Fill),
        text("Execution plan").size(18),
        join_plan_view(state.join_plan.as_ref()),
        status_view(state),
        preview_view(&state.records),
    ]
    .spacing(12)
    .padding(Padding {
        top: 0.0,
        right: 24.0,
        bottom: 0.0,
        left: 0.0,
    })
    .width(Length::Fill);
    container(
        column![
            scrollable(content).height(Length::Fill),
            row![
                button("Preview first 20").on_press(Message::Preview),
                generation_button(state),
            ]
            .spacing(10),
        ]
        .spacing(12),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

impl CombinatorGui {
    fn refresh_plan(&mut self) {
        if self.join_mode {
            if self.join_request.left_path.trim().is_empty()
                || self.join_request.right_path.trim().is_empty()
            {
                self.join_plan = None;
                self.error = None;
                self.status = "Enter both join input paths".into();
                return;
            }
            if self.join_request.left_key.trim().is_empty()
                || self.join_request.right_key.trim().is_empty()
            {
                self.join_plan = None;
                self.error = None;
                self.status = "Enter both join keys".into();
                return;
            }
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

    fn start_generation(&mut self) -> Task<Message> {
        self.refresh_plan();
        if (!self.join_mode && self.plan.is_none()) || (self.join_mode && self.join_plan.is_none())
        {
            return Task::none();
        }
        if self.join_mode {
            return self.start_join_generation();
        }
        let sink = match FileSink::open(&self.output_path, self.overwrite) {
            Ok(sink) => sink,
            Err(error) => {
                self.error = Some(format!("{}: {}", error.code, error.message));
                return Task::none();
            }
        };
        let request = self.request.clone();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let progress = Arc::new(Mutex::new(ProgressEvent {
            records: 0,
            bytes: 0,
        }));
        let worker_progress = progress.clone();
        self.running = true;
        self.cancellation = Some(token);
        self.progress = Some(progress);
        self.status = "Generating...".to_string();
        self.error = None;

        Task::perform(
            async move {
                let mut sink = ProgressFileSink {
                    sink,
                    progress: worker_progress,
                };
                let cancelled = || worker_token.is_cancelled();
                match stream(&request, &mut sink, Some(&cancelled)) {
                    Ok(progress) => {
                        sink.sink.commit()?;
                        Ok(progress)
                    }
                    Err(error) => Err(error),
                }
            },
            Message::GenerationFinished,
        )
    }

    fn start_join_generation(&mut self) -> Task<Message> {
        let sink = match FileSink::open(&self.output_path, self.overwrite) {
            Ok(sink) => sink,
            Err(error) => {
                self.error = Some(format!("{}: {}", error.code, error.message));
                return Task::none();
            }
        };
        let request = self.join_request.clone();
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let progress = Arc::new(Mutex::new(ProgressEvent {
            records: 0,
            bytes: 0,
        }));
        let worker_progress = progress.clone();
        self.running = true;
        self.cancellation = Some(token);
        self.progress = Some(progress);
        self.status = "Generating join...".into();
        self.error = None;
        Task::perform(
            async move {
                let mut sink = ProgressFileSink {
                    sink,
                    progress: worker_progress,
                };
                let cancelled = || worker_token.is_cancelled();
                match join_stream(&request, &mut sink, Some(&cancelled)) {
                    Ok(progress) => {
                        sink.sink.commit()?;
                        Ok(progress)
                    }
                    Err(error) => Err(error),
                }
            },
            Message::GenerationFinished,
        )
    }

    fn refresh_progress(&mut self) {
        if let Some(progress) = &self.progress {
            if let Ok(progress) = progress.lock() {
                self.status = format!(
                    "Generating... {} records ({} bytes)",
                    progress.records, progress.bytes
                );
            }
        }
    }

    fn update_limit(&mut self, value: String, combinations: bool) {
        match value.parse::<u128>() {
            Ok(value) if value > 0 => {
                if combinations {
                    self.request.max_combinations = value;
                } else {
                    self.request.max_output_bytes = value;
                }
                self.refresh_plan();
            }
            _ => {
                self.error = Some("LIMIT_INVALID: enter a positive integer".to_string());
            }
        }
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

const OPERATION_OPTIONS: &[&str] = &[
    "Product",
    "Zip",
    "Concat",
    "Permutations",
    "Combinations",
    "Variations",
];

fn parse_operation(label: &str, choose: usize, length: usize) -> AppOperation {
    match label {
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

const FORMAT_OPTIONS: &[&str] = &["Text", "JSONL", "CSV", "TSV", "NUL"];

fn format_label(format: Format) -> &'static str {
    match format {
        Format::Text => "Text",
        Format::Jsonl => "JSONL",
        Format::Csv => "CSV",
        Format::Tsv => "TSV",
        Format::Nul => "NUL",
    }
}

fn format_uses_field_separator(format: Format) -> bool {
    matches!(format, Format::Text | Format::Jsonl | Format::Nul)
}

fn parse_format(label: &str) -> Format {
    match label {
        "JSONL" => Format::Jsonl,
        "CSV" => Format::Csv,
        "TSV" => Format::Tsv,
        "NUL" => Format::Nul,
        _ => Format::Text,
    }
}

const ZIP_POLICY_OPTIONS: &[&str] = &["Error", "Truncate", "Cycle"];

fn zip_policy_label(policy: UnequalPolicy) -> &'static str {
    match policy {
        UnequalPolicy::Error => "Error",
        UnequalPolicy::Truncate => "Truncate",
        UnequalPolicy::Cycle => "Cycle",
    }
}

fn parse_zip_policy(label: &str) -> UnequalPolicy {
    match label {
        "Truncate" => UnequalPolicy::Truncate,
        "Cycle" => UnequalPolicy::Cycle,
        _ => UnequalPolicy::Error,
    }
}

const FILE_FORMAT_OPTIONS: &[&str] = &["Lines", "CSV", "TSV", "NUL"];

fn input_format_label(format: InputFormat) -> &'static str {
    match format {
        InputFormat::Lines => "Lines",
        InputFormat::Csv => "CSV",
        InputFormat::Tsv => "TSV",
        InputFormat::Nul => "NUL",
    }
}

fn parse_input_format(label: &str) -> InputFormat {
    match label {
        "CSV" => InputFormat::Csv,
        "TSV" => InputFormat::Tsv,
        "NUL" => InputFormat::Nul,
        _ => InputFormat::Lines,
    }
}

const JOIN_FORMAT_OPTIONS: &[&str] = &["CSV", "TSV", "JSONL"];

fn join_format_label(format: JoinFormat) -> &'static str {
    match format {
        JoinFormat::Csv => "CSV",
        JoinFormat::Tsv => "TSV",
        JoinFormat::Jsonl => "JSONL",
    }
}

fn parse_join_format(label: &str) -> JoinFormat {
    match label {
        "TSV" => JoinFormat::Tsv,
        "JSONL" => JoinFormat::Jsonl,
        _ => JoinFormat::Csv,
    }
}

const JOIN_KIND_OPTIONS: &[&str] = &["Inner", "Left", "Full", "Anti"];

fn join_kind_label(kind: JoinKind) -> &'static str {
    match kind {
        JoinKind::Inner => "Inner",
        JoinKind::Left => "Left",
        JoinKind::Full => "Full",
        JoinKind::Anti => "Anti",
    }
}

fn parse_join_kind(label: &str) -> JoinKind {
    match label {
        "Left" => JoinKind::Left,
        "Full" => JoinKind::Full,
        "Anti" => JoinKind::Anti,
        _ => JoinKind::Inner,
    }
}

fn operation_controls(state: &CombinatorGui) -> Element<'_, Message> {
    let mut controls = column![row![
        labeled_text_input("Offset", &state.offset, Message::OffsetChanged),
        labeled_text_input("Limit (optional)", &state.limit, Message::LimitChanged),
    ]
    .spacing(8)
    .width(Length::Fill),]
    .spacing(8)
    .width(Length::Fill);
    if let AppOperation::Product { reverse_fields } = state.request.operation {
        controls = controls.push(
            row![
                checkbox(reverse_fields).on_toggle(Message::ReverseFieldsChanged),
                text("Leftmost first"),
            ]
            .spacing(8),
        );
    }
    if let AppOperation::Combinations { choose: _ } = state.request.operation {
        controls = controls.push(labeled_text_input(
            "Choose",
            &state.choose,
            Message::ChooseChanged,
        ));
    }
    if let AppOperation::Variations { length: _ } = state.request.operation {
        controls = controls.push(labeled_text_input(
            "Length",
            &state.length,
            Message::LengthChanged,
        ));
    }
    controls.into()
}
