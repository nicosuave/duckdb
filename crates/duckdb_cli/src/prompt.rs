use crate::state::{HighlightStyle, PrintColor, PrintIntensity, ShellState};
use std::ffi::{CStr, CString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptParseState {
    Standard,
    ParseBracketType,
    ParseBracketContent,
    Escaped,
}

#[derive(Clone, Debug)]
enum PromptComponent {
    Literal(String),
    Sql(String),
    SetColor(PrintColor),
    SetIntensity(PrintIntensity),
    SetHighlightElement(String),
    ResetColor,
    Setting(String),
}

#[derive(Clone, Debug)]
struct Prompt {
    components: Vec<PromptComponent>,
    max_length: Option<usize>,
}

const HIGHLIGHT_ELEMENTS: &[&str] = &[
    "error",
    "keyword",
    "numeric_constant",
    "string_constant",
    "line_indicator",
    "database_name",
    "schema_name",
    "table_name",
    "column_name",
    "column_type",
    "numeric_value",
    "string_value",
    "temporal_value",
    "null_value",
    "footer",
    "layout",
    "startup_text",
    "startup_version",
    "continuation",
    "continuation_selected",
    "bracket",
    "comment",
    "suggestion_catalog_name",
    "suggestion_schema_name",
    "suggestion_table_name",
    "suggestion_column_name",
    "suggestion_file_name",
    "suggestion_directory_name",
    "suggestion_function_name",
    "suggestion_setting_name",
    "table_layout",
    "view_layout",
    "primary_key_column",
    "prompt",
    "error_emphasis",
    "error_suggestion",
    "log_trace",
    "log_debug",
    "log_info",
    "log_warning",
    "none",
];

const SUPPORTED_SETTINGS: &[&str] = &[
    "current_database",
    "current_schema",
    "current_database_and_schema",
    "memory_limit",
    "memory_usage",
    "swap_usage",
    "swap_max",
    "bytes_written",
    "bytes_read",
];

const PROGRESS_BAR_SETTINGS: &[&str] = &[
    "current_database",
    "current_schema",
    "current_database_and_schema",
    "memory_limit",
    "memory_usage",
    "swap_usage",
    "swap_max",
    "bytes_written",
    "bytes_read",
    "progress_bar_percentage",
    "progress_bar",
    "eta",
];

#[derive(Clone, Copy)]
struct PromptParseOptions<'a> {
    supported_settings: &'a [&'a str],
    allow_progress_controls: bool,
}

impl Prompt {
    fn add_literal(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(PromptComponent::Literal(existing)) = self.components.last_mut() {
            existing.push_str(text);
            return;
        }
        self.components
            .push(PromptComponent::Literal(text.to_string()));
    }

    fn parse(prompt: &str) -> Result<Self, String> {
        Self::parse_with_options(
            prompt,
            PromptParseOptions {
                supported_settings: SUPPORTED_SETTINGS,
                allow_progress_controls: false,
            },
        )
    }

    fn parse_progress_bar(prompt: &str) -> Result<Self, String> {
        Self::parse_with_options(
            prompt,
            PromptParseOptions {
                supported_settings: PROGRESS_BAR_SETTINGS,
                allow_progress_controls: true,
            },
        )
    }

    fn parse_with_options(prompt: &str, options: PromptParseOptions<'_>) -> Result<Self, String> {
        let mut out = Prompt {
            components: Vec::new(),
            max_length: None,
        };
        let mut parse_state = PromptParseState::Standard;
        let mut prev_state = parse_state;
        let mut bracket_type = String::new();
        let mut literal = String::new();

        for c in prompt.chars() {
            match parse_state {
                PromptParseState::Standard => match c {
                    '\\' => {
                        prev_state = parse_state;
                        parse_state = PromptParseState::Escaped;
                    }
                    '{' => {
                        out.add_literal(&literal);
                        literal.clear();
                        parse_state = PromptParseState::ParseBracketType;
                    }
                    _ => literal.push(c),
                },
                PromptParseState::Escaped => {
                    literal.push(c);
                    parse_state = prev_state;
                }
                PromptParseState::ParseBracketType => match c {
                    '}' => {
                        out.add_component(&literal, "", options)?;
                        literal.clear();
                        parse_state = PromptParseState::Standard;
                    }
                    ':' => {
                        bracket_type = std::mem::take(&mut literal);
                        parse_state = PromptParseState::ParseBracketContent;
                    }
                    '\\' => {
                        prev_state = parse_state;
                        parse_state = PromptParseState::Escaped;
                    }
                    _ => literal.push(c),
                },
                PromptParseState::ParseBracketContent => match c {
                    '}' => {
                        out.add_component(&bracket_type, &literal, options)?;
                        bracket_type.clear();
                        literal.clear();
                        parse_state = PromptParseState::Standard;
                    }
                    '\\' => {
                        prev_state = parse_state;
                        parse_state = PromptParseState::Escaped;
                    }
                    _ => literal.push(c),
                },
            }
        }

        if parse_state != PromptParseState::Standard {
            return Err(format!(
                "Failed to parse prompt \"{}\" - unterminated bracket or escape",
                prompt
            ));
        }
        out.add_literal(&literal);
        Ok(out)
    }

