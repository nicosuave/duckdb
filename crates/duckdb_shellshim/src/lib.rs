#![allow(non_camel_case_types)]

use std::os::raw::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;

pub const DUCKDB_TARGET_VERSION: &str = "1.4.3";

#[repr(C)]
pub struct duckdb_shellshim_duckbox_config {
    pub max_rows: u64,
    pub max_width: u64,
    pub max_analyze_rows: u64,
    pub null_value: *const c_char,
    pub columns: bool,
    pub decimal_separator: u8,
    pub thousand_separator: u8,
    pub large_number_rendering: c_int,
    pub stdout_is_console: bool,
    pub output_is_file: bool,
    pub highlight_results: bool,
    pub ansi_column_name: *const c_char,
    pub ansi_column_type: *const c_char,
    pub ansi_null_value: *const c_char,
    pub ansi_reset: *const c_char,
}

extern "C" {
    pub fn duckdb_shellshim_target_version() -> *const c_char;
    pub fn duckdb_shellshim_library_version() -> *const c_char;
    pub fn duckdb_shellshim_release_codename() -> *const c_char;
    pub fn duckdb_shellshim_source_id() -> *const c_char;
    pub fn duckdb_shellshim_keyword_check(str_: *const c_char, len: usize) -> c_int;

    pub fn duckdb_shellshim_echo_slices_from_extracted(
        extracted_statements: *mut c_void,
        query: *const c_char,
        out_slices: *mut *mut *mut c_char,
        out_count: *mut usize,
        out_error: *mut *mut c_char,
    ) -> c_int;

    pub fn duckdb_shellshim_free_echo_slices(slices: *mut *mut c_char, count: usize);

    pub fn duckdb_shellshim_render_duckbox(
        connection: *mut c_void,
        query: *const c_char,
        cfg: *const duckdb_shellshim_duckbox_config,
        out_rendered: *mut *mut c_char,
        out_error: *mut *mut c_char,
    ) -> c_int;

    pub fn duckdb_shellshim_cast_chunk_to_varchar(
        connection: *mut c_void,
        chunk: *mut c_void,
        out_chunk: *mut *mut c_void,
        out_error: *mut *mut c_char,
    ) -> c_int;
}
