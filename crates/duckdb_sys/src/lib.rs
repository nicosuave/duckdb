#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::os::raw::{c_char, c_int, c_void};

// NOTE: This is intentionally a "normalized" version (major.minor.patch),
// matching the prefix extracted from duckdb_library_version().
pub const DUCKDB_TARGET_VERSION: &str = "1.5.3";

pub type duckdb_database = *mut c_void;
pub type duckdb_connection = *mut c_void;
pub type duckdb_prepared_statement = *mut c_void;
pub type duckdb_extracted_statements = *mut c_void;
pub type duckdb_config = *mut c_void;
pub type duckdb_function_info = *mut c_void;
pub type duckdb_bind_info = *mut c_void;
pub type duckdb_client_context = *mut c_void;
pub type duckdb_scalar_function = *mut c_void;
pub type duckdb_replacement_scan_info = *mut c_void;
pub type duckdb_log_storage = *mut c_void;

pub type duckdb_scalar_function_bind_t = Option<unsafe extern "C" fn(info: duckdb_bind_info)>;
pub type duckdb_scalar_function_t = Option<
    unsafe extern "C" fn(
        info: duckdb_function_info,
        input: duckdb_data_chunk,
        output: duckdb_vector,
    ),
>;
pub type duckdb_delete_callback_t = Option<unsafe extern "C" fn(data: *mut c_void)>;
pub type duckdb_replacement_callback_t = Option<
    unsafe extern "C" fn(
        info: duckdb_replacement_scan_info,
        table_name: *const c_char,
        data: *mut c_void,
    ),
>;
pub type duckdb_logger_write_log_entry_t = Option<
    unsafe extern "C" fn(
        extra_data: *mut c_void,
        timestamp: *mut duckdb_timestamp,
        level: *const c_char,
        log_type: *const c_char,
        log_message: *const c_char,
    ),