    fn add_component(
        &mut self,
        bracket_type: &str,
        value: &str,
        options: PromptParseOptions<'_>,
    ) -> Result<(), String> {
        match bracket_type {
            "setting" => {
                if value.is_empty() {
                    return Err("setting requires a parameter".to_string());
                }
                if !options.supported_settings.contains(&value) {
                    return Err(format!(
                        "unsupported setting \"{}\" for setting, supported values: {}",
                        value,
                        options.supported_settings.join(", ")
                    ));
                }
                self.components
                    .push(PromptComponent::Setting(value.to_string()));
            }
            "sql" => {
                if value.is_empty() {
                    return Err("sql requires a parameter".to_string());
                }
                self.components
                    .push(PromptComponent::Sql(value.to_string()));
            }
            "color" => {
                if value.is_empty() {
                    return Err("color requires a parameter".to_string());
                }
                match value {
                    "bold" => self
                        .components
                        .push(PromptComponent::SetIntensity(PrintIntensity::Bold)),
                    "underline" => self
                        .components
                        .push(PromptComponent::SetIntensity(PrintIntensity::Underline)),
                    "reset" => self.components.push(PromptComponent::ResetColor),
                    _ => {
                        let code = crate::display_colors::try_get_highlight_color_code(value)?;
                        self.components
                            .push(PromptComponent::SetColor(PrintColor::Extended(code)));
                    }
                }
            }
            "highlight_element" => {
                if value.is_empty() {
                    return Err("highlight_element requires a parameter".to_string());
                }
                if !HIGHLIGHT_ELEMENTS.contains(&value) {
                    return Err(format!(
                        "Unknown element '{}', supported options: {}\n",
                        value,
                        HIGHLIGHT_ELEMENTS.join(", ")
                    ));
                }
                self.components
                    .push(PromptComponent::SetHighlightElement(value.to_string()));
            }
            "max_length" => {
                if value.is_empty() {
                    return Err("max_length requires a parameter".to_string());
                }
                let max_length = value.parse::<usize>().map_err(|_| {
                    format!("Could not convert string '{}' to unsigned integer", value)
                })?;
                self.max_length = Some(max_length);
            }
            "align" if options.allow_progress_controls => match value {
                "left" | "right" => {}
                _ => {
                    return Err(format!(
                        "Unsupported type {} for align: expected left or right",
                        value
                    ));
                }
            },
            "content_align" if options.allow_progress_controls => match value {
                "left" | "right" | "middle" => {}
                _ => {
                    return Err(format!(
                        "Unsupported type {} for content_align: expected left, middle or right",
                        value
                    ));
                }
            },
            "hide_if_contains" if options.allow_progress_controls => {}
            "min_size" if options.allow_progress_controls => {
                if value.is_empty() {
                    return Err("min_size requires a parameter".to_string());
                }
                value.parse::<usize>().map_err(|_| {
                    format!("Could not convert string '{}' to unsigned integer", value)
                })?;
            }
            other => return Err(format!("Unknown bracket type {}", other)),
        }
        Ok(())
    }

