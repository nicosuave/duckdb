use crate::state::ShellState;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe extern "C" {
    fn tzset();
}

fn print_database_error(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = std::io::Write::write_all(&mut stderr, msg.as_bytes());
    if !msg.ends_with('\n') {
        let _ = std::io::Write::write_all(&mut stderr, b"\n");
    }
}

fn sql_escape_single_quotes(s: &str) -> String {
    s.replace('\'', "''")
}

fn set_setting_quiet(con: duckdb_sys::duckdb_connection, name: &str, value: &str) -> bool {
    let value = sql_escape_single_quotes(value);
    run_sql_quiet(con, &format!("set {}='{}'", name, value))
}

fn try_create_dir_all(path: &Path) -> bool {
    std::fs::create_dir_all(path).is_ok()
}

fn try_write_probe_file(dir: &Path) -> bool {
    let probe = dir.join(".duckdb_cli_write_probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn choose_extension_directory_base() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("DUCKDB_EXTENSION_DIRECTORY") {
        let dir = dir.trim().to_string();
        if !dir.is_empty() {
            let p = PathBuf::from(dir);
            if try_create_dir_all(&p) && try_write_probe_file(&p) {
                return Some(p);
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim().to_string();
        if !home.is_empty() {
            let p = PathBuf::from(home).join(".duckdb").join("extensions");
            if try_create_dir_all(&p) && try_write_probe_file(&p) {
                return Some(p);
            }
        }
    }

    let Ok(cwd) = std::env::current_dir() else {
        return None;
    };
    let p = cwd.join(".duckdb").join("extensions");
    if try_create_dir_all(&p) && try_write_probe_file(&p) {
        Some(p)
    } else {
        None
    }
}

fn candidate_repo_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            let mut cur = dir;
            for _ in 0..6 {
                if let Some(parent) = cur.parent() {
                    roots.push(parent.to_path_buf());
                    cur = parent;
                } else {
                    break;
                }
            }
        }
    }

    // Dedupe while preserving order.
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !out.iter().any(|p| p == &r) {
            out.push(r);
        }
    }
    out
}

fn find_in_tree_extension_repository(source_id: &str) -> Option<PathBuf> {
    if let Ok(repo) = std::env::var("DUCKDB_CUSTOM_EXTENSION_REPOSITORY") {
        let repo = repo.trim().to_string();
        if !repo.is_empty() {
            return Some(PathBuf::from(repo));
        }
    }

    for root in candidate_repo_roots() {
        for rel in ["build/release/repository", "build/debug/repository"] {
            let base = root.join(rel);
            if base.join(source_id).is_dir() {
                return Some(base);
            }
        }
    }
    None
}

pub fn configure_default_extension_settings(
    state: &ShellState,
    con: duckdb_sys::duckdb_connection,
) {
    if state.safe_mode {
        return;
    }

    // In restricted environments (e.g., sandboxed tests), writing to $HOME may be blocked.
    // Fall back to a repo-local extension directory in the current working directory.
    if let Some(dir) = choose_extension_directory_base() {
        if let Some(dir) = dir.to_str() {
            let _ = set_setting_quiet(con, "extension_directory", dir);
        }
    }

    let Some(info) = query_version_info(con) else {
        return;
    };
    let Some(repo) = find_in_tree_extension_repository(info.source_id.as_str()) else {
        return;
    };
    if let Some(repo) = repo.to_str() {
        let _ = set_setting_quiet(con, "custom_extension_repository", repo);
    }
}

pub fn error_mentions_icu_extension(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("icu extension")
        || (lower.contains("install icu") && lower.contains("load icu"))
        || lower.contains("exists in the icu extension")
}

pub fn error_mentions_json_extension(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("json extension")
        || (lower.contains("install json") && lower.contains("load json"))
        || lower.contains("exists in the json extension")
}

pub fn ensure_icu_loaded(state: &ShellState, con: duckdb_sys::duckdb_connection) -> bool {
    if state.safe_mode {
        return false;
    }

    configure_default_extension_settings(state, con);

    if run_sql_quiet(con, "LOAD icu") {
        return true;
    }

    // Prefer matching shell behavior: attempt to install/load quietly when needed.
    // In dev/source builds this should succeed from the in-tree repository (no network).
    if !run_sql_quiet(con, "INSTALL icu") {
        return false;
    }
    run_sql_quiet(con, "LOAD icu")
}

