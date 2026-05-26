use crate::state::ShellState;
use std::fs::File;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub enum OutputHandle {
    Stdout,
    Stderr,
    File(File),
    Pipe(Child),
}

impl OutputHandle {
    pub fn write_all(&mut self, bytes: &[u8]) {
        match self {
            OutputHandle::Stdout => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(bytes);
            }
            OutputHandle::Stderr => {
                let mut stderr = std::io::stderr().lock();
                let _ = stderr.write_all(bytes);
            }
            OutputHandle::File(f) => {
                let _ = f.write_all(bytes);
            }
            OutputHandle::Pipe(child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(bytes);
                }
            }
        }
    }
}

fn expand_path(path: &str) -> String {
    let path = path.trim();
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = crate::paths::home_dir() {
            return format!("{}/{}", home, rest);
        }
    }
    path.to_string()
}

fn tmp_dir() -> String {
    std::env::var("TMPDIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("TMP").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| std::env::var("TEMP").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned())
}

pub fn shell_command(cmd: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(cmd);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c").arg(cmd);
        command
    }
}

pub fn new_temp_file_path(suffix: &str) -> String {
    let base = tmp_dir();
    let base = base.trim_end_matches('/').to_string();
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}/duckdb-shell-{}-{}.{}", base, pid, nanos, suffix)
}

pub fn reset_output(state: &mut ShellState) {
    if state.outfile.starts_with('|') {
        if let OutputHandle::Pipe(mut child) =
            std::mem::replace(&mut state.out, OutputHandle::Stdout)
        {
            drop(child.stdin.take());
            let _ = child.wait();
        }
    } else {
        state.out = OutputHandle::Stdout;
        if state.doXdgOpen && !state.zTempFile.is_empty() {
            if cfg!(target_os = "windows") {
                let _ = Command::new("cmd")
                    .arg("/C")
                    .arg("start")
                    .arg("")
                    .arg(&state.zTempFile)
                    .status();
                std::thread::sleep(Duration::from_millis(2000));
            } else {
                let opener = if cfg!(target_os = "macos") {
                    "open"
                } else if cfg!(target_os = "linux") {
                    "xdg-open"
                } else {
                    ""
                };
                if !opener.is_empty() {
                    let _ = Command::new(opener).arg(&state.zTempFile).status();
                    std::thread::sleep(Duration::from_millis(2000));
                }
            }
            pop_output_mode(state);
            state.doXdgOpen = false;
        }
    }
    state.outfile.clear();
    state.stdout_is_console = true;
}

pub fn push_output_mode(state: &mut ShellState) {
    state.modePrior = state.mode;
    state.priorShFlgs = state.shellFlgs;
    state.colSepPrior = state.colSeparator.clone();
    state.rowSepPrior = state.rowSeparator.clone();
}

pub fn pop_output_mode(state: &mut ShellState) {
    state.mode = state.modePrior;
    state.shellFlgs = state.priorShFlgs;
    state.colSeparator = state.colSepPrior.clone();
    state.rowSeparator = state.rowSepPrior.clone();
}

pub fn open_pipe(state: &mut ShellState, cmd: &str) -> Result<(), String> {
    let child = shell_command(cmd)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|_| format!("Error: cannot open pipe \"{}\"", cmd))?;
    if child.stdin.is_none() {
        return Err(format!("Error: cannot open pipe \"{}\"", cmd));
    }
    state.out = OutputHandle::Pipe(child);
    Ok(())
}

pub fn open_output_file(state: &mut ShellState, path: &str) -> Result<(), String> {
    match path {
        "stdout" => {
            state.out = OutputHandle::Stdout;
            Ok(())
        }
        "stderr" => {
            state.out = OutputHandle::Stderr;
            Ok(())
        }
        "off" => Err("Error: cannot write to \"off\"".to_string()),
        other => {
            let expanded = expand_path(other);
            let file =
                File::create(&expanded).map_err(|_| format!("Error: cannot open \"{}\"", other))?;
            state.out = OutputHandle::File(file);
            Ok(())
        }
    }
}