    fn generate(&self, state: &ShellState, con: duckdb_sys::duckdb_connection) -> String {
        let mut prompt = String::new();
        let mut length = 0usize;

        for component in &self.components {
            match component {
                PromptComponent::Literal(text) => {
                    prompt.push_str(&self.handle_text(state, text, &mut length));
                }
                PromptComponent::Sql(query) => {
                    let result = execute_sql_single_value(con, query);
                    prompt.push_str(&self.handle_text(state, &result, &mut length));
                }
                PromptComponent::Setting(setting) => {
                    let value = handle_setting(con, setting);
                    prompt.push_str(&self.handle_text(state, &value, &mut length));
                }
                PromptComponent::SetColor(color) => {
                    if state.highlighting_enabled {
                        prompt.push_str(&terminal_code(HighlightStyle {
                            color: *color,
                            intensity: PrintIntensity::Standard,
                        }));
                    }
                }
                PromptComponent::SetIntensity(intensity) => {
                    if state.highlighting_enabled {
                        prompt.push_str(&terminal_code(HighlightStyle {
                            color: PrintColor::Standard,
                            intensity: *intensity,
                        }));
                    }
                }
                PromptComponent::SetHighlightElement(element) => {
                    if state.highlighting_enabled {
                        prompt.push_str(&terminal_code(highlight_element_style(state, element)));
                    }
                }
                PromptComponent::ResetColor => {
                    if state.highlighting_enabled {
                        prompt.push_str(reset_terminal_code());
                    }
                }
            }
        }

        prompt
    }

    fn handle_text(&self, state: &ShellState, text: &str, length: &mut usize) -> String {
        let Some(max_length) = self.max_length else {
            return text.to_string();
        };
        if *length > max_length {
            return String::new();
        }
        let render_length = duckdb_render_width::compute_render_width(text.as_bytes());
        if *length + render_length <= max_length {
            *length += render_length;
            return text.to_string();
        }

        let mut truncated = String::new();
        for ch in text.chars() {
            let s = ch.to_string();
            let char_length = duckdb_render_width::compute_render_width(s.as_bytes());
            if *length + char_length > max_length {
                break;
            }
            truncated.push(ch);
            *length += char_length;
        }
        truncated.push_str("... D ");
        *length += 6;
        let _ = state;
        truncated
    }
}

fn query_single_varchar(con: duckdb_sys::duckdb_connection, sql: &str) -> Option<String> {
    let Ok(sql) = CString::new(sql) else {
        return None;
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut result) };
    let cols = unsafe { duckdb_sys::duckdb_column_count(&mut result) };
    if rows == 0 || cols == 0 || unsafe { duckdb_sys::duckdb_value_is_null(&mut result, 0, 0) } {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, 0) };
    if ptr.is_null() {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let value = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
    unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    Some(value)
}

fn execute_sql_single_value(con: duckdb_sys::duckdb_connection, sql: &str) -> String {
    let Ok(sql_c) = CString::new(sql) else {
        return "#ERROR".to_string();
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql_c.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#ERROR".to_string();
    }

    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut result) };
    let cols = unsafe { duckdb_sys::duckdb_column_count(&mut result) };
    if rows == 0 || cols == 0 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#EMPTY#".to_string();
    }
    if rows > 1 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#MULTIPLE_ROWS#".to_string();
    }
    if cols > 1 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#MULTIPLE_COLUMNS#".to_string();
    }
    if unsafe { duckdb_sys::duckdb_value_is_null(&mut result, 0, 0) } {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#NULL#".to_string();
    }
    let ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, 0) };
    if ptr.is_null() {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return "#NULL#".to_string();
    }
    let value = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
    unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    value
}

fn handle_setting(con: duckdb_sys::duckdb_connection, setting: &str) -> String {
    match setting {
        "current_database" => {
            query_single_varchar(con, "select current_database()").unwrap_or_default()
        }
        "current_schema" => {
            query_single_varchar(con, "select current_schema()").unwrap_or_default()
        }
        "current_database_and_schema" => {
            let db = query_single_varchar(con, "select current_database()").unwrap_or_default();
            let schema = query_single_varchar(con, "select current_schema()").unwrap_or_default();
            if schema == "main" || schema.is_empty() {
                db
            } else {
                format!("{}.{}", db, schema)
            }
        }
        "memory_limit" => {
            query_single_varchar(con, "select memory_limit from pragma_database_size()")
                .or_else(|| query_single_varchar(con, "select current_setting('memory_limit')"))
                .unwrap_or_default()
        }
        "memory_usage" => {
            query_single_varchar(con, "select memory_usage from pragma_database_size()")
                .unwrap_or_else(|| "0 bytes".to_string())
        }
        "swap_usage" => "0 bytes".to_string(),
        "swap_max" => "INF".to_string(),
        "bytes_written" => "0 bytes".to_string(),
        "bytes_read" => "0 bytes".to_string(),
        _ => String::new(),
    }
}

