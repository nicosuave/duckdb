use std::sync::atomic::{AtomicPtr, Ordering};

static CONN: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn install(con: duckdb_sys::duckdb_connection) {
    set_connection(con);
}

pub fn set_connection(con: duckdb_sys::duckdb_connection) {
    CONN.store(con as *mut _, Ordering::Relaxed);
}

pub fn current_connection() -> Option<duckdb_sys::duckdb_connection> {
    let ptr = CONN.load(Ordering::Relaxed);
    if ptr.is_null() {
        None
    } else {
        Some(ptr as duckdb_sys::duckdb_connection)
    }
}
