use crate::state::{HighlightMode, HighlightStyle, PrintColor, PrintIntensity, ShellState};

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
}
