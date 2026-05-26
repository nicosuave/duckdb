#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_void};

pub type linenoiseCompletions = *mut c_void;

pub type linenoiseCompletionCallback =
    unsafe extern "C" fn(*const c_char, *mut linenoiseCompletions);

extern "C" {
    pub fn linenoise(prompt: *const c_char) -> *mut c_char;
    pub fn linenoiseFree(ptr: *mut c_void);

    pub fn linenoiseHistoryAdd(line: *const c_char) -> c_int;
    pub fn linenoiseHistorySetMaxLen(len: c_int) -> c_int;
    pub fn linenoiseHistorySave(filename: *const c_char) -> c_int;
    pub fn linenoiseHistoryLoad(filename: *const c_char) -> c_int;

    pub fn linenoiseSetCompletionCallback(cb: Option<linenoiseCompletionCallback>);
    pub fn linenoiseAddCompletion(
        lc: *mut linenoiseCompletions,
        line: *const c_char,
        completion: *const c_char,
        n_completion: usize,
        completion_start: usize,
        completion_type: *const c_char,
        score: usize,
        extra_char: c_char,
    );

    pub fn linenoiseSetMultiLine(ml: c_int);
    pub fn linenoiseSetPrompt(
        continuation: *const c_char,
        continuation_selected: *const c_char,
        scroll_up: *const c_char,
        scroll_down: *const c_char,
    );

    pub fn linenoiseSetCompletionRendering(enabled: c_int);
    pub fn linenoiseSetErrorRendering(enabled: c_int);

    pub fn linenoiseComputeRenderWidth(buf: *const c_char, len: usize) -> usize;
    pub fn linenoiseGetRenderPosition(
        buf: *const c_char,
        len: usize,
        max_width: c_int,
        n: *mut c_int,
    ) -> c_int;

    // DuckDB sqlite shell wrapper: our local implementation compiled into the static lib.
    pub fn duckdb_shell_sqlite3_complete(sql: *const c_char) -> c_int;

    // DuckDB CLI helper: best-effort terminal background color detection.
    // Returns 0=unknown, 1=dark, 2=light, 3=mixed.
    pub fn duckdb_cli_get_terminal_color_mode() -> c_int;

    pub fn duckdb_cli_linenoise_set_highlighting(enabled: c_int);
    pub fn duckdb_cli_linenoise_set_highlight_color(
        element: *const c_char,
        color: u16,
        intensity: c_int,
        user_configured: c_int,
    ) -> c_int;
    pub fn duckdb_cli_linenoise_apply_highlight_mode(mode: c_int);
}
