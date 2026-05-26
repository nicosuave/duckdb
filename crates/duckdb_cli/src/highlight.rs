use crate::state::{HighlightMode, HighlightStyle, PrintColor, PrintIntensity, ShellState};
use std::ffi::CString;

pub fn apply_mode_styles(state: &mut ShellState, mode: HighlightMode) {
    // Minimal parity: we only control the elements we currently render in Rust (errors/results).
    // The full shell also updates many more highlight elements (prompt, keywords, etc.).
    match mode {
        HighlightMode::Automatic => {}
        HighlightMode::Mixed => {
            state.highlight_style_error = HighlightStyle {
                color: PrintColor::Red,
                intensity: PrintIntensity::Standard,
            };
            state.highlight_style_column_name = HighlightStyle {
                color: PrintColor::Standard,
                intensity: PrintIntensity::Standard,
            };
            state.highlight_style_column_type = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
            state.highlight_style_null_value = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
        }
        HighlightMode::Dark => {
            // Keep null/type gray; emphasize errors slightly more.
            state.highlight_style_error = HighlightStyle {
                color: PrintColor::Red,
                intensity: PrintIntensity::Bold,
            };
            state.highlight_style_column_type = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
            state.highlight_style_null_value = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
        }
        HighlightMode::Light => {
            // Similar to mixed; keep it readable on light backgrounds.
            state.highlight_style_error = HighlightStyle {
                color: PrintColor::Red,
                intensity: PrintIntensity::Bold,
            };
            state.highlight_style_column_type = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
            state.highlight_style_null_value = HighlightStyle {
                color: PrintColor::Gray,
                intensity: PrintIntensity::Standard,
            };
        }
    }
}

pub fn detect_dark_light_mode(state: &mut ShellState) {
    if state.highlight_mode != HighlightMode::Automatic {
        return;
    }
    if !state.stdout_is_console {
        return;
    }
    if !state.stdin_is_interactive {
        return;
    }
    // Best-effort: rely on our bundled C++ helper (does nothing when not on a tty).
    let mode = unsafe { duckdb_linenoise::duckdb_cli_get_terminal_color_mode() };
    state.highlight_mode = match mode {
        1 => HighlightMode::Dark,
        2 => HighlightMode::Light,
        3 => HighlightMode::Mixed,
        _ => HighlightMode::Mixed,
    };
    apply_mode_styles(state, state.highlight_mode);
    sync_linenoise_highlight_mode(state.highlight_mode);
}

pub fn sync_linenoise_highlighting_enabled(enabled: bool) {
    unsafe {
        duckdb_linenoise::duckdb_cli_linenoise_set_highlighting(if enabled { 1 } else { 0 });
    }
}

pub fn sync_linenoise_highlight_style(element: &str, style: HighlightStyle) -> bool {
    let Ok(element_c) = CString::new(element) else {
        return false;
    };
    unsafe {
        duckdb_linenoise::duckdb_cli_linenoise_set_highlight_color(
            element_c.as_ptr(),
            color_code(style.color),
            intensity_code(style.intensity),
            1,
        ) != 0
    }
}

pub fn sync_linenoise_highlight_mode(mode: HighlightMode) {
    let code = match mode {
        HighlightMode::Mixed => 1,
        HighlightMode::Dark => 2,
        HighlightMode::Light => 3,
        HighlightMode::Automatic => return,
    };
    unsafe {
        duckdb_linenoise::duckdb_cli_linenoise_apply_highlight_mode(code);
    }
}

fn color_code(color: PrintColor) -> u16 {
    match color {
        PrintColor::Standard => 256,
        PrintColor::Black => 0,
        PrintColor::Red => 1,
        PrintColor::Green => 2,
        PrintColor::Yellow => 3,
        PrintColor::Blue => 4,
        PrintColor::Magenta => 5,
        PrintColor::Cyan => 6,
        PrintColor::BrightGray => 7,
        PrintColor::Gray => 8,
        PrintColor::BrightRed => 9,
        PrintColor::BrightGreen => 10,
        PrintColor::BrightYellow => 11,
        PrintColor::BrightBlue => 12,
        PrintColor::BrightMagenta => 13,
        PrintColor::BrightCyan => 14,
        PrintColor::White => 15,
        PrintColor::Extended(code) => code as u16,
    }
}

fn intensity_code(intensity: PrintIntensity) -> i32 {
    match intensity {
        PrintIntensity::Standard => 0,
        PrintIntensity::Bold => 1,
        PrintIntensity::Underline => 2,
        PrintIntensity::BoldUnderline => 3,
    }
}