pub fn ensure_json_loaded(state: &ShellState, con: duckdb_sys::duckdb_connection) -> bool {
    if state.safe_mode {
        return false;
    }

    configure_default_extension_settings(state, con);

    if run_sql_quiet(con, "LOAD json") {
        return true;
    }

    if !run_sql_quiet(con, "INSTALL json") {
        return false;
    }
    run_sql_quiet(con, "LOAD json")
}

pub fn ensure_autocomplete_loaded(state: &ShellState, con: duckdb_sys::duckdb_connection) -> bool {
    if state.safe_mode {
        return false;
    }

    configure_default_extension_settings(state, con);

    if run_sql_quiet(con, "LOAD autocomplete") {
        return true;
    }
    if !run_sql_quiet(con, "INSTALL autocomplete") {
        return false;
    }
    run_sql_quiet(con, "LOAD autocomplete")
}

pub fn enable_console_progress_bar(state: &ShellState, con: duckdb_sys::duckdb_connection) {
    if !state.stdout_is_console {
        return;
    }
    let _ = run_sql_quiet(con, "PRAGMA enable_progress_bar");
    let _ = run_sql_quiet(con, "PRAGMA disable_print_progress_bar");
}

fn run_sql_quiet(con: duckdb_sys::duckdb_connection, sql: &str) -> bool {
    let Ok(sql) = CString::new(sql) else {
        return false;
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result) };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    rc == duckdb_sys::DuckDBSuccess
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
    if rows == 0 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let value_ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, 0) };
    if value_ptr.is_null() {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let value = unsafe { CStr::from_ptr(value_ptr) }
        .to_string_lossy()
        .to_string();
    unsafe { duckdb_sys::duckdb_free(value_ptr as *mut _) };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    Some(value)
}

#[derive(Clone, Debug)]
pub struct VersionInfo {
    pub library_version: String,
    pub source_id: String,
    pub codename: String,
}

pub fn query_version_info(con: duckdb_sys::duckdb_connection) -> Option<VersionInfo> {
    // pragma_version() returns: library_version, source_id, codename
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let sql =
        CString::new("select library_version, source_id, codename from pragma_version()").ok()?;
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut result) };
    if rows == 0 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return None;
    }
    let mut col = |idx: u64| -> Option<String> {
        let ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, idx, 0) };
        if ptr.is_null() {
            return None;
        }
        let v = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
        Some(v)
    };
    let out = VersionInfo {
        library_version: col(0)?,
        source_id: col(1)?,
        codename: col(2)?,
    };
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
    Some(out)
}

pub fn load_reserved_keywords(state: &mut ShellState, con: duckdb_sys::duckdb_connection) {
    if state.reserved_keywords_loaded {
        return;
    }
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let mut run = |sql: &str| -> bool {
        let Ok(sql) = CString::new(sql) else {
            return false;
        };
        let rc = unsafe { duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result) };
        rc == duckdb_sys::DuckDBSuccess
    };
    // DuckDB's duckdb_keywords() schema changed across versions.
    // Try the historical "keyword" name first, then the current "keyword_name".
    if !run("select keyword from duckdb_keywords()")
        && !run("select keyword_name from duckdb_keywords()")
    {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return;
    }
    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut result) } as usize;
    for row in 0..rows {
        let ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, row as u64) };
        if ptr.is_null() {
            continue;
        }
        let kw = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
        let kw = kw.trim();
        if !kw.is_empty() {
            state.reserved_keywords.insert(kw.to_ascii_uppercase());
        }
    }
    state.reserved_keywords_loaded = true;
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
}

#[cfg(target_os = "linux")]
fn linux_timedatectl_tz_name() -> Option<String> {
    let output = Command::new("timedatectl")
        .arg("show")
        .arg("-p")
        .arg("Timezone")
        .arg("--value")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tz = String::from_utf8_lossy(&output.stdout);
    let tz = tz.trim();
    if tz.is_empty() {
        return None;
    }
    Some(tz.to_string())
}

#[cfg(not(target_os = "linux"))]
fn linux_timedatectl_tz_name() -> Option<String> {
    None
}

