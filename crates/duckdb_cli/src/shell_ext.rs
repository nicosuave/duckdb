use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::ffi::CStr;
use std::ffi::CString;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::os::raw::c_char;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static EXTERNAL_ACCESS_ENABLED: AtomicBool = AtomicBool::new(true);
static LAST_RESULT_AVAILABLE: AtomicBool = AtomicBool::new(false);
static SUPPRESS_LOG_OUTPUT: AtomicBool = AtomicBool::new(false);
static PRINTED_LOGS: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

pub fn set_last_result_available(value: bool) {
    LAST_RESULT_AVAILABLE.store(value, Ordering::Relaxed);
}

pub fn set_suppress_log_output(value: bool) {
    SUPPRESS_LOG_OUTPUT.store(value, Ordering::Relaxed);
}

unsafe extern "C" fn shell_log_storage_write_log_entry(
    _extra_data: *mut c_void,
    _timestamp: *mut duckdb_sys::duckdb_timestamp,
    level: *const c_char,
    _log_type: *const c_char,
    log_message: *const c_char,
) {
    if SUPPRESS_LOG_OUTPUT.load(Ordering::Relaxed) {
        return;
    }
    if level.is_null() || log_message.is_null() {
        return;
    }
    let level = unsafe { CStr::from_ptr(level) }.to_string_lossy();
    let message = unsafe { CStr::from_ptr(log_message) }.to_string_lossy();
    let mut hasher = DefaultHasher::new();
    message.to_ascii_lowercase().hash(&mut hasher);
    let log_id = hasher.finish();
    let printed_logs = PRINTED_LOGS.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut printed_logs) = printed_logs.lock() {
        if !printed_logs.insert(log_id) {
            return;
        }
    }

    let mut stdout = std::io::stdout().lock();
    let style = match level.to_ascii_lowercase().as_str() {
        "trace" => "\x1b[1m\x1b[34m",
        "debug" => "\x1b[1m\x1b[33m",
        "info" => "\x1b[1m\x1b[32m",
        "warning" => "\x1b[90m",
        "error" | "fatal" => "\x1b[31m",
        _ => "",
    };
    if !style.is_empty() {
        let _ = stdout.write_all(style.as_bytes());
    }
    let _ = stdout.write_all(level.to_ascii_uppercase().as_bytes());
    let _ = stdout.write_all(b":\n");
    let _ = stdout.write_all(message.as_bytes());
    if !style.is_empty() {
        let _ = stdout.write_all(b"\x1b[00m");
    }
    let _ = stdout.write_all(b"\n\n");
    let _ = stdout.flush();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe fn dlsym_optional(symbol: &str) -> Option<*mut c_void> {
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[cfg(target_os = "macos")]
    let handle = (-2isize) as *mut c_void; // RTLD_DEFAULT
    #[cfg(target_os = "linux")]
    let handle = std::ptr::null_mut(); // RTLD_DEFAULT
    let Ok(symbol) = CString::new(symbol) else {
        return None;
    };
    let ptr = unsafe { dlsym(handle, symbol.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

pub fn sync_external_access(con: duckdb_sys::duckdb_connection) {
    let Ok(sql) = CString::new("SELECT current_setting('enable_external_access')") else {
        return;
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return;
    }
    let value_ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, 0) };
    if !value_ptr.is_null() {
        let s = unsafe { CStr::from_ptr(value_ptr) }.to_string_lossy();
        let enabled = s.trim().eq_ignore_ascii_case("true") || s.trim() == "1";
        EXTERNAL_ACCESS_ENABLED.store(enabled, Ordering::Relaxed);
        unsafe { duckdb_sys::duckdb_free(value_ptr as *mut _) };
    }
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
}

pub fn register(
    db: duckdb_sys::duckdb_database,
    con: duckdb_sys::duckdb_connection,
) -> Result<(), String> {
    unsafe extern "C" fn replacement_scan(
        info: duckdb_sys::duckdb_replacement_scan_info,
        table_name: *const c_char,
        _data: *mut c_void,
    ) {
        if table_name.is_null() {
            return;
        }
        let table = unsafe { CStr::from_ptr(table_name) }.to_string_lossy();
        if table.as_ref() != "_" {
            return;
        }
        if LAST_RESULT_AVAILABLE.load(Ordering::Relaxed) {
            // Let DuckDB bind the actual temp table "_" (created by the shell) instead.
            return;
        }
        const ERR: &[u8] = b"Failed to query last result \"_\": no result available\0";
        unsafe {
            duckdb_sys::duckdb_replacement_scan_set_error(info, ERR.as_ptr() as *const c_char)
        };
    }

    unsafe {
        duckdb_sys::duckdb_add_replacement_scan(
            db,
            Some(replacement_scan),
            std::ptr::null_mut(),
            None,
        );
    }

    unsafe extern "C" fn getenv_scalar(
        info: duckdb_sys::duckdb_function_info,
        input: duckdb_sys::duckdb_data_chunk,
        output: duckdb_sys::duckdb_vector,
    ) {
        if !EXTERNAL_ACCESS_ENABLED.load(Ordering::Relaxed) {
            let err = CString::new("getenv is disabled through configuration").unwrap();
            duckdb_sys::duckdb_scalar_function_set_error(info, err.as_ptr());
            return;
        }

        let count = duckdb_sys::duckdb_data_chunk_get_size(input) as u64;
        let arg0 = duckdb_sys::duckdb_data_chunk_get_vector(input, 0);
        let data = duckdb_sys::duckdb_vector_get_data(arg0);
        if data.is_null() {
            for i in 0..count {
                duckdb_sys::duckdb_vector_assign_string_element(
                    output,
                    i,
                    b"\0".as_ptr() as *const c_char,
                );
            }
            return;
        }

        let validity = duckdb_sys::duckdb_vector_get_validity(arg0);
        let strings = data as *const duckdb_sys::duckdb_string_t;
        for i in 0..count {
            if !validity.is_null() && !duckdb_sys::duckdb_validity_row_is_valid(validity, i) {
                duckdb_sys::duckdb_vector_assign_string_element(
                    output,
                    i,
                    b"\0".as_ptr() as *const c_char,
                );
                continue;
            }
            let s_ptr = strings.add(i as usize);
            let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as usize;
            let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const u8;
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            let env_name = String::from_utf8_lossy(bytes);
            let value = std::env::var(env_name.as_ref()).unwrap_or_default();
            duckdb_sys::duckdb_vector_assign_string_element_len(
                output,
                i,
                value.as_ptr() as *const c_char,
                value.len() as u64,
            );
        }
    }

    unsafe {
        let fun = duckdb_sys::duckdb_create_scalar_function();
        if fun.is_null() {
            return Err("duckdb_create_scalar_function returned null".to_string());
        }
        let mut fun_to_destroy = fun;

        let name =
            CString::new("getenv").map_err(|_| "Invalid scalar function name".to_string())?;
        duckdb_sys::duckdb_scalar_function_set_name(fun, name.as_ptr());
        duckdb_sys::duckdb_scalar_function_set_volatile(fun);

        let mut varchar_arg =
            duckdb_sys::duckdb_create_logical_type(duckdb_sys::DUCKDB_TYPE_VARCHAR);
        if varchar_arg.is_null() {
            duckdb_sys::duckdb_destroy_scalar_function(&mut fun_to_destroy);
            return Err("duckdb_create_logical_type(VARCHAR) returned null".to_string());
        }
        duckdb_sys::duckdb_scalar_function_add_parameter(fun, varchar_arg);

        let mut varchar_ret =
            duckdb_sys::duckdb_create_logical_type(duckdb_sys::DUCKDB_TYPE_VARCHAR);
        if varchar_ret.is_null() {
            duckdb_sys::duckdb_destroy_logical_type(&mut varchar_arg);
            duckdb_sys::duckdb_destroy_scalar_function(&mut fun_to_destroy);
            return Err("duckdb_create_logical_type(VARCHAR) returned null".to_string());
        }
        duckdb_sys::duckdb_scalar_function_set_return_type(fun, varchar_ret);
        duckdb_sys::duckdb_scalar_function_set_function(fun, Some(getenv_scalar));

        let rc = duckdb_sys::duckdb_register_scalar_function(con, fun);

        duckdb_sys::duckdb_destroy_logical_type(&mut varchar_arg);
        duckdb_sys::duckdb_destroy_logical_type(&mut varchar_ret);
        duckdb_sys::duckdb_destroy_scalar_function(&mut fun_to_destroy);

        if rc != duckdb_sys::DuckDBSuccess {
            return Err("duckdb_register_scalar_function(getenv) failed".to_string());
        }
    }

    sync_external_access(con);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    unsafe {
        type DuckdbCreateLogStorage = unsafe extern "C" fn() -> duckdb_sys::duckdb_log_storage;
        type DuckdbDestroyLogStorage =
            unsafe extern "C" fn(log_storage: *mut duckdb_sys::duckdb_log_storage);
        type DuckdbLogStorageSetWriteLogEntry = unsafe extern "C" fn(
            log_storage: duckdb_sys::duckdb_log_storage,
            function: duckdb_sys::duckdb_logger_write_log_entry_t,
        );
        type DuckdbLogStorageSetExtraData = unsafe extern "C" fn(
            log_storage: duckdb_sys::duckdb_log_storage,
            extra_data: *mut c_void,
            delete_callback: duckdb_sys::duckdb_delete_callback_t,
        );
        type DuckdbLogStorageSetName =
            unsafe extern "C" fn(log_storage: duckdb_sys::duckdb_log_storage, name: *const c_char);
        type DuckdbRegisterLogStorage = unsafe extern "C" fn(
            database: duckdb_sys::duckdb_database,
            log_storage: duckdb_sys::duckdb_log_storage,
        ) -> duckdb_sys::duckdb_state;

        let Some(create) = dlsym_optional("duckdb_create_log_storage") else {
            return Ok(());
        };
        let Some(destroy) = dlsym_optional("duckdb_destroy_log_storage") else {
            return Ok(());
        };
        let Some(set_write) = dlsym_optional("duckdb_log_storage_set_write_log_entry") else {
            return Ok(());
        };
        let Some(set_extra_data) = dlsym_optional("duckdb_log_storage_set_extra_data") else {
            return Ok(());
        };
        let Some(set_name) = dlsym_optional("duckdb_log_storage_set_name") else {
            return Ok(());
        };
        let Some(register) = dlsym_optional("duckdb_register_log_storage") else {
            return Ok(());
        };

        let create: DuckdbCreateLogStorage = std::mem::transmute(create);
        let destroy: DuckdbDestroyLogStorage = std::mem::transmute(destroy);
        let set_write: DuckdbLogStorageSetWriteLogEntry = std::mem::transmute(set_write);
        let set_extra_data: DuckdbLogStorageSetExtraData = std::mem::transmute(set_extra_data);
        let set_name: DuckdbLogStorageSetName = std::mem::transmute(set_name);
        let register: DuckdbRegisterLogStorage = std::mem::transmute(register);

        let name = CString::new("shell_log_storage")
            .map_err(|_| "Invalid log storage name".to_string())?;

        let log_storage = create();
        if log_storage.is_null() {
            return Err("duckdb_create_log_storage returned null".to_string());
        }
        let mut log_storage_to_destroy = log_storage;

        set_name(log_storage, name.as_ptr());
        set_write(log_storage, Some(shell_log_storage_write_log_entry));
        set_extra_data(log_storage, std::ptr::null_mut(), None);

        let rc = register(db, log_storage);
        destroy(&mut log_storage_to_destroy);
        if rc != duckdb_sys::DuckDBSuccess {
            return Err("duckdb_register_log_storage(shell_log_storage) failed".to_string());
        }

        SUPPRESS_LOG_OUTPUT.store(true, Ordering::Relaxed);
        let _ = (|| {
            let sql =
                CString::new("CALL enable_logging(level='warning', storage='shell_log_storage')")
                    .map_err(|_| ())?;
            let mut result: duckdb_sys::duckdb_result = std::mem::zeroed();
            let rc = duckdb_sys::duckdb_query(con, sql.as_ptr(), &mut result);
            duckdb_sys::duckdb_destroy_result(&mut result);
            if rc != duckdb_sys::DuckDBSuccess {
                return Err(());
            }
            Ok(())
        })();
        SUPPRESS_LOG_OUTPUT.store(false, Ordering::Relaxed);
    }
    Ok(())
}