fn highlight_element_style(state: &ShellState, element: &str) -> HighlightStyle {
    if let Some(style) = state.highlight_styles.get(element) {
        return *style;
    }
    match element {
        "error" => state.highlight_style_error,
        "keyword" => HighlightStyle {
            color: PrintColor::Green,
            intensity: PrintIntensity::Standard,
        },
        "numeric_constant" | "string_constant" => HighlightStyle {
            color: PrintColor::Yellow,
            intensity: PrintIntensity::Standard,
        },
        "line_indicator" | "table_name" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Bold,
        },
        "database_name" | "suggestion_catalog_name" => HighlightStyle {
            color: PrintColor::Extended(172),
            intensity: PrintIntensity::Standard,
        },
        "schema_name" | "suggestion_schema_name" => HighlightStyle {
            color: PrintColor::Extended(39),
            intensity: PrintIntensity::Standard,
        },
        "column_name" => state.highlight_style_column_name,
        "column_type" => state.highlight_style_column_type,
        "null_value" => state.highlight_style_null_value,
        "footer" | "layout" | "startup_text" | "continuation" | "comment" | "table_layout"
        | "log_warning" => HighlightStyle {
            color: PrintColor::Gray,
            intensity: PrintIntensity::Standard,
        },
        "continuation_selected" | "log_info" => HighlightStyle {
            color: PrintColor::Green,
            intensity: PrintIntensity::Standard,
        },
        "bracket" | "primary_key_column" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Underline,
        },
        "prompt" => HighlightStyle {
            color: PrintColor::Extended(208),
            intensity: PrintIntensity::Bold,
        },
        "error_emphasis" | "error_suggestion" => HighlightStyle {
            color: PrintColor::Red,
            intensity: PrintIntensity::Bold,
        },
        "log_trace" => HighlightStyle {
            color: PrintColor::Blue,
            intensity: PrintIntensity::Bold,
        },
        "log_debug" => HighlightStyle {
            color: PrintColor::Yellow,
            intensity: PrintIntensity::Bold,
        },
        _ => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Standard,
        },
    }
}

fn terminal_code(style: HighlightStyle) -> String {
    let mut out = String::new();
    match style.intensity {
        PrintIntensity::Standard => {}
        PrintIntensity::Bold => out.push_str("\x1b[1m"),
        PrintIntensity::Underline => out.push_str("\x1b[4m"),
        PrintIntensity::BoldUnderline => out.push_str("\x1b[1m\x1b[4m"),
    }
    let code: Option<u8> = match style.color {
        PrintColor::Standard => None,
        PrintColor::Black => Some(0),
        PrintColor::Red => Some(1),
        PrintColor::Green => Some(2),
        PrintColor::Yellow => Some(3),
        PrintColor::Blue => Some(4),
        PrintColor::Magenta => Some(5),
        PrintColor::Cyan => Some(6),
        PrintColor::BrightGray => Some(7),
        PrintColor::Gray => Some(8),
        PrintColor::BrightRed => Some(9),
        PrintColor::BrightGreen => Some(10),
        PrintColor::BrightYellow => Some(11),
        PrintColor::BrightBlue => Some(12),
        PrintColor::BrightMagenta => Some(13),
        PrintColor::BrightCyan => Some(14),
        PrintColor::White => Some(15),
        PrintColor::Extended(code) => Some(code),
    };
    if let Some(code) = code {
        if (1..=7).contains(&code) {
            out.push_str(&format!("\x1b[{}m", 31u16 + (code as u16 - 1)));
        } else if (8..=15).contains(&code) {
            out.push_str(&format!("\x1b[{}m", 90u16 + (code as u16 - 8)));
        } else {
            out.push_str(&format!("\x1b[38;5;{}m", code));
        }
    }
    out
}

fn reset_terminal_code() -> &'static str {
    "\x1b[00m"
}

pub fn validate_prompt_spec(prompt: &str) -> Result<(), String> {
    Prompt::parse(prompt).map(|_| ())
}

pub fn validate_progress_bar_spec(prompt: &str) -> Result<(), String> {
    Prompt::parse_progress_bar(prompt).map(|_| ())
}

pub fn render_main_prompt(state: &ShellState, con: duckdb_sys::duckdb_connection) -> String {
    match Prompt::parse(&state.mainPrompt) {
        Ok(prompt) => prompt.generate(state, con),
        Err(_) => state.mainPrompt.clone(),
    }
}
