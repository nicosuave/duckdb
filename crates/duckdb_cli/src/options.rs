use crate::candidates;
use crate::state::{InitialAction, MetadataResult, RenderMode, ShellFlags, ShellState};

pub struct CommandLineOption {
    pub option: &'static str,
    pub argument_count: usize,
    pub arguments: &'static str,
    pub pre_init_callback: Option<fn(&mut ShellState, &[String]) -> MetadataResult>,
    pub post_init_callback: Option<fn(&mut ShellState, &[String]) -> MetadataResult>,
    pub description: &'static str,
}

fn toggle_output_mode(state: &mut ShellState, mode: RenderMode) -> MetadataResult {
    state.cMode = mode;
    state.mode = mode;
    MetadataResult::Success
}

fn toggle_ascii_mode(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.cMode = RenderMode::ASCII;
    state.mode = RenderMode::ASCII;
    state.colSeparator = "\x1F".to_string();
    state.rowSeparator = "\x1E".to_string();
    MetadataResult::Success
}

fn toggle_csv_mode(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.cMode = RenderMode::CSV;
    state.mode = RenderMode::CSV;
    state.colSeparator = ",".to_string();
    MetadataResult::Success
}

fn enable_bail(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.bail_on_error = true;
    MetadataResult::Success
}

fn enable_batch(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.stdin_is_interactive = false;
    MetadataResult::Success
}

fn disable_batch(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.stdin_is_interactive = true;
    MetadataResult::Success
}

fn set_read_only_mode(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state
        .config_kv
        .push(("access_mode".to_string(), "read_only".to_string()));
    MetadataResult::Success
}

fn toggle_header<const HEADER: bool>(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.showHeader = HEADER;
    MetadataResult::Success
}

fn disable_stdin(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.readStdin = false;
    MetadataResult::Success
}

fn enable_echo(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.shellFlgs |= ShellFlags::SHFLG_Echo as u32;
    MetadataResult::Success
}

fn allow_unredacted(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state
        .config_kv
        .push(("allow_unredacted_secrets".to_string(), "true".to_string()));
    MetadataResult::Success
}

fn allow_unsigned(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state
        .config_kv
        .push(("allow_unsigned_extensions".to_string(), "true".to_string()));
    MetadataResult::Success
}

fn show_version_and_exit(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.print_version_and_exit = true;
    MetadataResult::Exit
}

fn print_help_and_exit(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.print_help_and_exit = true;
    MetadataResult::Exit
}

fn set_newline_separator(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.rowSeparator = args[1].clone();
    MetadataResult::Success
}

fn set_null_value(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.nullValue = args[1].clone();
    MetadataResult::Success
}

fn set_separator(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.colSeparator = args[1].clone();
    MetadataResult::Success
}

fn enable_safe_mode(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.safe_mode = true;
    MetadataResult::Success
}

fn set_init_file(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.initFile = args[1].clone();
    MetadataResult::Success
}

fn run_command_exit(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.readStdin = false;
    state.initial_commands.push(InitialAction::Command {
        text: args[1].clone(),
        bail_on_error: true,
    });
    state.exit_after_initial_commands = true;
    MetadataResult::Success
}

fn run_command_keep_running(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.initial_commands.push(InitialAction::Command {
        text: args[1].clone(),
        bail_on_error: state.bail_on_error,
    });
    MetadataResult::Success
}

fn process_file_and_exit(state: &mut ShellState, args: &[String]) -> MetadataResult {
    state.readStdin = false;
    state.stdin_is_interactive = false;
    state.initial_commands.push(InitialAction::File {
        path: args[1].clone(),
        bail_on_error: true,
    });
    state.exit_after_initial_commands = true;
    MetadataResult::Success
}

fn launch_ui(state: &mut ShellState, _args: &[String]) -> MetadataResult {
    state.launch_ui = true;
    if !state.ui_command.trim().is_empty() {
        state.initial_commands.push(InitialAction::Command {
            text: state.ui_command.clone(),
            bail_on_error: true,
        });
    }
    MetadataResult::Success
}

fn toggle_mode(state: &mut ShellState, mode: RenderMode) -> MetadataResult {
    toggle_output_mode(state, mode)
}

