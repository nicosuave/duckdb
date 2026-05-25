mod candidates;
mod completion;
mod db;
mod display_colors;
mod dotcmd;
mod duckbox_json_formatter;
mod exec;
mod highlight;
mod history;
mod options;
mod output;
mod prompt;
mod repl;
mod session;
mod shell_ext;
mod signals;
mod sql_split;
mod sqlite_shell;
mod state;
mod value;

use crate::options::CommandLineOption;
use crate::session::Session;
use crate::state::{
    BailOnError, InitialAction, InputMode, MetadataResult, ShellState, StartupText,
};
use std::ffi::{CStr, CString};
use std::io::Write;

struct CommandLineCall {
    option_index: usize,
    arguments: Vec<String>,
}

fn print_database_error(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(msg.as_bytes());
    if !msg.ends_with('\n') {
        let _ = stderr.write_all(b"\n");
    }
}

fn print_stderr(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(msg.as_bytes());
}

fn isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) == 1 }
}

fn get_home_directory() -> Option<String> {
    std::env::var("HOME").ok().filter(|s| !s.is_empty())
}

fn get_default_duckdbrc() -> Option<String> {
    get_home_directory().map(|home| format!("{}/.duckdbrc", home))
}

#[allow(dead_code)]
fn set_startup_text(state: &mut ShellState, value: &str) -> bool {
    let prev = state.startup_text;
    let next = match value {
        "all" => StartupText::All,
        "version" => StartupText::Version,
        "none" => StartupText::None,
        _ => return false,
    };
    state.startup_text = next;
    if state.displayed_loading_resources_message
        && prev == StartupText::All
        && next != StartupText::All
    {
        print_stderr(
			"WARNING: .startup_text should be on top of your ~/.duckdbrc in order to prevent the \"Loading resources\" message from being displayed\n",
		);
    }
    true
}

fn process_duckdbrc(
    state: &mut ShellState,
    session: &mut Session,
    file_override: Option<&str>,
) -> bool {
    let default_duckdb_rc = file_override.is_none();
    let path = if let Some(file_override) = file_override {
        file_override.to_string()
    } else {
        let Some(path) = get_default_duckdbrc() else {
            print_stderr("-- warning: cannot find home directory; cannot read ~/.duckdbrc\n");
            return true;
        };
        path
    };

    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => {
            if default_duckdb_rc {
                return true;
            }
            print_database_error(&format!("IO Error: Failed to open file \"{}\"", path));
            state.duckdb_rc_path = Some(path);
            return false;
        }
    };

    state.duckdb_rc_path = Some(path.clone());

    let reader = std::io::BufReader::new(file);
    let mode = if state.stdin_is_interactive {
        InputMode::DuckDbRc
    } else {
        InputMode::File
    };
    let rc = repl::process_reader(state, session, reader, mode, false);
    rc == 0
}

fn parse_args(
    state: &mut ShellState,
    argv: &[String],
) -> Result<(Vec<CommandLineCall>, Vec<String>), i32> {
    let mut extra_commands: Vec<String> = Vec::new();
    let mut command_line_calls: Vec<CommandLineCall> = Vec::new();

    let argc = argv.len();
    let mut i = 1usize;
    while i < argc {
        let arg = &argv[i];
        if !arg.starts_with('-') || arg == "-" {
            if state.zDbFilename.is_empty() {
                state.zDbFilename = arg.clone();
            } else {
                state.readStdin = false;
                state.stdin_is_interactive = false;
                extra_commands.push(arg.clone());
            }
            i += 1;
            continue;
        }

        let mut z = arg.as_str();
        z = &z[1..];
        if z.starts_with('-') {
            z = &z[1..];
        }

        let option = match options::find_command_line_option(z, &state.program_name) {
            Ok(opt) => opt,
            Err(err) => {
                print_database_error(&err);
                return Err(1);
            }
        };

        let mut arguments: Vec<String> = Vec::with_capacity(1 + option.argument_count);
        arguments.push(option.option.to_string());
        for arg_idx in 0..option.argument_count {
            if i + 1 >= argc {
                let mut error = format!(
                    "Missing Argument Error: Argument '-{}' needs {} arguments, but got {}\n",
                    option.option, option.argument_count, arg_idx
                );
                error.push_str(&format!(
                    "OPTION:\n  -{} {}    {}\n\n",
                    option.option, option.arguments, option.description
                ));
                error.push_str(&format!(
                    "Run '{} -help' for a list of options.\n",
                    state.program_name
                ));
                print_database_error(&error);
                return Err(1);
            }
            i += 1;
            arguments.push(argv[i].clone());
        }

        if let Some(callback) = option.pre_init_callback {
            let result = callback(state, &arguments);
            if result == MetadataResult::Exit {
                return Err(0);
            }
        }

        let option_index = options::COMMAND_LINE_OPTIONS
            .iter()
            .position(|o| std::ptr::eq(o, option))
            .expect("option not found in COMMAND_LINE_OPTIONS");
        command_line_calls.push(CommandLineCall {
            option_index,
            arguments,
        });

        i += 1;
    }

    Ok((command_line_calls, extra_commands))
}

