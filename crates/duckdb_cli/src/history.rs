use std::fs::OpenOptions;
use std::io::{Read, Write};

pub const MAX_HISTORY_LINES: usize = 2000;

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok().filter(|s| !s.is_empty())
}

pub fn history_path() -> Option<String> {
    if let Ok(path) = std::env::var("DUCKDB_HISTORY") {
        if !path.is_empty() {
            return Some(path);
        }
    }
    home_dir().map(|home| format!("{}/.duckdb_history", home))
}

#[allow(dead_code)]
pub fn load(path: &str) -> Vec<String> {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return Vec::new();
    }
    let mut lines: Vec<String> = buf.lines().map(|s| s.to_string()).collect();
    if lines.len() > MAX_HISTORY_LINES {
        lines.drain(0..(lines.len() - MAX_HISTORY_LINES));
    }
    lines
}

#[allow(dead_code)]
pub fn save(path: &str, entries: &[String]) -> std::io::Result<()> {
    let start = entries.len().saturating_sub(MAX_HISTORY_LINES);
    let slice = &entries[start..];

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;

    for line in slice {
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }
    Ok(())
}
