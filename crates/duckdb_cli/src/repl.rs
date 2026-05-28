use crate::exec;
use crate::history;
use crate::session::Session;
use crate::state::{InputMode, ReadLineVersion, ShellState, StartupText};
use std::env;
use std::ffi::{CStr, CString};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct StdinTermios {
    fd: i32,
    baseline: Option<libc::termios>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl StdinTermios {
    fn new() -> Self {
        let fd = libc::STDIN_FILENO;
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::tcgetattr(fd, &mut t) };
        let mut baseline = if rc == 0 { Some(t) } else { None };
        if let Some(t) = baseline.as_mut() {
            // Avoid Ctrl-R being swallowed by the terminal line discipline (VREPRINT) when we
            // briefly restore baseline termios around linenoise calls.
            //
            // This is important for PTY-driven tests that send Ctrl-R immediately after a query
            // finishes and before linenoise re-enters raw mode.
            #[allow(clippy::indexing_slicing)]
            {
                t.c_cc[libc::VREPRINT] = 0;
            }
        }
        Self { fd, baseline }
    }

    fn restore_baseline(&self) {
        let Some(t) = self.baseline else {
            return;
        };
        let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &t) };
    }

    fn set_between_commands(&self) {
        let Some(mut t) = self.baseline else {
            return;
        };
        // Keep signals enabled (Ctrl-C -> SIGINT), but avoid canonical line buffering and echo.
        t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &t) };
    }
}

fn print_stdout(msg: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(msg.as_bytes());
    let _ = stdout.flush();
}

fn print_stderr(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(msg.as_bytes());
}

fn is_edit_command_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == ".edit" || trimmed == "\\e"
}

fn shell_quote_single(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn edit_buffer_with_external_editor(initial_sql: &str) -> Result<String, String> {
    let editor = env::var("DUCKDB_EDITOR")
        .ok()
        .or_else(|| env::var("EDITOR").ok())
        .or_else(|| env::var("VISUAL").ok())
        .unwrap_or_else(|| "vi".to_string());

    let mut file = None;
    let mut path = PathBuf::new();
    for attempt in 0..100 {
        let mut candidate = env::temp_dir();
        candidate.push(format!(
            "duckdb.edit.{}.{}.sql",
            std::process::id(),
            attempt
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => {
                file = Some(f);
                path = candidate;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "could not open temporary file \"{}\": {}",
                    candidate.display(),
                    e
                ));
            }
        }
    }
    let Some(mut file) = file else {
        return Err("could not create temporary edit file".to_string());
    };
    if let Err(e) = file.write_all(initial_sql.as_bytes()) {
        let _ = fs::remove_file(&path);
        return Err(format!("failed to write data {}: {}", path.display(), e));
    }
    drop(file);

    #[cfg(not(target_os = "windows"))]
    let status = if editor.chars().any(|ch| ch.is_whitespace()) {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "exec {} {}",
                editor,
                shell_quote_single(path.to_string_lossy().as_ref())
            ))
            .status()
    } else {
        Command::new(&editor).arg(&path).status()
    };

    #[cfg(target_os = "windows")]
    let status = Command::new(&editor).arg(&path).status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let _ = fs::remove_file(&path);
            return Err(format!(
                "could not start editor \"{}\": exit status {}",
                editor, status
            ));
        }
        Err(e) => {
            let _ = fs::remove_file(&path);
            return Err(format!("could not start editor \"{}\": {}", editor, e));
        }
    }

    let edited = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_file(&path);
            return Err(format!("failed to open file {}: {}", path.display(), e));
        }
    };
    let _ = fs::remove_file(&path);
    Ok(edited)
}

fn line_contains_semicolon(bytes: &[u8]) -> bool {
    bytes.iter().any(|b| *b == b';')
}