fn set_safe_mode(con: duckdb_sys::duckdb_connection) -> Result<(), i32> {
    let query = CString::new("SET enable_external_access=false").map_err(|_| 1)?;
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, query.as_ptr(), &mut result) };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        print_database_error("Failed to set enable_external_access=false for safe mode");
        return Err(1);
    }
    Ok(())
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let program_name = argv.get(0).cloned().unwrap_or_else(|| "duckdb".to_string());

    let skip_version_check = std::env::var("DUCKDB_SKIP_LIB_VERSION_CHECK")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"));
    let version = unsafe { duckdb_sys::duckdb_library_version() };
    if version.is_null() {
        print_database_error("duckdb_library_version returned null");
        std::process::exit(1);
    }
    let version = unsafe { CStr::from_ptr(version) };
    let version_str = version.to_string_lossy();
    let normalized = version_str
        .trim()
        .trim_start_matches('v')
        .split(|c: char| c.is_whitespace() || c == '-' || c == '+')
        .next()
        .unwrap_or("");
    if !skip_version_check && normalized != duckdb_sys::DUCKDB_TARGET_VERSION {
        print_database_error(&format!(
            "DuckDB library version mismatch: expected {}, got {}",
            duckdb_sys::DUCKDB_TARGET_VERSION,
            version_str.trim()
        ));
        std::process::exit(1);
    }

    let mut state = ShellState::new(program_name.clone());
    state.stdin_is_interactive = isatty(0);
    state.stdout_is_console = isatty(1);
    state.stderr_is_console = isatty(2);

    signals::install(state.stdin_is_interactive);

    let (command_line_calls, extra_commands) = match parse_args(&mut state, &argv) {
        Ok(v) => v,
        Err(code) => std::process::exit(code),
    };

    if state.zDbFilename.is_empty() {
        state.opened_transient_in_memory = true;
        state.zDbFilename = ":memory:".to_string();
    }

    let (db, con) = match db::open_db(&state) {
        Ok(v) => v,
        Err(code) => std::process::exit(code),
    };
    let mut session = Session::new(db, con);
    signals::set_connection(session.con);

    // Match the shipped shell behavior: configure default extension settings and load the
    // statically-available autocomplete extension when possible.
    db::configure_default_extension_settings(&state, session.con);
    let _ = db::ensure_autocomplete_loaded(&state, session.con);
    // Our libduckdb build may not include the json type by default; load it eagerly (cached).
    let _ = db::ensure_json_loaded(&state, session.con);
    // Match shell: enable progress bar rendering on console.
    db::enable_console_progress_bar(&state, session.con);

    // Ensure timestamptz formatting matches the shipped CLI behavior (local tz when available).
    db::init_local_timezone(&mut state, session.con);
    db::load_reserved_keywords(&mut state, session.con);
    db::sync_process_timezone(&mut state, session.con);

    if state.safe_mode {
        if let Err(code) = set_safe_mode(session.con) {
            db::close_db(&mut session.db, &mut session.con);
            signals::clear_connection();
            std::process::exit(code);
        }
    }

    let init_file = if state.initFile.is_empty() {
        None
    } else {
        Some(state.initFile.clone())
    };

    if state.run_init && !process_duckdbrc(&mut state, &mut session, init_file.as_deref()) {
        let bail_on_init_fail = state.bail != BailOnError::DontBail;
        if bail_on_init_fail {
            if let Some(path) = state.duckdb_rc_path.as_deref() {
                print_database_error(&format!(
                    "Encountered errors while executing init file \"{}\". Exiting.",
                    path
                ));
            }
            db::close_db(&mut session.db, &mut session.con);
            signals::clear_connection();
            std::process::exit(1);
        }
    }
    // ~/.duckdbrc may set TimeZone; keep Rust-side timestamptz formatting in sync.
    db::sync_process_timezone(&mut state, session.con);

    for call in &command_line_calls {
        let option: &CommandLineOption = &options::COMMAND_LINE_OPTIONS[call.option_index];
        if let Some(cb) = option.post_init_callback {
            let result = cb(&mut state, &call.arguments);
            if result == MetadataResult::Exit {
                break;
            }
        }
    }

    if state.print_help_and_exit {
        options::print_usage(&state.program_name);
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(0);
    }

    if state.print_version_and_exit {
        if let Some(info) = db::query_version_info(session.con) {
            println!(
                "{} ({}) {}",
                info.library_version, info.codename, info.source_id
            );
        } else {
            println!("{}", version_str.trim());
        }
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(0);
    }

    let run_initial_actions = |state: &mut ShellState, session: &mut Session| -> i32 {
        let mut rc: i32 = 0;
        let initial_commands = state.initial_commands.clone();
        for action in &initial_commands {
            match action {
                InitialAction::Command {
                    text,
                    bail_on_error,
                } => {
                    rc = exec::run_command(state, session, text);
                    if rc == 2 {
                        return state.exit_code.unwrap_or(0);
                    }
                    if rc != 0 && *bail_on_error {
                        return rc;
                    }
                }
                InitialAction::File {
                    path,
                    bail_on_error,
                } => {
                    let old_bail = state.bail;
                    state.bail = if *bail_on_error {
                        BailOnError::Bail
                    } else {
                        BailOnError::DontBail
                    };
                    let file = std::fs::File::open(path);
                    rc = if let Ok(file) = file {
                        let reader = std::io::BufReader::new(file);
                        repl::process_reader(state, session, reader, InputMode::File, false)
                    } else {
                        print_database_error(&format!("Failed to read file \"{}\"", path));
                        1
                    };
                    state.bail = old_bail;
                    if rc != 0 && *bail_on_error {
                        return rc;
                    }
                }
            }
        }
        rc
    };

    if !state.readStdin {
        let mut rc = run_initial_actions(&mut state, &mut session);
        if rc == 0 {
            for cmd in &extra_commands {
                rc = exec::run_command(&mut state, &mut session, cmd);
                if rc == 2 {
                    rc = state.exit_code.unwrap_or(0);
                    break;
                }
                if rc != 0 && state.get_bail_on_error(InputMode::File) {
                    break;
                }
            }
        }
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(rc);
    }

    crate::highlight::detect_dark_light_mode(&mut state);

    if state.stdin_is_interactive {
        repl::run_interactive_banner(&state, session.con);
        crate::completion::install(session.con);

        let rc = run_initial_actions(&mut state, &mut session);
        if state.exit_after_initial_commands {
            db::close_db(&mut session.db, &mut session.con);
            signals::clear_connection();
            std::process::exit(rc);
        }
        if rc != 0 && state.command_line_command_bail() {
            db::close_db(&mut session.db, &mut session.con);
            signals::clear_connection();
            std::process::exit(rc);
        }

        let rc = match state.rl_version {
            crate::state::ReadLineVersion::Linenoise => {
                repl::process_stdin_interactive(&mut state, &mut session)
            }
            crate::state::ReadLineVersion::Fallback => {
                let stdin = std::io::stdin();
                repl::process_reader(
                    &mut state,
                    &mut session,
                    stdin.lock(),
                    InputMode::Standard,
                    true,
                )
            }
        };
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(rc);
    }

    let rc = run_initial_actions(&mut state, &mut session);
    if state.exit_after_initial_commands {
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(rc);
    }
    if rc != 0 && state.command_line_command_bail() {
        db::close_db(&mut session.db, &mut session.con);
        signals::clear_connection();
        std::process::exit(rc);
    }

    let stdin = std::io::stdin();
    let rc = repl::process_reader(
        &mut state,
        &mut session,
        stdin.lock(),
        InputMode::Standard,
        false,
    );
    db::close_db(&mut session.db, &mut session.con);
    signals::clear_connection();
    std::process::exit(rc);
}