>;
#[repr(C)]
pub struct duckdb_column {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_result {
    pub deprecated_column_count: idx_t,
    pub deprecated_row_count: idx_t,
    pub deprecated_rows_changed: idx_t,
    pub deprecated_columns: *mut duckdb_column,
    pub deprecated_error_message: *mut c_char,
    pub internal_data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_string_t {
    pub value: [u8; 16],
}
pub type duckdb_data_chunk = *mut c_void;
pub type duckdb_vector = *mut c_void;
pub type duckdb_logical_type = *mut c_void;
pub type duckdb_value = *mut c_void;

pub type idx_t = u64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_date {
    pub days: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_time {
    pub micros: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_time_ns {
    pub nanos: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_time_tz {
    pub bits: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_timestamp {
    pub micros: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_timestamp_s {
    pub seconds: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_timestamp_ms {
    pub millis: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_timestamp_ns {
    pub nanos: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_interval {
    pub months: i32,
    pub days: i32,
    pub micros: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_hugeint {
    pub lower: u64,
    pub upper: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_uhugeint {
    pub lower: u64,
    pub upper: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_decimal {
    pub width: u8,
    pub scale: u8,
    pub value: duckdb_hugeint,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct duckdb_list_entry {
    pub offset: u64,
    pub length: u64,
}

pub type duckdb_type = u32;
pub const DUCKDB_TYPE_INVALID: duckdb_type = 0;
pub const DUCKDB_TYPE_BOOLEAN: duckdb_type = 1;
pub const DUCKDB_TYPE_TINYINT: duckdb_type = 2;
pub const DUCKDB_TYPE_SMALLINT: duckdb_type = 3;
pub const DUCKDB_TYPE_INTEGER: duckdb_type = 4;
pub const DUCKDB_TYPE_BIGINT: duckdb_type = 5;
pub const DUCKDB_TYPE_UTINYINT: duckdb_type = 6;
pub const DUCKDB_TYPE_USMALLINT: duckdb_type = 7;
pub const DUCKDB_TYPE_UINTEGER: duckdb_type = 8;
pub const DUCKDB_TYPE_UBIGINT: duckdb_type = 9;
pub const DUCKDB_TYPE_FLOAT: duckdb_type = 10;
pub const DUCKDB_TYPE_DOUBLE: duckdb_type = 11;
pub const DUCKDB_TYPE_TIMESTAMP: duckdb_type = 12;
pub const DUCKDB_TYPE_DATE: duckdb_type = 13;
pub const DUCKDB_TYPE_TIME: duckdb_type = 14;
pub const DUCKDB_TYPE_INTERVAL: duckdb_type = 15;
pub const DUCKDB_TYPE_HUGEINT: duckdb_type = 16;
pub const DUCKDB_TYPE_VARCHAR: duckdb_type = 17;
pub const DUCKDB_TYPE_BLOB: duckdb_type = 18;
pub const DUCKDB_TYPE_DECIMAL: duckdb_type = 19;
pub const DUCKDB_TYPE_TIMESTAMP_S: duckdb_type = 20;
pub const DUCKDB_TYPE_TIMESTAMP_MS: duckdb_type = 21;
pub const DUCKDB_TYPE_TIMESTAMP_NS: duckdb_type = 22;
pub const DUCKDB_TYPE_ENUM: duckdb_type = 23;
pub const DUCKDB_TYPE_LIST: duckdb_type = 24;
pub const DUCKDB_TYPE_STRUCT: duckdb_type = 25;
pub const DUCKDB_TYPE_MAP: duckdb_type = 26;
pub const DUCKDB_TYPE_UUID: duckdb_type = 27;
pub const DUCKDB_TYPE_UNION: duckdb_type = 28;
pub const DUCKDB_TYPE_BIT: duckdb_type = 29;
pub const DUCKDB_TYPE_TIME_TZ: duckdb_type = 30;
pub const DUCKDB_TYPE_TIMESTAMP_TZ: duckdb_type = 31;
pub const DUCKDB_TYPE_UHUGEINT: duckdb_type = 32;
pub const DUCKDB_TYPE_ARRAY: duckdb_type = 33;
pub const DUCKDB_TYPE_ANY: duckdb_type = 34;
pub const DUCKDB_TYPE_BIGNUM: duckdb_type = 35;
pub const DUCKDB_TYPE_SQLNULL: duckdb_type = 36;
pub const DUCKDB_TYPE_TIME_NS: duckdb_type = 39;

pub type duckdb_state = c_int;
pub const DuckDBSuccess: duckdb_state = 0;
pub const DuckDBError: duckdb_state = 1;

pub type duckdb_result_type = u32;
pub const DUCKDB_RESULT_TYPE_INVALID: duckdb_result_type = 0;
pub const DUCKDB_RESULT_TYPE_CHANGED_ROWS: duckdb_result_type = 1;
pub const DUCKDB_RESULT_TYPE_NOTHING: duckdb_result_type = 2;
pub const DUCKDB_RESULT_TYPE_QUERY_RESULT: duckdb_result_type = 3;

pub type duckdb_statement_type = u32;
pub const DUCKDB_STATEMENT_TYPE_INVALID: duckdb_statement_type = 0;
pub const DUCKDB_STATEMENT_TYPE_SELECT: duckdb_statement_type = 1;
pub const DUCKDB_STATEMENT_TYPE_INSERT: duckdb_statement_type = 2;
pub const DUCKDB_STATEMENT_TYPE_UPDATE: duckdb_statement_type = 3;
pub const DUCKDB_STATEMENT_TYPE_EXPLAIN: duckdb_statement_type = 4;
pub const DUCKDB_STATEMENT_TYPE_DELETE: duckdb_statement_type = 5;
pub const DUCKDB_STATEMENT_TYPE_PREPARE: duckdb_statement_type = 6;
pub const DUCKDB_STATEMENT_TYPE_CREATE: duckdb_statement_type = 7;
pub const DUCKDB_STATEMENT_TYPE_EXECUTE: duckdb_statement_type = 8;
pub const DUCKDB_STATEMENT_TYPE_ALTER: duckdb_statement_type = 9;
pub const DUCKDB_STATEMENT_TYPE_TRANSACTION: duckdb_statement_type = 10;
pub const DUCKDB_STATEMENT_TYPE_COPY: duckdb_statement_type = 11;
pub const DUCKDB_STATEMENT_TYPE_ANALYZE: duckdb_statement_type = 12;
pub const DUCKDB_STATEMENT_TYPE_VARIABLE_SET: duckdb_statement_type = 13;
pub const DUCKDB_STATEMENT_TYPE_CREATE_FUNC: duckdb_statement_type = 14;
pub const DUCKDB_STATEMENT_TYPE_DROP: duckdb_statement_type = 15;
pub const DUCKDB_STATEMENT_TYPE_EXPORT: duckdb_statement_type = 16;
pub const DUCKDB_STATEMENT_TYPE_PRAGMA: duckdb_statement_type = 17;
pub const DUCKDB_STATEMENT_TYPE_VACUUM: duckdb_statement_type = 18;
pub const DUCKDB_STATEMENT_TYPE_CALL: duckdb_statement_type = 19;
pub const DUCKDB_STATEMENT_TYPE_SET: duckdb_statement_type = 20;
pub const DUCKDB_STATEMENT_TYPE_LOAD: duckdb_statement_type = 21;
pub const DUCKDB_STATEMENT_TYPE_RELATION: duckdb_statement_type = 22;
pub const DUCKDB_STATEMENT_TYPE_EXTENSION: duckdb_statement_type = 23;
pub const DUCKDB_STATEMENT_TYPE_LOGICAL_PLAN: duckdb_statement_type = 24;
pub const DUCKDB_STATEMENT_TYPE_ATTACH: duckdb_statement_type = 25;
pub const DUCKDB_STATEMENT_TYPE_DETACH: duckdb_statement_type = 26;
pub const DUCKDB_STATEMENT_TYPE_MULTI: duckdb_statement_type = 27;

#[repr(C)]
pub struct duckdb_blob {
    pub data: *mut c_void,
    pub size: idx_t,
}

#[repr(C)]
pub struct duckdb_bit {
    pub data: *mut u8,
    pub size: idx_t,
}

#[repr(C)]
pub struct duckdb_bignum {
    pub data: *mut u8,
    pub size: idx_t,
    pub is_negative: bool,
}

#[repr(C)]
pub struct duckdb_string {
    pub data: *mut c_char,
    pub size: idx_t,
}

extern "C" {
    pub fn duckdb_library_version() -> *const c_char;

    pub fn duckdb_open(path: *const c_char, out_db: *mut duckdb_database) -> duckdb_state;
    pub fn duckdb_open_ext(
        path: *const c_char,
        out_db: *mut duckdb_database,
        config: duckdb_config,
        out_error: *mut *mut c_char,
    ) -> duckdb_state;
    pub fn duckdb_close(database: *mut duckdb_database);

    pub fn duckdb_connect(
        database: duckdb_database,
        out_connection: *mut duckdb_connection,
    ) -> duckdb_state;
    pub fn duckdb_disconnect(connection: *mut duckdb_connection);

    pub fn duckdb_interrupt(connection: duckdb_connection);

    pub fn duckdb_create_config(out_config: *mut duckdb_config) -> duckdb_state;
    pub fn duckdb_set_config(
        config: duckdb_config,
        name: *const c_char,
        option: *const c_char,
    ) -> duckdb_state;
    pub fn duckdb_destroy_config(config: *mut duckdb_config);

    pub fn duckdb_prepare(
        connection: duckdb_connection,
        query: *const c_char,
        out_prepared_statement: *mut duckdb_prepared_statement,
    ) -> duckdb_state;
    pub fn duckdb_destroy_prepare(prepared_statement: *mut duckdb_prepared_statement);
    pub fn duckdb_prepare_error(prepared_statement: duckdb_prepared_statement) -> *const c_char;
    pub fn duckdb_prepared_statement_type(
        statement: duckdb_prepared_statement,
    ) -> duckdb_statement_type;

    pub fn duckdb_execute_prepared_streaming(
        prepared_statement: duckdb_prepared_statement,
        out_result: *mut duckdb_result,
    ) -> duckdb_state;

    pub fn duckdb_execute_prepared(
        prepared_statement: duckdb_prepared_statement,
        out_result: *mut duckdb_result,
    ) -> duckdb_state;

    pub fn duckdb_destroy_result(result: *mut duckdb_result);

    pub fn duckdb_result_return_type(result: duckdb_result) -> duckdb_result_type;
    pub fn duckdb_rows_changed(result: *mut duckdb_result) -> idx_t;

    pub fn duckdb_fetch_chunk(result: duckdb_result) -> duckdb_data_chunk;
    pub fn duckdb_destroy_data_chunk(chunk: *mut duckdb_data_chunk);

    pub fn duckdb_data_chunk_get_size(chunk: duckdb_data_chunk) -> idx_t;
    pub fn duckdb_data_chunk_get_column_count(chunk: duckdb_data_chunk) -> idx_t;
    pub fn duckdb_data_chunk_get_vector(chunk: duckdb_data_chunk, col_idx: idx_t) -> duckdb_vector;

    pub fn duckdb_vector_get_data(vector: duckdb_vector) -> *mut c_void;
    pub fn duckdb_vector_get_validity(vector: duckdb_vector) -> *mut u64;
    pub fn duckdb_validity_row_is_valid(validity: *mut u64, row: idx_t) -> bool;

    pub fn duckdb_string_t_length(string: duckdb_string_t) -> u32;
    pub fn duckdb_string_t_data(string: *const duckdb_string_t) -> *const c_char;

    pub fn duckdb_vector_get_column_type(vector: duckdb_vector) -> duckdb_logical_type;
    pub fn duckdb_create_logical_type(type_: duckdb_type) -> duckdb_logical_type;
    pub fn duckdb_destroy_logical_type(type_: *mut duckdb_logical_type);

    pub fn duckdb_create_scalar_function() -> duckdb_scalar_function;
    pub fn duckdb_destroy_scalar_function(scalar_function: *mut duckdb_scalar_function);
    pub fn duckdb_scalar_function_set_name(
        scalar_function: duckdb_scalar_function,
        name: *const c_char,
    );
    pub fn duckdb_scalar_function_set_bind(
        scalar_function: duckdb_scalar_function,
        bind: duckdb_scalar_function_bind_t,
    );
    pub fn duckdb_scalar_function_set_volatile(scalar_function: duckdb_scalar_function);
    pub fn duckdb_scalar_function_add_parameter(
        scalar_function: duckdb_scalar_function,
        type_: duckdb_logical_type,
    );
    pub fn duckdb_scalar_function_set_return_type(
        scalar_function: duckdb_scalar_function,
        type_: duckdb_logical_type,
    );
    pub fn duckdb_scalar_function_set_function(
        scalar_function: duckdb_scalar_function,
        function: duckdb_scalar_function_t,
    );
    pub fn duckdb_register_scalar_function(
        con: duckdb_connection,
        scalar_function: duckdb_scalar_function,
    ) -> duckdb_state;
    pub fn duckdb_scalar_function_get_client_context(
        info: duckdb_bind_info,
        out_context: *mut duckdb_client_context,
    );
    pub fn duckdb_scalar_function_bind_set_error(info: duckdb_bind_info, error: *const c_char);
    pub fn duckdb_scalar_function_set_error(info: duckdb_function_info, error: *const c_char);

    pub fn duckdb_destroy_client_context(context: *mut duckdb_client_context);

    pub fn duckdb_vector_assign_string_element(
        vector: duckdb_vector,
        index: idx_t,
        str_: *const c_char,
    );
    pub fn duckdb_vector_assign_string_element_len(
        vector: duckdb_vector,
        index: idx_t,
        str_: *const c_char,
        len: idx_t,
    );

    pub fn duckdb_list_vector_get_child(vector: duckdb_vector) -> duckdb_vector;
    pub fn duckdb_struct_vector_get_child(vector: duckdb_vector, index: idx_t) -> duckdb_vector;
    pub fn duckdb_array_vector_get_child(vector: duckdb_vector) -> duckdb_vector;

    pub fn duckdb_destroy_value(value: *mut duckdb_value);
    pub fn duckdb_value_to_string(value: duckdb_value) -> *mut c_char;
    pub fn duckdb_free(ptr: *mut c_void);

    pub fn duckdb_create_null_value() -> duckdb_value;
    pub fn duckdb_create_bool(input: bool) -> duckdb_value;
    pub fn duckdb_create_int8(input: i8) -> duckdb_value;
    pub fn duckdb_create_uint8(input: u8) -> duckdb_value;
    pub fn duckdb_create_int16(input: i16) -> duckdb_value;
    pub fn duckdb_create_uint16(input: u16) -> duckdb_value;
    pub fn duckdb_create_int32(input: i32) -> duckdb_value;
    pub fn duckdb_create_uint32(input: u32) -> duckdb_value;
    pub fn duckdb_create_int64(val: i64) -> duckdb_value;
    pub fn duckdb_create_uint64(input: u64) -> duckdb_value;
    pub fn duckdb_create_hugeint(input: duckdb_hugeint) -> duckdb_value;
    pub fn duckdb_create_uhugeint(input: duckdb_uhugeint) -> duckdb_value;
    pub fn duckdb_create_decimal(input: duckdb_decimal) -> duckdb_value;
    pub fn duckdb_create_float(input: f32) -> duckdb_value;
    pub fn duckdb_create_double(input: f64) -> duckdb_value;
    pub fn duckdb_create_date(input: duckdb_date) -> duckdb_value;
    pub fn duckdb_create_time(input: duckdb_time) -> duckdb_value;
    pub fn duckdb_create_time_ns(input: duckdb_time_ns) -> duckdb_value;
    pub fn duckdb_create_time_tz_value(value: duckdb_time_tz) -> duckdb_value;
    pub fn duckdb_create_timestamp(input: duckdb_timestamp) -> duckdb_value;
    pub fn duckdb_create_timestamp_tz(input: duckdb_timestamp) -> duckdb_value;
    pub fn duckdb_create_timestamp_s(input: duckdb_timestamp_s) -> duckdb_value;
    pub fn duckdb_create_timestamp_ms(input: duckdb_timestamp_ms) -> duckdb_value;
    pub fn duckdb_create_timestamp_ns(input: duckdb_timestamp_ns) -> duckdb_value;
    pub fn duckdb_create_interval(input: duckdb_interval) -> duckdb_value;
    pub fn duckdb_create_blob(data: *const u8, length: idx_t) -> duckdb_value;
    pub fn duckdb_create_bit(input: duckdb_bit) -> duckdb_value;
    pub fn duckdb_create_bignum(input: duckdb_bignum) -> duckdb_value;
    pub fn duckdb_create_uuid(input: duckdb_uhugeint) -> duckdb_value;
    pub fn duckdb_create_varchar_length(text: *const c_char, length: idx_t) -> duckdb_value;

    pub fn duckdb_create_struct_value(
        type_: duckdb_logical_type,
        values: *mut duckdb_value,
    ) -> duckdb_value;
    pub fn duckdb_create_list_value(
        type_: duckdb_logical_type,
        values: *mut duckdb_value,
        value_count: idx_t,
    ) -> duckdb_value;
    pub fn duckdb_create_array_value(
        type_: duckdb_logical_type,
        values: *mut duckdb_value,
        value_count: idx_t,
    ) -> duckdb_value;
    pub fn duckdb_create_map_value(
        map_type: duckdb_logical_type,
        keys: *mut duckdb_value,
        values: *mut duckdb_value,
        entry_count: idx_t,
    ) -> duckdb_value;

    pub fn duckdb_query(
        connection: duckdb_connection,
        query: *const c_char,
        out_result: *mut duckdb_result,
    ) -> duckdb_state;
    pub fn duckdb_result_error(result: *mut duckdb_result) -> *const c_char;
    pub fn duckdb_column_count(result: *mut duckdb_result) -> idx_t;
    pub fn duckdb_row_count(result: *mut duckdb_result) -> idx_t;
    pub fn duckdb_column_type(result: *mut duckdb_result, col: idx_t) -> duckdb_type;
    pub fn duckdb_result_statement_type(result: duckdb_result) -> duckdb_statement_type;
    pub fn duckdb_column_logical_type(
        result: *mut duckdb_result,
        col: idx_t,
    ) -> duckdb_logical_type;
    pub fn duckdb_column_name(result: *mut duckdb_result, col: idx_t) -> *const c_char;
    pub fn duckdb_value_is_null(result: *mut duckdb_result, col: idx_t, row: idx_t) -> bool;
    pub fn duckdb_value_varchar(result: *mut duckdb_result, col: idx_t, row: idx_t) -> *mut c_char;
    pub fn duckdb_value_string(result: *mut duckdb_result, col: idx_t, row: idx_t)
        -> duckdb_string;
    pub fn duckdb_value_blob(result: *mut duckdb_result, col: idx_t, row: idx_t) -> duckdb_blob;

    pub fn duckdb_get_type_id(type_: duckdb_logical_type) -> duckdb_type;
    pub fn duckdb_logical_type_get_alias(type_: duckdb_logical_type) -> *mut c_char;
    pub fn duckdb_decimal_width(type_: duckdb_logical_type) -> u8;
    pub fn duckdb_decimal_scale(type_: duckdb_logical_type) -> u8;
    pub fn duckdb_decimal_internal_type(type_: duckdb_logical_type) -> duckdb_type;
    pub fn duckdb_enum_dictionary_size(type_: duckdb_logical_type) -> u32;
    pub fn duckdb_enum_internal_type(type_: duckdb_logical_type) -> duckdb_type;
    pub fn duckdb_enum_dictionary_value(type_: duckdb_logical_type, index: idx_t) -> *mut c_char;
    pub fn duckdb_list_type_child_type(type_: duckdb_logical_type) -> duckdb_logical_type;
    pub fn duckdb_array_type_child_type(type_: duckdb_logical_type) -> duckdb_logical_type;
    pub fn duckdb_array_type_array_size(type_: duckdb_logical_type) -> idx_t;
    pub fn duckdb_map_type_key_type(type_: duckdb_logical_type) -> duckdb_logical_type;
    pub fn duckdb_map_type_value_type(type_: duckdb_logical_type) -> duckdb_logical_type;
    pub fn duckdb_struct_type_child_count(type_: duckdb_logical_type) -> idx_t;
    pub fn duckdb_struct_type_child_name(type_: duckdb_logical_type, index: idx_t) -> *mut c_char;
    pub fn duckdb_struct_type_child_type(
        type_: duckdb_logical_type,
        index: idx_t,
    ) -> duckdb_logical_type;
    pub fn duckdb_union_type_member_count(type_: duckdb_logical_type) -> idx_t;
    pub fn duckdb_union_type_member_name(type_: duckdb_logical_type, index: idx_t) -> *mut c_char;
    pub fn duckdb_union_type_member_type(
        type_: duckdb_logical_type,
        index: idx_t,
    ) -> duckdb_logical_type;

    pub fn duckdb_extract_statements(
        connection: duckdb_connection,
        query: *const c_char,
        out_extracted_statements: *mut duckdb_extracted_statements,
    ) -> idx_t;
    pub fn duckdb_prepare_extracted_statement(
        connection: duckdb_connection,
        extracted_statements: duckdb_extracted_statements,
        index: idx_t,
        out_prepared_statement: *mut duckdb_prepared_statement,
    ) -> duckdb_state;
    pub fn duckdb_extract_statements_error(
        extracted_statements: duckdb_extracted_statements,
    ) -> *const c_char;
    pub fn duckdb_destroy_extracted(extracted_statements: *mut duckdb_extracted_statements);

    pub fn duckdb_add_replacement_scan(
        db: duckdb_database,
        replacement: duckdb_replacement_callback_t,
        extra_data: *mut c_void,
        delete_callback: duckdb_delete_callback_t,
    );
    pub fn duckdb_replacement_scan_set_function_name(
        info: duckdb_replacement_scan_info,
        function_name: *const c_char,
    );
    pub fn duckdb_replacement_scan_add_parameter(
        info: duckdb_replacement_scan_info,
        parameter: duckdb_value,
    );
    pub fn duckdb_replacement_scan_set_error(
        info: duckdb_replacement_scan_info,
        error: *const c_char,
    );

    pub fn duckdb_create_log_storage() -> duckdb_log_storage;
    pub fn duckdb_destroy_log_storage(log_storage: *mut duckdb_log_storage);
    pub fn duckdb_log_storage_set_write_log_entry(
        log_storage: duckdb_log_storage,
        function: duckdb_logger_write_log_entry_t,
    );
    pub fn duckdb_log_storage_set_extra_data(
        log_storage: duckdb_log_storage,
        extra_data: *mut c_void,
        delete_callback: duckdb_delete_callback_t,
    );
    pub fn duckdb_log_storage_set_name(log_storage: duckdb_log_storage, name: *const c_char);
    pub fn duckdb_register_log_storage(
        database: duckdb_database,
        log_storage: duckdb_log_storage,
    ) -> duckdb_state;
}