fn sql_is_complete(sql: &str) -> bool {
    // Port of `ShellState::SQLIsComplete(const char *zSql)` from `tools/shell/shell.cpp`.
    //
    // We intentionally do not use sqlite3_complete() here. DuckDB's shell behavior differs
    // (notably: dollar-quoted strings).
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum SqlParseState {
        Semicolon,
        Whitespace,
        Normal,
    }

    fn skip_dollar_quoted_string(sql: &[u8], mut i: usize, delimiter: &[u8]) -> Option<usize> {
        while i < sql.len() {
            if sql[i] != b'$' {
                i += 1;
                continue;
            }
            // Found a dollar.
            i += 1;
            let start = i;
            while i < sql.len() && sql[i] != b'$' {
                i += 1;
            }
            if i >= sql.len() {
                return None;
            }
            // Check delimiter match.
            if i - start == delimiter.len() && sql[start..i] == *delimiter {
                return Some(i);
            }
            // Dollar does not match: reset position to start and keep looking.
            i = start;
        }
        None
    }

    let bytes = sql.as_bytes();
    let mut state = SqlParseState::Normal;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let next_state = match b {
            b';' => SqlParseState::Semicolon,
            b' ' | b'\r' | b'\t' | b'\n' | b'\x0c' => SqlParseState::Whitespace,
            b'/' => {
                // C-style comment.
                if i + 1 >= bytes.len() || bytes[i + 1] != b'*' {
                    SqlParseState::Normal
                } else {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    if i + 1 >= bytes.len() {
                        return false;
                    }
                    i += 1; // positioned on '*', outer loop increments once more
                    SqlParseState::Whitespace
                }
            }
            b'-' => {
                // SQL-style comment.
                if i + 1 >= bytes.len() || bytes[i + 1] != b'-' {
                    SqlParseState::Normal
                } else {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return state == SqlParseState::Semicolon;
                    }
                    SqlParseState::Whitespace
                }
            }
            b'$' => {
                // Dollar-quoted strings.
                let mut next_dollar = 0usize;
                let mut idx = 1usize;
                while i + idx < bytes.len() {
                    let c = bytes[i + idx];
                    if c == b'$' {
                        next_dollar = idx;
                        break;
                    }
                    let is_valid = matches!(c, b'A'..=b'Z')
                        || matches!(c, b'a'..=b'z')
                        || c == b'_'
                        || (c >= 0x80)
                        || (idx > 1 && matches!(c, b'0'..=b'9'));
                    if !is_valid {
                        break;
                    }
                    idx += 1;
                }
                if next_dollar == 0 {
                    SqlParseState::Normal
                } else {
                    let delimiter_start = i + 1;
                    let delimiter_end = i + next_dollar;
                    let delimiter = &bytes[delimiter_start..delimiter_end];
                    i = delimiter_end + 1;
                    let Some(end_dollar) = skip_dollar_quoted_string(bytes, i, delimiter) else {
                        return false;
                    };
                    i = end_dollar; // outer loop increments once more
                    SqlParseState::Whitespace
                }
            }
            b'"' | b'\'' => {
                let quote = b;
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                if i >= bytes.len() {
                    return false;
                }
                SqlParseState::Whitespace
            }
            _ => SqlParseState::Normal,
        };
        if next_state != SqlParseState::Whitespace {
            state = next_state;
        }
        i += 1;
    }
    state == SqlParseState::Semicolon
}

fn is_space(byte: u8) -> bool {
    (byte as char).is_whitespace()
}

fn all_whitespace_sqlite_style(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_space(b) {
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return false;
            }
            i += 2;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if i >= bytes.len() {
                return true;
            }
            continue;
        }
        return false;
    }
    true
}

