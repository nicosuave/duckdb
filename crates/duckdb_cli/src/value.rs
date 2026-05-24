use std::ffi::CStr;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use libc::{c_char, localtime_r, time_t, tm};

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn split_epoch_micros(micros: i64) -> (i64, i64) {
    // Shell/parquet tests expect TIMESTAMPTZ formatting quirks for extreme values.
    // DuckDB's shell conversion path effectively operates on epoch milliseconds via a
    // floating-point conversion (ICU expects ms), which loses 1ms precision near i64::MAX.
    // We emulate that behavior here while still handling negative timestamps correctly.
    let micros_sub_ms = micros.rem_euclid(1000);
    let millis = ((micros as f64) / 1000.0).floor() as i64;
    let secs = millis.div_euclid(1000);
    let ms = millis.rem_euclid(1000);
    let us = ms * 1000 + micros_sub_ms;
    (secs, us)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn format_offset(gmtoff_seconds: i64) -> String {
    let sign = if gmtoff_seconds >= 0 { '+' } else { '-' };
    let abs = gmtoff_seconds.abs();
    let hours = abs / 3600;
    let minutes = (abs % 3600) / 60;
    if minutes == 0 {
        format!("{}{:02}", sign, hours)
    } else {
        format!("{}{:02}:{:02}", sign, hours, minutes)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn format_timestamp_tz_local(micros: i64) -> Option<String> {
    let (secs, us) = split_epoch_micros(micros);
    let timep: time_t = secs as time_t;
    let mut out_tm: tm = unsafe { std::mem::zeroed() };
    let rc = unsafe { localtime_r(&timep as *const time_t, &mut out_tm as *mut tm) };
    if rc.is_null() {
        return None;
    }

    let year = out_tm.tm_year as i64 + 1900;
    let month = out_tm.tm_mon as i64 + 1;
    let day = out_tm.tm_mday as i64;
    let hour = out_tm.tm_hour as i64;
    let min = out_tm.tm_min as i64;
    let sec = out_tm.tm_sec as i64;

    let mut s = if year <= 0 {
        // DuckDB renders "BC" dates using "YYYY-MM-DD (BC)" with no year zero.
        let bc_year = 1 - year;
        format!(
            "{:04}-{:02}-{:02} (BC) {:02}:{:02}:{:02}",
            bc_year, month, day, hour, min, sec
        )
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        )
    };
    if us != 0 {
        let mut frac = format!("{:06}", us);
        while frac.ends_with('0') {
            frac.pop();
        }
        s.push('.');
        s.push_str(&frac);
    }
    s.push_str(&format_offset(out_tm.tm_gmtoff as i64));
    Some(s)
}

fn strip_single_quoted_typed_literal(s: &str) -> Option<String> {
    let rest = s.strip_prefix('\'')?;
    let idx = rest.rfind("'::")?;
    Some(rest[..idx].to_string())
}

fn decode_bit_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    let padding_bits = bytes[0] as usize;
    if padding_bits > 7 {
        return None;
    }
    if bytes.len() == 1 {
        // Only the padding byte, no payload.
        return Some(String::new());
    }

    let mut out = String::new();
    // Skip the leading padding bits in the second byte (MSB-first).
    let first_payload = bytes[1];
    for bit in padding_bits..8 {
        let mask = 1u8 << (7 - bit);
        out.push(if (first_payload & mask) != 0 {
            '1'
        } else {
            '0'
        });
    }
    for &b in bytes.iter().skip(2) {
        for bit in 0..8 {
            let mask = 1u8 << (7 - bit);
            out.push(if (b & mask) != 0 { '1' } else { '0' });
        }
    }
    Some(out)
}

fn format_uuid_from_biased_hugeint(upper: i64, lower: u64) -> String {
    // DuckDB stores UUIDs in a signed hugeint with a bias of 2^127.
    let signed = (upper as i128) * (1i128 << 64) + (lower as i128);
    let unbiased = (signed as u128).wrapping_add(1u128 << 127);
    let hex = format!("{:032x}", unbiased);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn vector_row_is_null(vector: duckdb_sys::duckdb_vector, row: u64) -> bool {
    let validity = unsafe { duckdb_sys::duckdb_vector_get_validity(vector) };
    !validity.is_null() && !unsafe { duckdb_sys::duckdb_validity_row_is_valid(validity, row) }
}

fn create_value_from_vector_non_null(
    vector: duckdb_sys::duckdb_vector,
    type_: duckdb_sys::duckdb_logical_type,
    row: u64,
    depth: usize,
) -> duckdb_sys::duckdb_value {
    if depth > 64 {
        return unsafe { duckdb_sys::duckdb_create_null_value() };
    }

    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
    let data = unsafe { duckdb_sys::duckdb_vector_get_data(vector) };
    if data.is_null()
        && type_id != duckdb_sys::DUCKDB_TYPE_STRUCT
        && type_id != duckdb_sys::DUCKDB_TYPE_ARRAY
    {
        return unsafe { duckdb_sys::duckdb_create_null_value() };
    }

    unsafe {
        match type_id {
            duckdb_sys::DUCKDB_TYPE_BOOLEAN => {
                let ptr = data as *const bool;
                duckdb_sys::duckdb_create_bool(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TINYINT => {
                let ptr = data as *const i8;
                duckdb_sys::duckdb_create_int8(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_SMALLINT => {
                let ptr = data as *const i16;
                duckdb_sys::duckdb_create_int16(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_INTEGER => {
                let ptr = data as *const i32;
                duckdb_sys::duckdb_create_int32(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_BIGINT => {
                let ptr = data as *const i64;
                duckdb_sys::duckdb_create_int64(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_UTINYINT => {
                let ptr = data as *const u8;
                duckdb_sys::duckdb_create_uint8(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_USMALLINT => {
                let ptr = data as *const u16;
                duckdb_sys::duckdb_create_uint16(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_UINTEGER => {
                let ptr = data as *const u32;
                duckdb_sys::duckdb_create_uint32(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_UBIGINT => {
                let ptr = data as *const u64;
                duckdb_sys::duckdb_create_uint64(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_FLOAT => {
                let ptr = data as *const f32;
                duckdb_sys::duckdb_create_float(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_DOUBLE => {
                let ptr = data as *const f64;
                duckdb_sys::duckdb_create_double(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_DATE => {
                let ptr = data as *const duckdb_sys::duckdb_date;
                duckdb_sys::duckdb_create_date(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIME => {
                let ptr = data as *const duckdb_sys::duckdb_time;
                duckdb_sys::duckdb_create_time(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIME_NS => {
                let ptr = data as *const duckdb_sys::duckdb_time_ns;
                duckdb_sys::duckdb_create_time_ns(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIME_TZ => {
                let ptr = data as *const duckdb_sys::duckdb_time_tz;
                duckdb_sys::duckdb_create_time_tz_value(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIMESTAMP => {
                let ptr = data as *const duckdb_sys::duckdb_timestamp;
                duckdb_sys::duckdb_create_timestamp(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIMESTAMP_TZ => {
                // Timestamp with time zone is stored as duckdb_timestamp (microseconds).
                let ptr = data as *const duckdb_sys::duckdb_timestamp;
                duckdb_sys::duckdb_create_timestamp_tz(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIMESTAMP_S => {
                let ptr = data as *const duckdb_sys::duckdb_timestamp_s;
                duckdb_sys::duckdb_create_timestamp_s(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIMESTAMP_MS => {
                let ptr = data as *const duckdb_sys::duckdb_timestamp_ms;
                duckdb_sys::duckdb_create_timestamp_ms(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_TIMESTAMP_NS => {
                let ptr = data as *const duckdb_sys::duckdb_timestamp_ns;
                duckdb_sys::duckdb_create_timestamp_ns(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_INTERVAL => {
                let ptr = data as *const duckdb_sys::duckdb_interval;
                duckdb_sys::duckdb_create_interval(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_HUGEINT => {
                let ptr = data as *const duckdb_sys::duckdb_hugeint;
                duckdb_sys::duckdb_create_hugeint(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_UHUGEINT => {
                let ptr = data as *const duckdb_sys::duckdb_uhugeint;
                duckdb_sys::duckdb_create_uhugeint(*ptr.add(row as usize))
            }
            duckdb_sys::DUCKDB_TYPE_UUID => {
                let ptr = data as *const duckdb_sys::duckdb_uhugeint;
                duckdb_sys::duckdb_create_uuid(*ptr.add(row as usize))
            }
	            duckdb_sys::DUCKDB_TYPE_DECIMAL => {
	                let width = duckdb_sys::duckdb_decimal_width(type_);
	                let scale = duckdb_sys::duckdb_decimal_scale(type_);
	                let internal = duckdb_sys::duckdb_decimal_internal_type(type_);
                let value = match internal {
                    duckdb_sys::DUCKDB_TYPE_TINYINT => {
                        let v = *(data as *const i8).add(row as usize) as i128;
                        duckdb_sys::duckdb_hugeint {
                            lower: v as u64,
                            upper: if v < 0 { -1 } else { 0 },
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_SMALLINT => {
                        let v = *(data as *const i16).add(row as usize) as i128;
                        duckdb_sys::duckdb_hugeint {
                            lower: v as u64,
                            upper: if v < 0 { -1 } else { 0 },
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_INTEGER => {
                        let v = *(data as *const i32).add(row as usize) as i128;
                        duckdb_sys::duckdb_hugeint {
                            lower: v as u64,
                            upper: if v < 0 { -1 } else { 0 },
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_BIGINT => {
                        let v = *(data as *const i64).add(row as usize) as i128;
                        duckdb_sys::duckdb_hugeint {
                            lower: v as u64,
                            upper: if v < 0 { -1 } else { 0 },
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_UTINYINT => duckdb_sys::duckdb_hugeint {
                        lower: *(data as *const u8).add(row as usize) as u64,
                        upper: 0,
                    },
                    duckdb_sys::DUCKDB_TYPE_USMALLINT => duckdb_sys::duckdb_hugeint {
                        lower: *(data as *const u16).add(row as usize) as u64,
                        upper: 0,
                    },
                    duckdb_sys::DUCKDB_TYPE_UINTEGER => duckdb_sys::duckdb_hugeint {
                        lower: *(data as *const u32).add(row as usize) as u64,
                        upper: 0,
                    },
                    duckdb_sys::DUCKDB_TYPE_UBIGINT => duckdb_sys::duckdb_hugeint {
                        lower: *(data as *const u64).add(row as usize),
                        upper: 0,
                    },
                    duckdb_sys::DUCKDB_TYPE_HUGEINT => {
                        *(data as *const duckdb_sys::duckdb_hugeint).add(row as usize)
                    }
                    _ => duckdb_sys::duckdb_hugeint { lower: 0, upper: 0 },
                };
	                let dec = duckdb_sys::duckdb_decimal {
	                    width,
	                    scale,
	                    value,
	                };
	                duckdb_sys::duckdb_create_decimal(dec)
		            }
		            duckdb_sys::DUCKDB_TYPE_BIGNUM => {
		                // BIGNUM vectors store the internal "bignum blob" representation inside a duckdb_string_t:
		                //   header (3 bytes) + big-endian magnitude bytes (bitwise-NOT for negative values).
		                // We decode that to (abs_bytes, is_negative) and create a duckdb_bignum value.
		                let ptr = data as *const duckdb_sys::duckdb_string_t;
		                let s_ptr = ptr.add(row as usize);
		                let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as usize;
		                let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const u8;
		                if data_ptr.is_null() || len < 4 {
		                    duckdb_sys::duckdb_create_null_value()
		                } else {
		                    let blob = std::slice::from_raw_parts(data_ptr, len);
		                    let is_negative = (blob[0] & 0x80) == 0;
		                    let mut abs_bytes: Vec<u8> = Vec::with_capacity(len - 3);
		                    if is_negative {
		                        for &b in blob.iter().skip(3) {
		                            abs_bytes.push(!b);
		                        }
		                    } else {
		                        abs_bytes.extend_from_slice(&blob[3..]);
		                    }
		                    let input = duckdb_sys::duckdb_bignum {
		                        data: abs_bytes.as_mut_ptr(),
		                        size: abs_bytes.len() as duckdb_sys::idx_t,
		                        is_negative,
		                    };
		                    duckdb_sys::duckdb_create_bignum(input)
		                }
		            }
		            duckdb_sys::DUCKDB_TYPE_VARCHAR => {
		                let ptr = data as *const duckdb_sys::duckdb_string_t;
		                let s_ptr = ptr.add(row as usize);
		                let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as u64;
                let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const c_char;
                duckdb_sys::duckdb_create_varchar_length(data_ptr, len)
            }
            duckdb_sys::DUCKDB_TYPE_BLOB => {
                let ptr = data as *const duckdb_sys::duckdb_string_t;
                let s_ptr = ptr.add(row as usize);
                let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as u64;
                let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const u8;
                duckdb_sys::duckdb_create_blob(data_ptr, len)
            }
            duckdb_sys::DUCKDB_TYPE_BIT => {
                let ptr = data as *const duckdb_sys::duckdb_string_t;
                let s_ptr = ptr.add(row as usize);
                let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as u64;
                let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *mut u8;
                let bit = duckdb_sys::duckdb_bit {
                    data: data_ptr,
                    size: len,
                };
                duckdb_sys::duckdb_create_bit(bit)
            }
            duckdb_sys::DUCKDB_TYPE_ENUM => {
                let internal = duckdb_sys::duckdb_enum_internal_type(type_);
                let idx: u64 = match internal {
                    duckdb_sys::DUCKDB_TYPE_UTINYINT => {
                        *(data as *const u8).add(row as usize) as u64
                    }
                    duckdb_sys::DUCKDB_TYPE_USMALLINT => {
                        *(data as *const u16).add(row as usize) as u64
                    }
                    duckdb_sys::DUCKDB_TYPE_UINTEGER => {
                        *(data as *const u32).add(row as usize) as u64
                    }
                    duckdb_sys::DUCKDB_TYPE_UBIGINT => *(data as *const u64).add(row as usize),
                    _ => *(data as *const u32).add(row as usize) as u64,
                };
                let dict_ptr = duckdb_sys::duckdb_enum_dictionary_value(type_, idx);
                if dict_ptr.is_null() {
                    duckdb_sys::duckdb_create_null_value()
                } else {
                    let s = CStr::from_ptr(dict_ptr).to_bytes();
                    let v = duckdb_sys::duckdb_create_varchar_length(dict_ptr, s.len() as u64);
                    duckdb_sys::duckdb_free(dict_ptr as *mut _);
                    v
                }
            }
            duckdb_sys::DUCKDB_TYPE_LIST => {
                let entries = data as *const duckdb_sys::duckdb_list_entry;
                let entry = *entries.add(row as usize);

                let child_vector = duckdb_sys::duckdb_list_vector_get_child(vector);
                let mut child_type = duckdb_sys::duckdb_list_type_child_type(type_);

                let mut values: Vec<duckdb_sys::duckdb_value> =
                    Vec::with_capacity(entry.length as usize);
                for i in 0..entry.length {
                    let child_row = entry.offset + i;
                    if vector_row_is_null(child_vector, child_row) {
                        values.push(duckdb_sys::duckdb_create_null_value());
                    } else {
                        values.push(create_value_from_vector_non_null(
                            child_vector,
                            child_type,
                            child_row,
                            depth + 1,
                        ));
                    }
                }

                let list_value = duckdb_sys::duckdb_create_list_value(
                    child_type,
                    values.as_mut_ptr(),
                    entry.length as u64,
                );
                for v in values.iter_mut() {
                    duckdb_sys::duckdb_destroy_value(v);
                }
                duckdb_sys::duckdb_destroy_logical_type(&mut child_type);
                if list_value.is_null() {
                    duckdb_sys::duckdb_create_null_value()
                } else {
                    list_value
                }
            }
            duckdb_sys::DUCKDB_TYPE_ARRAY => {
                let array_size = duckdb_sys::duckdb_array_type_array_size(type_);
                let child_vector = duckdb_sys::duckdb_array_vector_get_child(vector);
                let mut child_type = duckdb_sys::duckdb_array_type_child_type(type_);

                let mut values: Vec<duckdb_sys::duckdb_value> =
                    Vec::with_capacity(array_size as usize);
                for i in 0..array_size {
                    let child_row = row * array_size + i;
                    if vector_row_is_null(child_vector, child_row) {
                        values.push(duckdb_sys::duckdb_create_null_value());
                    } else {
                        values.push(create_value_from_vector_non_null(
                            child_vector,
                            child_type,
                            child_row,
                            depth + 1,
                        ));
                    }
                }
                let array_value = duckdb_sys::duckdb_create_array_value(
                    child_type,
                    values.as_mut_ptr(),
                    array_size as u64,
                );
                for v in values.iter_mut() {
                    duckdb_sys::duckdb_destroy_value(v);
                }
                duckdb_sys::duckdb_destroy_logical_type(&mut child_type);
                if array_value.is_null() {
                    duckdb_sys::duckdb_create_null_value()
                } else {
                    array_value
                }
            }
            duckdb_sys::DUCKDB_TYPE_STRUCT => {
                let child_count = duckdb_sys::duckdb_struct_type_child_count(type_) as usize;
                let mut values: Vec<duckdb_sys::duckdb_value> = Vec::with_capacity(child_count);
                let mut child_types: Vec<duckdb_sys::duckdb_logical_type> =
                    Vec::with_capacity(child_count);
                for idx in 0..child_count {
                    let child_vector =
                        duckdb_sys::duckdb_struct_vector_get_child(vector, idx as u64);
                    let child_type = duckdb_sys::duckdb_struct_type_child_type(type_, idx as u64);
                    let v = if vector_row_is_null(child_vector, row) {
                        duckdb_sys::duckdb_create_null_value()
                    } else {
                        create_value_from_vector_non_null(child_vector, child_type, row, depth + 1)
                    };
                    values.push(v);
                    child_types.push(child_type);
                }
                let struct_value =
                    duckdb_sys::duckdb_create_struct_value(type_, values.as_mut_ptr());
                for v in values.iter_mut() {
                    duckdb_sys::duckdb_destroy_value(v);
                }
                for t in child_types.iter_mut() {
                    duckdb_sys::duckdb_destroy_logical_type(t);
                }
                if struct_value.is_null() {
                    duckdb_sys::duckdb_create_null_value()
                } else {
                    struct_value
                }
            }
            duckdb_sys::DUCKDB_TYPE_MAP => {
                // MAP is represented as a LIST of STRUCT(key, value).
                let entries = data as *const duckdb_sys::duckdb_list_entry;
                let entry = *entries.add(row as usize);

                let child_vector = duckdb_sys::duckdb_list_vector_get_child(vector);
                let key_vector = duckdb_sys::duckdb_struct_vector_get_child(child_vector, 0);
                let value_vector = duckdb_sys::duckdb_struct_vector_get_child(child_vector, 1);

                let mut key_type = duckdb_sys::duckdb_map_type_key_type(type_);
                let mut value_type = duckdb_sys::duckdb_map_type_value_type(type_);

                let mut keys: Vec<duckdb_sys::duckdb_value> =
                    Vec::with_capacity(entry.length as usize);
                let mut values: Vec<duckdb_sys::duckdb_value> =
                    Vec::with_capacity(entry.length as usize);
                for i in 0..entry.length {
                    let child_row = entry.offset + i;
                    let k = if vector_row_is_null(key_vector, child_row) {
                        duckdb_sys::duckdb_create_null_value()
                    } else {
                        create_value_from_vector_non_null(
                            key_vector,
                            key_type,
                            child_row,
                            depth + 1,
                        )
                    };
                    let v = if vector_row_is_null(value_vector, child_row) {
                        duckdb_sys::duckdb_create_null_value()
                    } else {
                        create_value_from_vector_non_null(
                            value_vector,
                            value_type,
                            child_row,
                            depth + 1,
                        )
                    };
                    keys.push(k);
                    values.push(v);
                }

                let map_value = duckdb_sys::duckdb_create_map_value(
                    type_,
                    keys.as_mut_ptr(),
                    values.as_mut_ptr(),
                    entry.length as u64,
                );
                for v in keys.iter_mut() {
                    duckdb_sys::duckdb_destroy_value(v);
                }
                for v in values.iter_mut() {
                    duckdb_sys::duckdb_destroy_value(v);
                }
                duckdb_sys::duckdb_destroy_logical_type(&mut key_type);
                duckdb_sys::duckdb_destroy_logical_type(&mut value_type);

                if map_value.is_null() {
                    duckdb_sys::duckdb_create_null_value()
                } else {
                    map_value
                }
            }
            _ => duckdb_sys::duckdb_create_null_value(),
        }
    }
}

pub fn vector_value_to_string(
    vector: duckdb_sys::duckdb_vector,
    type_: duckdb_sys::duckdb_logical_type,
    row: u64,
) -> Option<String> {
    if vector_row_is_null(vector, row) {
        return None;
    }

    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
    let data = unsafe { duckdb_sys::duckdb_vector_get_data(vector) };
    if data.is_null()
        && type_id != duckdb_sys::DUCKDB_TYPE_STRUCT
        && type_id != duckdb_sys::DUCKDB_TYPE_ARRAY
    {
        return None;
    }

    match type_id {
        duckdb_sys::DUCKDB_TYPE_VARCHAR => unsafe {
            let ptr = data as *const duckdb_sys::duckdb_string_t;
            let s_ptr = ptr.add(row as usize);
            let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as usize;
            let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const u8;
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            // Match shell rendering: print NUL bytes as "\0" so terminal output remains visible.
            if !bytes.contains(&0) {
                return Some(String::from_utf8_lossy(bytes).to_string());
            }
            let mut out = String::with_capacity(bytes.len() + 4);
            let mut start = 0usize;
            for (idx, b) in bytes.iter().copied().enumerate() {
                if b != 0 {
                    continue;
                }
                if start < idx {
                    out.push_str(&String::from_utf8_lossy(&bytes[start..idx]));
                }
                out.push_str("\\0");
                start = idx + 1;
            }
            if start < bytes.len() {
                out.push_str(&String::from_utf8_lossy(&bytes[start..]));
            }
            Some(out)
        },
        duckdb_sys::DUCKDB_TYPE_ENUM => unsafe {
            let internal = duckdb_sys::duckdb_enum_internal_type(type_);
            let idx: u64 = match internal {
                duckdb_sys::DUCKDB_TYPE_UTINYINT => *(data as *const u8).add(row as usize) as u64,
                duckdb_sys::DUCKDB_TYPE_USMALLINT => *(data as *const u16).add(row as usize) as u64,
                duckdb_sys::DUCKDB_TYPE_UINTEGER => *(data as *const u32).add(row as usize) as u64,
                duckdb_sys::DUCKDB_TYPE_UBIGINT => *(data as *const u64).add(row as usize),
                _ => *(data as *const u32).add(row as usize) as u64,
            };
            let dict_ptr = duckdb_sys::duckdb_enum_dictionary_value(type_, idx);
            if dict_ptr.is_null() {
                None
            } else {
                let v = CStr::from_ptr(dict_ptr).to_string_lossy().to_string();
                duckdb_sys::duckdb_free(dict_ptr as *mut _);
                Some(v)
            }
        },
        duckdb_sys::DUCKDB_TYPE_UUID => unsafe {
            // UUIDs are stored as a biased signed hugeint in vectors.
            let ptr = data as *const duckdb_sys::duckdb_hugeint;
            let v = *ptr.add(row as usize);
            Some(format_uuid_from_biased_hugeint(v.upper, v.lower))
        },
        duckdb_sys::DUCKDB_TYPE_BIT => unsafe {
            let ptr = data as *const duckdb_sys::duckdb_string_t;
            let s_ptr = ptr.add(row as usize);
            let len = duckdb_sys::duckdb_string_t_length(*s_ptr) as usize;
            let data_ptr = duckdb_sys::duckdb_string_t_data(s_ptr) as *const u8;
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            decode_bit_bytes(bytes)
        },
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP_TZ => {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            unsafe {
                let ptr = data as *const duckdb_sys::duckdb_timestamp;
                let micros = (*ptr.add(row as usize)).micros;
                if micros == i64::MAX {
                    return Some("infinity".to_string());
                }
                if micros == i64::MIN {
                    return Some("-infinity".to_string());
                }
                format_timestamp_tz_local(micros)
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                let mut value = create_value_from_vector_non_null(vector, type_, row, 0);
                if value.is_null() {
                    None
                } else {
                    let ptr = unsafe { duckdb_sys::duckdb_value_to_string(value) };
                    let mut out = if ptr.is_null() {
                        String::new()
                    } else {
                        unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
                    };
                    if !ptr.is_null() {
                        unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
                    }
                    unsafe { duckdb_sys::duckdb_destroy_value(&mut value) };
                    if let Some(stripped) = strip_single_quoted_typed_literal(out.as_str()) {
                        out = stripped;
                    }
                    Some(out)
                }
            }
        }
        _ => {
            let mut value = create_value_from_vector_non_null(vector, type_, row, 0);
            if value.is_null() {
                return None;
            }
            let ptr = unsafe { duckdb_sys::duckdb_value_to_string(value) };
            let mut out = if ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
            };
            if !ptr.is_null() {
                unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
            }
            unsafe { duckdb_sys::duckdb_destroy_value(&mut value) };

            match type_id {
                duckdb_sys::DUCKDB_TYPE_DATE
                | duckdb_sys::DUCKDB_TYPE_TIME
                | duckdb_sys::DUCKDB_TYPE_TIME_NS
                | duckdb_sys::DUCKDB_TYPE_TIME_TZ
                | duckdb_sys::DUCKDB_TYPE_TIMESTAMP
                | duckdb_sys::DUCKDB_TYPE_TIMESTAMP_S
                | duckdb_sys::DUCKDB_TYPE_TIMESTAMP_MS
                | duckdb_sys::DUCKDB_TYPE_TIMESTAMP_NS => {
                    if let Some(stripped) = strip_single_quoted_typed_literal(out.as_str()) {
                        out = stripped;
                    }
                }
                duckdb_sys::DUCKDB_TYPE_UUID
                | duckdb_sys::DUCKDB_TYPE_INTERVAL
                | duckdb_sys::DUCKDB_TYPE_BLOB
                | duckdb_sys::DUCKDB_TYPE_BIT => {
                    if let Some(stripped) = strip_single_quoted_typed_literal(out.as_str()) {
                        out = stripped;
                    }
                }
                _ => {}
            }

            Some(out)
        }
    }
}