pub static COMMAND_LINE_OPTIONS: &[CommandLineOption] = &[
    CommandLineOption {
        option: "ascii",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(toggle_ascii_mode),
        description: "set output mode to 'ascii'",
    },
    CommandLineOption {
        option: "bail",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(enable_bail),
        description: "stop after hitting an error",
    },
    CommandLineOption {
        option: "batch",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(enable_batch),
        post_init_callback: Some(enable_batch),
        description: "force batch I/O'",
    },
    CommandLineOption {
        option: "box",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::BOX)),
        description: "set output mode to 'box'",
    },
    CommandLineOption {
        option: "column",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::COLUMN)),
        description: "set output mode to 'column'",
    },
    CommandLineOption {
        option: "cmd",
        argument_count: 1,
        arguments: "COMMAND",
        pre_init_callback: None,
        post_init_callback: Some(run_command_keep_running),
        description: "run \"COMMAND\" before reading stdin",
    },
    CommandLineOption {
        option: "csv",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(toggle_csv_mode),
        description: "set output mode to 'csv'",
    },
    CommandLineOption {
        option: "c",
        argument_count: 1,
        arguments: "COMMAND",
        pre_init_callback: Some(enable_batch),
        post_init_callback: Some(run_command_exit),
        description: "run \"COMMAND\" and exit",
    },
    CommandLineOption {
        option: "echo",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(enable_echo),
        description: "print commands before execution",
    },
    CommandLineOption {
        option: "f",
        argument_count: 1,
        arguments: "FILENAME",
        pre_init_callback: Some(enable_batch),
        post_init_callback: Some(process_file_and_exit),
        description: "read/process named file and exit",
    },
    CommandLineOption {
        option: "init",
        argument_count: 1,
        arguments: "FILENAME",
        pre_init_callback: Some(set_init_file),
        post_init_callback: None,
        description: "read/process named file",
    },
    CommandLineOption {
        option: "header",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(toggle_header::<true>),
        description: "turn headers on",
    },
    CommandLineOption {
        option: "help",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(enable_batch),
        post_init_callback: Some(print_help_and_exit),
        description: "show this message",
    },
    CommandLineOption {
        option: "html",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::HTML)),
        description: "set output mode to HTML",
    },
    CommandLineOption {
        option: "interactive",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(disable_batch),
        description: "force interactive I/O",
    },
    CommandLineOption {
        option: "json",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::JSON)),
        description: "set output mode to 'json'",
    },
    CommandLineOption {
        option: "line",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::LINE)),
        description: "set output mode to 'line'",
    },
    CommandLineOption {
        option: "list",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::LIST)),
        description: "set output mode to 'list'",
    },
    CommandLineOption {
        option: "markdown",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::MARKDOWN)),
        description: "set output mode to 'markdown'",
    },
    CommandLineOption {
        option: "newline",
        argument_count: 1,
        arguments: "SEP",
        pre_init_callback: None,
        post_init_callback: Some(set_newline_separator),
        description: "set output row separator. Default: '\\n'",
    },
    CommandLineOption {
        option: "no-stdin",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(disable_stdin),
        description: "exit after processing options instead of reading stdin",
    },
    CommandLineOption {
        option: "noheader",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(toggle_header::<false>),
        description: "turn headers off",
    },
    CommandLineOption {
        option: "nullvalue",
        argument_count: 1,
        arguments: "TEXT",
        pre_init_callback: None,
        post_init_callback: Some(set_null_value),
        description: "set text string for NULL values. Default 'NULL'",
    },
    CommandLineOption {
        option: "quote",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::QUOTE)),
        description: "set output mode to 'quote'",
    },
    CommandLineOption {
        option: "readonly",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(set_read_only_mode),
        post_init_callback: None,
        description: "open the database read-only",
    },
    CommandLineOption {
        option: "s",
        argument_count: 1,
        arguments: "COMMAND",
        pre_init_callback: Some(enable_batch),
        post_init_callback: Some(run_command_exit),
        description: "run \"COMMAND\" and exit",
    },
    CommandLineOption {
        option: "safe",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(enable_safe_mode),
        post_init_callback: None,
        description: "enable safe-mode",
    },
    CommandLineOption {
        option: "separator",
        argument_count: 1,
        arguments: "SEP",
        pre_init_callback: None,
        post_init_callback: Some(set_separator),
        description: "set output column separator. Default: '|'",
    },
    CommandLineOption {
        option: "storage-version",
        argument_count: 1,
        arguments: "VER",
        pre_init_callback: Some(|state, args| {
            state
                .config_kv
                .push(("storage_compatibility_version".to_string(), args[1].clone()));
            state.storage_version = Some(args[1].clone());
            MetadataResult::Success
        }),
        post_init_callback: None,
        description: "database storage compatibility version to use. Default: 'v0.10.0'",
    },
    CommandLineOption {
        option: "table",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(|s, _| toggle_mode(s, RenderMode::TABLE)),
        description: "set output mode to 'table'",
    },
    CommandLineOption {
        option: "ui",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(launch_ui),
        description:
            "launches a web interface using the ui extension (configurable with .ui_command)",
    },
    CommandLineOption {
        option: "unredacted",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(allow_unredacted),
        post_init_callback: None,
        description: "allow printing unredacted secrets",
    },
    CommandLineOption {
        option: "unsigned",
        argument_count: 0,
        arguments: "",
        pre_init_callback: Some(allow_unsigned),
        post_init_callback: None,
        description: "allow loading of unsigned extensions",
    },
    CommandLineOption {
        option: "version",
        argument_count: 0,
        arguments: "",
        pre_init_callback: None,
        post_init_callback: Some(show_version_and_exit),
        description: "show DuckDB version",
    },
];