static LINENOISE_INSTALLED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn linenoise_completion(
    z_line: *const c_char,
    lc: *mut duckdb_linenoise::linenoiseCompletions,
) {
    if z_line.is_null() || lc.is_null() {
        return;
    }
    let z_line = unsafe { CStr::from_ptr(z_line) };
    let line = z_line.to_string_lossy();
    let bytes = line.as_bytes();
    if bytes.len() > 1000usize.saturating_sub(30) {
        return;
    }

    if bytes.first().copied() == Some(b'.') {
        for &cmd in crate::dotcmd::METADATA_COMMAND_NAMES {
            let candidate = format!(".{}", cmd);
            if candidate.len() < line.len() {
                continue;
            }
            if !candidate.as_bytes().starts_with(bytes) {
                continue;
            }
            let Ok(c) = CString::new(candidate) else {
                continue;
            };
            let Ok(completion_type) = CString::new("keyword") else {
                continue;
            };
            unsafe {
                duckdb_linenoise::linenoiseAddCompletion(
                    lc,
                    z_line.as_ptr(),
                    c.as_ptr(),
                    c.as_bytes().len(),
                    0,
                    completion_type.as_ptr(),
                    0,
                    0,
                )
            }
        }
        return;
    }
    if bytes.first().copied() == Some(b'#') {
        return;
    }

    let Some(con) = crate::completion::current_connection() else {
        return;
    };

    fn quote_sql_string(z: &str) -> String {
        let mut out = String::with_capacity(z.len() + 2);
        out.push('\'');
        for ch in z.chars() {
            if ch == '\'' {
                out.push_str("''");
            } else {
                out.push(ch);
            }
        }
        out.push('\'');
        out
    }

    let sql = format!("CALL sql_auto_complete({})", quote_sql_string(&line));
    let Ok(sql_c) = CString::new(sql) else {
        return;
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql_c.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return;
    }

    let row_count = unsafe { duckdb_sys::duckdb_row_count(&mut result) } as usize;
    for r in 0..row_count {
        unsafe fn value_varchar(
            result: &mut duckdb_sys::duckdb_result,
            col: usize,
            row: usize,
        ) -> Option<String> {
            if duckdb_sys::duckdb_value_is_null(result, col as u64, row as u64) {
                return None;
            }
            let ptr = duckdb_sys::duckdb_value_varchar(result, col as u64, row as u64);
            if ptr.is_null() {
                return None;
            }
            let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
            duckdb_sys::duckdb_free(ptr as *mut c_void);
            Some(s)
        }

        let Some(completion) = (unsafe { value_varchar(&mut result, 0, r) }) else {
            continue;
        };
        let Some(start_str) = (unsafe { value_varchar(&mut result, 1, r) }) else {
            continue;
        };
        let Ok(i_start) = start_str.trim().parse::<usize>() else {
            continue;
        };
        if i_start > bytes.len() {
            continue;
        }
        if bytes[..i_start].len().saturating_add(completion.len()) >= 1000 {
            continue;
        }
        let completion_type =
            unsafe { value_varchar(&mut result, 2, r) }.unwrap_or_else(|| "unknown".to_string());
        let score = unsafe { value_varchar(&mut result, 3, r) }
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let extra_char = unsafe { value_varchar(&mut result, 4, r) }
            .and_then(|v| {
                let bytes = v.as_bytes();
                if bytes.len() == 1 {
                    Some(bytes[0] as c_char)
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let Ok(completion_c) = CString::new(completion) else {
            continue;
        };
        let Ok(completion_type_c) = CString::new(completion_type) else {
            continue;
        };
        unsafe {
            duckdb_linenoise::linenoiseAddCompletion(
                lc,
                z_line.as_ptr(),
                completion_c.as_ptr(),
                completion_c.as_bytes().len(),
                i_start,
                completion_type_c.as_ptr(),
                score,
                extra_char,
            )
        };
    }

    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
}

pub fn ensure_linenoise_installed(state: &mut ShellState) {
    if state.rl_version != ReadLineVersion::Linenoise {
        return;
    }

    // Keep the continuation prompts in sync.
    if let (Ok(cont), Ok(cont_sel)) = (
        CString::new(state.continuePrompt.as_str()),
        CString::new(state.continuePromptSelected.as_str()),
    ) {
        let Ok(scroll_up) = CString::new("⇡ ") else {
            return;
        };
        let Ok(scroll_down) = CString::new("⇣ ") else {
            return;
        };
        unsafe {
            duckdb_linenoise::linenoiseSetPrompt(
                cont.as_ptr(),
                cont_sel.as_ptr(),
                scroll_up.as_ptr(),
                scroll_down.as_ptr(),
            )
        };
    }

    if LINENOISE_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    unsafe { duckdb_linenoise::linenoiseSetCompletionCallback(Some(linenoise_completion)) };
    let _ =
        unsafe { duckdb_linenoise::linenoiseHistorySetMaxLen(history::MAX_HISTORY_LINES as i32) };
    if state.history_path.is_none() {
        state.history_path = history::history_path();
    }
    if let Some(path) = state.history_path.as_deref() {
        if let Ok(c_path) = CString::new(path) {
            let _ = unsafe { duckdb_linenoise::linenoiseHistoryLoad(c_path.as_ptr()) };
        }
    }
}

pub fn process_stdin_interactive(state: &mut ShellState, session: &mut Session) -> i32 {
    let mut err_cnt: i32 = 0;
    let mut exit_code: Option<i32> = None;
    let mut sql_buf = String::new();
    let mut last_editable_sql = String::new();
    let mut ctrl_c_count: u32 = 0;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let stdin_termios = StdinTermios::new();

    let stdin = std::io::stdin();
    let mut stdin_lock = stdin.lock();

    loop {
        let is_continuation = !sql_buf.is_empty();

        let line: Option<String> = if state.rl_version == ReadLineVersion::Linenoise {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            stdin_termios.restore_baseline();

            ensure_linenoise_installed(state);
            let prompt = if is_continuation {
                state.continuePrompt.clone()
            } else {
                crate::prompt::render_main_prompt(state, session.con)
            };
            let Ok(prompt_c) = CString::new(prompt) else {
                break;
            };
            let ptr = unsafe { duckdb_linenoise::linenoise(prompt_c.as_ptr()) };
            if ptr.is_null() {
                print_stdout("\n");
                break;
            }
            let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
            unsafe { duckdb_linenoise::linenoiseFree(ptr as *mut c_void) };

            #[cfg(any(target_os = "macos", target_os = "linux"))]
            stdin_termios.set_between_commands();

            Some(s)
        } else {
            let prompt = if is_continuation {
                state.continuePrompt.clone()
            } else {
                crate::prompt::render_main_prompt(state, session.con)
            };
            print_stdout(&prompt);
            let _ = std::io::stdout().flush();
            let mut buf = String::new();
            match stdin_lock.read_line(&mut buf) {
                Ok(0) => {
                    print_stdout("\n");
                    break;
                }
                Ok(_) => {
                    while buf.ends_with('\n') || buf.ends_with('\r') {
                        buf.pop();
                    }
                    Some(buf)
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    crate::signals::clear_interrupt();
                    Some("\u{3}".to_string())
                }
                Err(_) => break,
            }
        };

        let Some(line) = line else {
            break;
        };

        if crate::signals::has_seen_interrupt() {
            crate::signals::clear_interrupt();
        }

        if line.as_bytes().first().copied() == Some(3) {
            if sql_buf.is_empty() && line.as_bytes().len() == 1 {
                ctrl_c_count += 1;
                if ctrl_c_count >= 2 {
                    print_stdout("Interrupted, use Ctrl+D to exit\n");
                }
            }
            sql_buf.clear();
            continue;
        }
        ctrl_c_count = 0;

        if sql_buf.is_empty() && all_whitespace_sqlite_style(&line) {
            if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                print_stdout(&line);
                print_stdout("\n");
            }
            continue;
        }

        if is_edit_command_line(&line) {
            if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                print_stdout(&line);
                print_stdout("\n");
            }
            let initial_sql = if sql_buf.is_empty() {
                last_editable_sql.as_str()
            } else {
                sql_buf.as_str()
            };
            match edit_buffer_with_external_editor(initial_sql) {
                Ok(edited) => {
                    if edited.trim().is_empty() {
                        sql_buf.clear();
                        continue;
                    }
                    if line_contains_semicolon(edited.as_bytes()) && sql_is_complete(&edited) {
                        if state.rl_version == ReadLineVersion::Linenoise {
                            if let Ok(c) = CString::new(edited.as_str()) {
                                let _ =
                                    unsafe { duckdb_linenoise::linenoiseHistoryAdd(c.as_ptr()) };
                            }
                        }
                        let executed_sql = edited.trim_end_matches('\n').to_string();
                        let rc = run_sql_buffer(state, session, &edited, InputMode::Standard);
                        last_editable_sql = executed_sql;
                        if rc != 0 {
                            err_cnt += 1;
                        }
                        sql_buf.clear();
                    } else {
                        sql_buf.clear();
                        sql_buf.push_str(edited.trim_start_matches(|c: char| c.is_whitespace()));
                    }
                }
                Err(e) => {
                    print_stderr(&format!("{}\n", e));
                    err_cnt += 1;
                    sql_buf.clear();
                }
            }
            continue;
        }

        if sql_buf.is_empty() && (line.starts_with('.') || line.starts_with('#')) {
            if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                print_stdout(&line);
                print_stdout("\n");
            }
            if line.starts_with('.') {
                if state.rl_version == ReadLineVersion::Linenoise
                    && !line.is_empty()
                    && line.as_bytes()[0] != 3
                {
                    if let Ok(c) = CString::new(line.as_str()) {
                        let _ = unsafe { duckdb_linenoise::linenoiseHistoryAdd(c.as_ptr()) };
                    }
                }
                let rc = exec::run_command(state, session, line.as_str());
                if rc == 2 {
                    exit_code = Some(state.exit_code.unwrap_or(0));
                    break;
                }
                if rc != 0 {
                    err_cnt += 1;
                }
            }
            continue;
        }

        if sql_buf.is_empty() {
            let trimmed = line.trim_start_matches(|c: char| c.is_whitespace());
            sql_buf.push_str(trimmed);
        } else {
            sql_buf.push('\n');
            sql_buf.push_str(&line);
        }

        if !sql_buf.is_empty() {
            if line_contains_semicolon(sql_buf.as_bytes()) && sql_is_complete(&sql_buf) {
                if state.rl_version == ReadLineVersion::Linenoise
                    && !sql_buf.is_empty()
                    && sql_buf.as_bytes()[0] != 3
                {
                    if let Ok(c) = CString::new(sql_buf.as_str()) {
                        let _ = unsafe { duckdb_linenoise::linenoiseHistoryAdd(c.as_ptr()) };
                    }
                }
                let rc = run_sql_buffer(state, session, &sql_buf, InputMode::Standard);
                if rc != 0 {
                    err_cnt += 1;
                }
                last_editable_sql = sql_buf.trim_end_matches('\n').to_string();
                sql_buf.clear();
            } else if all_whitespace_sqlite_style(&sql_buf) {
                if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                    print_stdout(&sql_buf);
                    print_stdout("\n");
                }
                sql_buf.clear();
            }
        }
    }

    if !sql_buf.is_empty() && !all_whitespace_sqlite_style(&sql_buf) {
        if state.rl_version == ReadLineVersion::Linenoise
            && !sql_buf.is_empty()
            && sql_buf.as_bytes()[0] != 3
        {
            if let Ok(c) = CString::new(sql_buf.as_str()) {
                let _ = unsafe { duckdb_linenoise::linenoiseHistoryAdd(c.as_ptr()) };
            }
        }
        let rc = run_sql_buffer(state, session, &sql_buf, InputMode::Standard);
        if rc != 0 {
            err_cnt += 1;
        }
    }

    if LINENOISE_INSTALLED.load(Ordering::SeqCst) {
        if state.history_path.is_none() {
            state.history_path = history::history_path();
        }
        if let Some(path) = state.history_path.as_deref() {
            let _ = unsafe {
                duckdb_linenoise::linenoiseHistorySetMaxLen(history::MAX_HISTORY_LINES as i32)
            };
            if let Ok(c_path) = CString::new(path) {
                let _ = unsafe { duckdb_linenoise::linenoiseHistorySave(c_path.as_ptr()) };
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    stdin_termios.restore_baseline();

    if let Some(code) = exit_code {
        code
    } else if err_cnt > 0 {
        1
    } else {
        0
    }
}

fn run_sql_buffer(
    state: &mut ShellState,
    session: &mut Session,
    sql: &str,
    _mode: InputMode,
) -> i32 {
    let sql = sql.trim_end_matches('\n');

    let rc = exec::run_command(state, session, sql);
    if sql.to_ascii_lowercase().contains("enable_external_access") {
        crate::shell_ext::sync_external_access(session.con);
    }
    if rc == 0 && (state.shellFlgs & (crate::state::ShellFlags::SHFLG_CountChanges as u32)) != 0 {
        print_stdout(&format!(
            "changes: {:3}   total_changes: {}\n",
            state.last_changes, state.total_changes
        ));
    }
    rc
}

pub fn process_reader<R: BufRead>(
    state: &mut ShellState,
    session: &mut Session,
    mut reader: R,
    mode: InputMode,
    interactive: bool,
) -> i32 {
    let mut err_cnt: i32 = 0;
    let mut sql_buf = String::new();
    let mut ctrl_c_count: u32 = 0;
    let mut mode = mode;

    loop {
        if err_cnt != 0
            && state.get_bail_on_error(mode)
            && !(interactive && mode == InputMode::Standard)
        {
            break;
        }

        if interactive {
            if sql_buf.is_empty() {
                print_stdout(&crate::prompt::render_main_prompt(state, session.con));
            } else {
                print_stdout("· ");
            }
        }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                if interactive {
                    print_stdout("\n");
                }
                break;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                // Ctrl-C
                crate::signals::clear_interrupt();
                if interactive && sql_buf.is_empty() {
                    ctrl_c_count += 1;
                    if ctrl_c_count >= 2 {
                        print_stdout("Interrupted, use Ctrl+D to exit\n");
                    }
                } else if !interactive {
                    break;
                }
                sql_buf.clear();
                continue;
            }
            Err(_) => break,
        }

        // Match shell.cpp local_getline: strip trailing newline (and optional \r).
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }

        // If we are receiving input after a query was interrupted, we need to clear the interrupt flag
        // to be able to print messages again. When reading from a file (rc or -init), we stop processing.
        if crate::signals::has_seen_interrupt() {
            if mode != InputMode::Standard {
                break;
            }
            crate::signals::clear_interrupt();
        }

        if line.as_bytes().first().copied() == Some(3) {
            // Ctrl-C line (linenoise-like behavior).
            if sql_buf.is_empty() && interactive {
                ctrl_c_count += 1;
                if ctrl_c_count >= 2 {
                    print_stdout("Interrupted, use Ctrl+D to exit\n");
                }
            }
            sql_buf.clear();
            continue;
        }
        ctrl_c_count = 0;

        if mode == InputMode::DuckDbRc && !line.starts_with(".startup_text") {
            if state.startup_text == StartupText::All && !state.displayed_loading_resources_message
            {
                if let Some(path) = state.duckdb_rc_path.as_deref() {
                    print_stderr(&format!("-- Loading resources from {}\n", path));
                    state.displayed_loading_resources_message = true;
                }
            }
            mode = InputMode::File;
        }

        if sql_buf.is_empty() && all_whitespace_sqlite_style(&line) {
            if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                print_stdout(&line);
                print_stdout("\n");
            }
            continue;
        }

        if sql_buf.is_empty() && (line.starts_with('.') || line.starts_with('#')) {
            if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                print_stdout(&line);
                print_stdout("\n");
            }
            if line.starts_with('.') {
                let rc = exec::run_command(state, session, line.as_str());
                if rc == 2 {
                    return state.exit_code.unwrap_or(0);
                }
                if rc != 0 {
                    err_cnt += 1;
                }
            }
            continue;
        }

        if sql_buf.is_empty() {
            let trimmed = line.trim_start_matches(|c: char| c.is_whitespace());
            sql_buf.push_str(trimmed);
        } else {
            sql_buf.push('\n');
            sql_buf.push_str(&line);
        }

        if !sql_buf.is_empty() {
            if line_contains_semicolon(sql_buf.as_bytes()) && sql_is_complete(&sql_buf) {
                let rc = run_sql_buffer(state, session, &sql_buf, mode);
                if rc != 0 {
                    err_cnt += 1;
                }
                sql_buf.clear();
            } else if all_whitespace_sqlite_style(&sql_buf) {
                if (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0 {
                    print_stdout(&sql_buf);
                    print_stdout("\n");
                }
                sql_buf.clear();
            }
        }
    }

    if !sql_buf.is_empty() && !all_whitespace_sqlite_style(&sql_buf) {
        let rc = run_sql_buffer(state, session, &sql_buf, mode);
        if rc != 0 {
            err_cnt += 1;
        }
    }

    if err_cnt > 0 {
        1
    } else {
        0
    }
}

pub fn run_interactive_banner(state: &ShellState, con: duckdb_sys::duckdb_connection) {
    if state.startup_text == StartupText::None {
        return;
    }

    let info = crate::db::query_version_info(con);
    let (version, codename, source_id) = if let Some(info) = info {
        (info.library_version, info.codename, info.source_id)
    } else {
        let version_ptr = unsafe { duckdb_sys::duckdb_library_version() };
        let version = if version_ptr.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(version_ptr) }
                .to_string_lossy()
                .to_string()
        };
        (version, String::new(), String::new())
    };

    if codename.is_empty() || source_id.is_empty() {
        print_stdout(&format!("DuckDB {}\n", version.trim()));
    } else {
        print_stdout(&format!("DuckDB {} ({})\n", version.trim(), codename));
    }
    if state.startup_text == StartupText::All {
        if state.highlighting_enabled && state.stdout_is_console {
            print_stdout("\x1b[90m");
        }
        print_stdout("Enter \".help\" for usage hints.\n");
        if state.highlighting_enabled && state.stdout_is_console {
            print_stdout("\x1b[00m");
        }
    }
}

pub fn validate_prompt_spec(prompt: &str) -> Result<(), String> {
    crate::prompt::validate_prompt_spec(prompt)
}