fn localtime_symlink_tz_name() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_string_lossy();
    let marker = "zoneinfo/";
    let idx = target.rfind(marker)?;
    let tz = &target[idx + marker.len()..];
    let tz = tz.trim_matches('/').trim();
    if tz.is_empty() {
        return None;
    }
    Some(tz.to_string())
}

fn etc_timezone_file_tz_name() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/timezone").ok()?;
    let tz = contents.lines().next()?.trim();
    if tz.is_empty() {
        return None;
    }
    Some(tz.to_string())
}

#[cfg(target_os = "macos")]
fn macos_systemsetup_tz_name() -> Option<String> {
    let output = Command::new("/usr/sbin/systemsetup")
        .arg("-gettimezone")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    for line in combined.lines() {
        let line = line.trim();
        let prefix = "Time Zone:";
        if let Some(rest) = line.strip_prefix(prefix) {
            let tz = rest.trim();
            if !tz.is_empty() {
                return Some(tz.to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn macos_systemsetup_tz_name() -> Option<String> {
    None
}

fn local_tz_name() -> Option<String> {
    macos_systemsetup_tz_name()
        .or_else(linux_timedatectl_tz_name)
        .or_else(localtime_symlink_tz_name)
        .or_else(etc_timezone_file_tz_name)
}

fn tz_is_utcish(tz: &str) -> bool {
    let tz = tz.trim();
    if tz.is_empty() {
        return false;
    }
    let tz = tz.to_ascii_lowercase();
    matches!(
        tz.as_str(),
        "utc" | "etc/utc" | "z" | "gmt" | "etc/gmt" | "etc/gmt0" | "gmt0"
    )
}

pub fn init_local_timezone(state: &mut ShellState, con: duckdb_sys::duckdb_connection) {
    if state.safe_mode {
        return;
    }

    // Best-effort: make the TimeZone setting available for TIMESTAMPTZ formatting.
    // Keep quiet on failure (e.g., no installed extension, no permissions for extension install).
    //
    // NOTE: In development/source builds, ICU might not be built in. If the user passed `--unsigned`,
    // we can install the in-tree ICU extension from a local repository (no network).
    let _ = ensure_icu_loaded(state, con);

    // Preserve an explicit user setting across `.open` by applying the already-chosen process TZ
    // to the new connection.
    if let Some(tz) = state.applied_process_tz.as_deref() {
        let tz = tz.trim();
        if !tz.is_empty() {
            let tz_escaped = sql_escape_single_quotes(tz);
            let _ = run_sql_quiet(con, &format!("SET TimeZone='{}'", tz_escaped));
        }
        return;
    }

    // Respect any existing setting (e.g., from ~/.duckdbrc or prior commands).
    if let Some(current_tz) = query_single_varchar(con, "select current_setting('TimeZone')") {
        let current_tz = current_tz.trim();
        if !current_tz.is_empty() && !tz_is_utcish(current_tz) {
            return;
        }
    }

    let Some(tz) = local_tz_name() else {
        // Match shipped shell behavior: prefer OS local time zone even when TZ is set.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        unsafe {
            std::env::remove_var("TZ");
            tzset();
        }
        return;
    };
    let tz_escaped = sql_escape_single_quotes(&tz);
    let _ = run_sql_quiet(con, &format!("SET TimeZone='{}'", tz_escaped));
    apply_process_timezone_setting(state, tz.as_str());
}

pub fn sync_process_timezone(state: &mut ShellState, con: duckdb_sys::duckdb_connection) {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (state, con);
        return;
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let Some(tz) = query_single_varchar(con, "select current_setting('TimeZone')") else {
            return;
        };
        let tz = tz.trim().to_string();
        if tz.is_empty() {
            return;
        }
        if tz_is_utcish(tz.as_str()) && state.applied_process_tz.is_none() {
            return;
        }
        if state.applied_process_tz.as_deref() == Some(tz.as_str()) {
            return;
        }

        apply_process_timezone_setting(state, tz.as_str());
    }
}

pub fn apply_process_timezone_setting(state: &mut ShellState, tz: &str) {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (state, tz);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let tz = tz.trim();
        if tz.is_empty() {
            return;
        }
        if state.applied_process_tz.as_deref() == Some(tz) {
            return;
        }

        // Prefer zoneinfo lookups for IANA names.
        let env_tz = if tz.contains('/') && !tz.starts_with(':') {
            format!(":{}", tz)
        } else {
            tz.to_string()
        };
        std::env::set_var("TZ", env_tz);
        unsafe { tzset() };
        state.applied_process_tz = Some(tz.to_string());
    }
}

pub fn open_db(
    state: &ShellState,
) -> Result<(duckdb_sys::duckdb_database, duckdb_sys::duckdb_connection), i32> {
    open_db_with_overrides(state, &[])
}

pub fn open_db_with_overrides(
    state: &ShellState,
    overrides: &[(String, String)],
) -> Result<(duckdb_sys::duckdb_database, duckdb_sys::duckdb_connection), i32> {
    let mut config: duckdb_sys::duckdb_config = std::ptr::null_mut();
    let config_state = unsafe { duckdb_sys::duckdb_create_config(&mut config) };
    if config_state != duckdb_sys::DuckDBSuccess {
        print_database_error("Failed to allocate duckdb_config");
        return Err(1);
    }

    for (k, v) in &state.config_kv {
        let k_c = CString::new(k.as_str()).map_err(|_| 1)?;
        let v_c = CString::new(v.as_str()).map_err(|_| 1)?;
        let rc = unsafe { duckdb_sys::duckdb_set_config(config, k_c.as_ptr(), v_c.as_ptr()) };
        if rc != duckdb_sys::DuckDBSuccess {
            unsafe { duckdb_sys::duckdb_destroy_config(&mut config) };
            print_database_error(&format!("Failed to set config option '{}'='{}'", k, v));
            return Err(1);
        }
    }

    for (k, v) in overrides {
        let k_c = CString::new(k.as_str()).map_err(|_| 1)?;
        let v_c = CString::new(v.as_str()).map_err(|_| 1)?;
        let rc = unsafe { duckdb_sys::duckdb_set_config(config, k_c.as_ptr(), v_c.as_ptr()) };
        if rc != duckdb_sys::DuckDBSuccess {
            unsafe { duckdb_sys::duckdb_destroy_config(&mut config) };
            print_database_error(&format!("Failed to set config option '{}'='{}'", k, v));
            return Err(1);
        }
    }

    let db_path = CString::new(state.zDbFilename.as_str()).map_err(|_| 1)?;
    let mut db: duckdb_sys::duckdb_database = std::ptr::null_mut();
    let mut out_error: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc =
        unsafe { duckdb_sys::duckdb_open_ext(db_path.as_ptr(), &mut db, config, &mut out_error) };
    unsafe { duckdb_sys::duckdb_destroy_config(&mut config) };
    if rc != duckdb_sys::DuckDBSuccess {
        if !out_error.is_null() {
            let err = unsafe { CStr::from_ptr(out_error) }
                .to_string_lossy()
                .to_string();
            unsafe { duckdb_sys::duckdb_free(out_error as *mut _) };
            print_database_error(&format!(
                "Error: unable to open database \"{}\": {}",
                state.zDbFilename, err
            ));
        } else {
            print_database_error("Failed to open database");
        }
        return Err(1);
    }

    let mut con: duckdb_sys::duckdb_connection = std::ptr::null_mut();
    let rc = unsafe { duckdb_sys::duckdb_connect(db, &mut con) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_close(&mut db) };
        print_database_error("Failed to connect to database");
        return Err(1);
    }

    if let Err(err) = crate::shell_ext::register(db, con) {
        print_database_error(&format!("Failed to register shell extensions: {}", err));
        unsafe {
            duckdb_sys::duckdb_disconnect(&mut con);
            duckdb_sys::duckdb_close(&mut db);
        }
        return Err(1);
    }

    configure_default_extension_settings(state, con);

    Ok((db, con))
}

pub fn close_db(db: &mut duckdb_sys::duckdb_database, con: &mut duckdb_sys::duckdb_connection) {
    if !con.is_null() {
        unsafe { duckdb_sys::duckdb_disconnect(con) };
    }
    if !db.is_null() {
        unsafe { duckdb_sys::duckdb_close(db) };
    }
}