fn find_option_index(name: &str) -> Option<usize> {
    COMMAND_LINE_OPTIONS
        .iter()
        .position(|opt| opt.option == name)
}

pub fn find_command_line_option<'a>(
    name: &str,
    program_name: &str,
) -> Result<&'a CommandLineOption, String> {
    let mut idx = find_option_index(name);
    if idx.is_none() {
        let replaced = name.replace('_', "-");
        idx = find_option_index(&replaced);
    }
    if let Some(idx) = idx {
        return Ok(&COMMAND_LINE_OPTIONS[idx]);
    }

    let mut error_msg = format!("Unknown Option Error: Unrecognized option '-{}'\n", name);
    let option_names: Vec<String> = COMMAND_LINE_OPTIONS
        .iter()
        .map(|o| format!("-{}", o.option))
        .collect();
    error_msg.push_str(&candidates::candidates_error_message(
        &option_names,
        &format!("-{}", name),
        "Did you mean",
    ));
    error_msg.push('\n');
    error_msg.push_str(&format!(
        "Run '{} -help' for a list of options.\n",
        program_name
    ));
    Err(error_msg)
}

pub fn print_usage(program_name: &str) {
    print!("Usage: {} [OPTIONS] FILENAME [SQL]\n\n", program_name);
    print!(
		"FILENAME is the name of a DuckDB database. A new database is created\nif the file does not previously exist.\n\n"
	);
    print!("OPTIONS:\n");

    const INITIAL_SPACING: usize = 2;
    const MIN_SPACING: usize = 4;

    let mut max_lhs_length: usize = 0;
    for opt in COMMAND_LINE_OPTIONS {
        let mut lhs_length = INITIAL_SPACING + 1 + opt.option.len(); // "  -" + option
        if !opt.arguments.is_empty() {
            lhs_length += 1 + opt.arguments.len();
        }
        max_lhs_length = max_lhs_length.max(lhs_length);
    }

    for opt in COMMAND_LINE_OPTIONS {
        let command_name = format!("{}-{}", " ".repeat(INITIAL_SPACING), opt.option);
        let lhs_length = if opt.arguments.is_empty() {
            command_name.len()
        } else {
            command_name.len() + 1 + opt.arguments.len()
        };
        let padding = max_lhs_length.saturating_sub(lhs_length) + MIN_SPACING;
        if opt.arguments.is_empty() {
            print!(
                "{}{}{}\n",
                command_name,
                " ".repeat(padding),
                opt.description
            );
        } else {
            print!(
                "{} {}{}{}\n",
                command_name,
                opt.arguments,
                " ".repeat(padding),
                opt.description
            );
        }
    }
}
