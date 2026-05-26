#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum RenderMode {
    LINE = 0,
    COLUMN,
    LIST,
    SEMI,
    HTML,
    INSERT,
    QUOTE,
    TCL,
    CSV,
    EXPLAIN,
    DESCRIBE,
    ASCII,
    PRETTY,
    EQP,
    JSON,
    MARKDOWN,
    TABLE,
    BOX,
    LATEX,
    TRASH,
    JSONLINES,
    DUCKBOX,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionType {
    Default,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PagerMode {
    Automatic,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BailOnError {
    Automatic,
    Bail,
    DontBail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadLineVersion {
    Linenoise,
    Fallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HighlightMode {
    Automatic,
    Mixed,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintIntensity {
    Standard,
    Bold,
    Underline,
    BoldUnderline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintColor {
    Standard,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    BrightGray,
    Gray,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    White,
    Extended(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HighlightStyle {
    pub color: PrintColor,
    pub intensity: PrintIntensity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum MetadataResult {
    Success = 0,
    Fail = 1,
    Exit = 2,
    PrintUsage = 3,
}

#[repr(u32)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub enum ShellFlags {
    SHFLG_Newlines = 0x00000010,
    SHFLG_CountChanges = 0x00000020,
    SHFLG_Echo = 0x00000040,
    SHFLG_HeaderSet = 0x00000080,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum InputMode {
    Standard,
    File,
    DuckDbRc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum StartupText {
    All,
    Version,
    None,
}

#[derive(Clone, Debug)]
pub enum InitialAction {
    Command { text: String, bail_on_error: bool },
    File { path: String, bail_on_error: bool },
}

#[allow(non_snake_case)]
#[allow(dead_code)]
pub struct ShellState {
    pub program_name: String,

    pub zDbFilename: String,
    pub opened_transient_in_memory: bool,
    pub initFile: String,
    pub run_init: bool,

    pub readStdin: bool,
    pub stdin_is_interactive: bool,
    pub stdout_is_console: bool,
    pub stderr_is_console: bool,
    pub stdin_temp_path: Option<String>,

    pub bail: BailOnError,
    pub safe_mode: bool,

    pub mode: RenderMode,
    pub cMode: RenderMode,
    pub normalMode: RenderMode,
    pub modePrior: RenderMode,
    pub priorShFlgs: u32,
    pub colSepPrior: String,
    pub rowSepPrior: String,

    pub highlighting_enabled: bool,
    pub highlight_results: OptionType,
    pub highlight_errors: OptionType,
    pub highlight_style_error: HighlightStyle,
    pub highlight_style_column_name: HighlightStyle,
    pub highlight_style_column_type: HighlightStyle,
    pub highlight_style_null_value: HighlightStyle,
    pub highlight_styles: HashMap<String, HighlightStyle>,

    pub pager_mode: PagerMode,
    pub pager_command: String,
    pub pager_min_rows: u64,
    pub pager_min_cols: u64,

    pub rl_version: ReadLineVersion,
    pub render_completion: bool,
    pub render_errors: bool,
    pub highlight_mode: HighlightMode,
    #[cfg(target_os = "windows")]
    pub win_utf8_mode: bool,

    pub mainPrompt: String,
    pub continuePrompt: String,
    pub continuePromptSelected: String,

    pub colSeparator: String,
    pub rowSeparator: String,
    pub showHeader: bool,
    pub binary_mode: bool,
    pub shellFlgs: u32,
    pub colWidth: Vec<i32>,
    pub nullValue: String,
    pub zDestTable: String,
    pub max_rows: u64,
    pub max_width: u64,
    pub max_analyze_rows: u64,
    pub decimal_separator: u8,
    pub thousand_separator: u8,
    // 0=none,1=footer,2=all,3=default
    pub large_number_rendering: i32,
    pub columns: bool,
    pub outfile: String,
    pub outCount: u32,
    pub doXdgOpen: bool,
    pub zTempFile: String,
    pub out: crate::output::OutputHandle,
    pub log: Option<crate::output::OutputHandle>,
    pub timer_enabled: bool,
    pub last_changes: u64,
    pub total_changes: u64,
    pub last_query_duckbox: Option<String>,
    pub ui_command: String,
    pub progress_bar_components: Vec<String>,

    pub reserved_keywords_loaded: bool,
    pub reserved_keywords: HashSet<String>,
    pub applied_process_tz: Option<String>,

    pub config_kv: Vec<(String, String)>,
    pub storage_version: Option<String>,
    pub startup_text: StartupText,
    pub displayed_loading_resources_message: bool,
    pub duckdb_rc_path: Option<String>,

    pub print_help_and_exit: bool,
    pub print_version_and_exit: bool,

    pub exit_code: Option<i32>,

    pub initial_commands: Vec<InitialAction>,
    pub exit_after_initial_commands: bool,
    pub files_to_process: Vec<String>,
    pub launch_ui: bool,

    pub history_path: Option<String>,
    pub history_entries: Vec<String>,
}

impl ShellState {
    pub fn new(program_name: String) -> Self {
        ShellState {
            program_name,
            zDbFilename: String::new(),
            opened_transient_in_memory: false,
            initFile: String::new(),
            run_init: true,
            readStdin: true,
            stdin_is_interactive: true,
            stdout_is_console: true,
            stderr_is_console: true,
            stdin_temp_path: None,
            bail: BailOnError::Automatic,
            safe_mode: false,
            normalMode: RenderMode::DUCKBOX,
            cMode: RenderMode::DUCKBOX,
            mode: RenderMode::DUCKBOX,
            modePrior: RenderMode::DUCKBOX,
            priorShFlgs: 0,
            colSepPrior: "|".to_string(),
            rowSepPrior: "\n".to_string(),
            highlighting_enabled: true,
            highlight_results: OptionType::Default,
            highlight_errors: OptionType::Default,
            highlight_style_error: HighlightStyle {
                color: PrintColor::Red,
                intensity: PrintIntensity::Standard,
            },
            highlight_style_column_name: HighlightStyle {
                color: PrintColor::Standard,
                intensity: PrintIntensity::Standard,
            },
            highlight_style_column_type: HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            },
            highlight_style_null_value: HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            },
            highlight_styles: HashMap::new(),
            pager_mode: PagerMode::Automatic,
            pager_command: String::new(),
            pager_min_rows: 50,
            pager_min_cols: 0,
            rl_version: ReadLineVersion::Linenoise,
            render_completion: true,
            render_errors: true,
            highlight_mode: HighlightMode::Automatic,
            #[cfg(target_os = "windows")]
            win_utf8_mode: false,
            mainPrompt:
                "{max_length:40}{highlight_element:prompt}{setting:current_database_and_schema}{color:reset} D "
                    .to_string(),
            continuePrompt: "· ".to_string(),
            continuePromptSelected: "‣ ".to_string(),
            colSeparator: "|".to_string(),
            rowSeparator: "\n".to_string(),
            showHeader: true,
            binary_mode: false,
            shellFlgs: 0,
            colWidth: Vec::new(),
            nullValue: "NULL".to_string(),
            zDestTable: "\"table\"".to_string(),
            max_rows: 40,
            max_width: 0,
            max_analyze_rows: 0,
            decimal_separator: 0,
            thousand_separator: 0,
            large_number_rendering: 3,
            columns: false,
            outfile: String::new(),
            outCount: 0,
            doXdgOpen: false,
            zTempFile: String::new(),
            out: crate::output::OutputHandle::Stdout,
            log: None,
            timer_enabled: false,
            last_changes: 0,
            total_changes: 0,
            last_query_duckbox: None,
            ui_command: "CALL start_ui()".to_string(),
            progress_bar_components: vec![
                "{setting:progress_bar_percentage} {setting:progress_bar}{setting:eta}".to_string(),
                "{align:right}{min_size:18}{hide_if_contains:0 bytes}Written: {setting:bytes_written}"
                    .to_string(),
                "{align:right}{min_size:15}{hide_if_contains:0 bytes}Read: {setting:bytes_read}"
                    .to_string(),
                "{align:right}{min_size:17}Memory: {setting:memory_usage}".to_string(),
                "{align:right}{min_size:15}{hide_if_contains:0 bytes}Swap: {setting:swap_usage}"
                    .to_string(),
            ],
            reserved_keywords_loaded: false,
            reserved_keywords: HashSet::new(),
            applied_process_tz: None,
            config_kv: Vec::new(),
            storage_version: None,
            startup_text: StartupText::All,
            displayed_loading_resources_message: false,
            duckdb_rc_path: None,
            print_help_and_exit: false,
            print_version_and_exit: false,
            exit_code: None,
            initial_commands: Vec::new(),
            exit_after_initial_commands: false,
            files_to_process: Vec::new(),
            launch_ui: false,
            history_path: None,
            history_entries: Vec::new(),
        }
    }

    pub fn get_bail_on_error(&self, mode: InputMode) -> bool {
        match self.bail {
            BailOnError::Bail => true,
            BailOnError::DontBail => false,
            BailOnError::Automatic => matches!(mode, InputMode::File | InputMode::DuckDbRc),
        }
    }

    pub fn command_line_command_bail(&self) -> bool {
        match self.bail {
            BailOnError::DontBail => false,
            BailOnError::Automatic | BailOnError::Bail => true,
        }
    }
}
use std::collections::{HashMap, HashSet};
