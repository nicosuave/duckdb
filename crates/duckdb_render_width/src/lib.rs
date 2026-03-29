use std::os::raw::{c_char, c_int};

extern "C" {
    fn duckdb_cli_compute_render_width(buf: *const c_char, len: usize) -> usize;
    fn duckdb_cli_get_render_position(
        buf: *const c_char,
        len: usize,
        max_width: c_int,
        n: *mut c_int,
    ) -> c_int;
    fn duckdb_cli_compute_render_width_duckbox(buf: *const c_char, len: usize) -> usize;
    fn duckdb_cli_get_render_position_duckbox(
        buf: *const c_char,
        len: usize,
        max_width: c_int,
        n: *mut c_int,
    ) -> c_int;
}

pub fn compute_render_width(bytes: &[u8]) -> usize {
    unsafe { duckdb_cli_compute_render_width(bytes.as_ptr() as *const c_char, bytes.len()) }
}

pub fn compute_render_width_duckbox(bytes: &[u8]) -> usize {
    unsafe { duckdb_cli_compute_render_width_duckbox(bytes.as_ptr() as *const c_char, bytes.len()) }
}

/// Equivalent to DuckDB's linenoise `GetRenderPosition`:
/// returns the byte offset where `max_width` would be exceeded, and writes the
/// render width (<= `max_width`) to `out_n`.
pub fn get_render_position(bytes: &[u8], max_width: usize, out_n: &mut i32) -> i32 {
    unsafe {
        duckdb_cli_get_render_position(
            bytes.as_ptr() as *const c_char,
            bytes.len(),
            max_width as c_int,
            out_n,
        )
    }
}

pub fn get_render_position_duckbox(bytes: &[u8], max_width: usize, out_n: &mut i32) -> i32 {
    unsafe {
        duckdb_cli_get_render_position_duckbox(
            bytes.as_ptr() as *const c_char,
            bytes.len(),
            max_width as c_int,
            out_n,
        )
    }
}
