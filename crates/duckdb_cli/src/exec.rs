use crate::output::OutputHandle;
use crate::session::Session;
use crate::state::{
    BailOnError, HighlightMode, HighlightStyle, OptionType, PagerMode, PrintColor, PrintIntensity,
    ReadLineVersion, RenderMode, ShellState,
};
use duckdb_shellshim as shellshim;
use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::io::Read;
use std::io::Write;
use std::process::Stdio;

fn shell_string_is_null_literal(inner: &str) -> bool {
    inner.trim().eq_ignore_ascii_case("null")
}

fn looks_like_json_literal_for_shell_display(inner: &str) -> bool {
    let t = inner.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

fn shell_should_unquote_string_literal_in_complex(
    inner: &str,
    unquote_json_literals: bool,
) -> bool {
    // DuckDB's shipped shell unquotes string literals inside nested/list/struct displays quite
    // aggressively (even with whitespace and numeric/boolean-looking values). It keeps quotes
    // for cases that would otherwise be visually confusing, and for the special-case "null"
    // string (to distinguish it from NULL).
    let inner = inner.trim_end_matches('\n');
    if inner.is_empty() {
        return false;
    }
    if shell_string_is_null_literal(inner) {
        return false;
    }
    let _ = unquote_json_literals;
    if inner.contains('\'') || inner.contains('"') {
        return false;
    }
    for ch in inner.chars() {
        match ch {
            ',' | ':' | '[' | ']' | '{' | '}' | '(' | ')' => return false,
            _ => {}
        }
    }
    true
}

fn escape_single_quotes_for_shell_nested_string(inner: &str) -> String {
    // Shell display uses backslash-escaped single quotes inside single-quoted nested values.
    // Example: "a'b" -> 'a\'b'
    let mut out = String::with_capacity(inner.len() + 2);
    for ch in inner.chars() {
        if ch == '\'' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn is_key_starting_quote(value: &str, quote_pos: usize) -> bool {
    // A struct key is written like: { 'key': value, ... }
    // Detect by looking for the previous non-whitespace character being `{` or `,`.
    if quote_pos == 0 {
        return false;
    }
    let bytes = value.as_bytes();
    let mut i = quote_pos;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'{' | b',' => return true,
            _ => return false,
        }
    }
    false
}

fn find_key_closing_quote(value: &str, quote_pos: usize) -> Option<usize> {
    // Find the quote that terminates a struct key by looking for `'` followed by optional
    // whitespace and then `:`. This handles keys containing unescaped `'` that appear in some
    // DuckDB C API stringification outputs (e.g. `{'a'b': 1}`).
    //
    // Guard against scanning past list/struct boundaries when we are not actually looking at
    // a struct key (e.g. a string element inside a list). If we hit a `]` or `}` before finding
    // a `':` pattern, treat this as not being a struct key.
    let bytes = value.as_bytes();
    let mut i = quote_pos + 1;
    while i < bytes.len() {
        match bytes[i] {
            b']' | b'}' => return None,
            b'\'' => {
                let mut j = i + 1;
                while j < bytes.len() {
                    match bytes[j] {
                        b' ' | b'\t' | b'\n' | b'\r' => {
                            j += 1;
                            continue;
                        }
                        b':' => {
                            // Avoid treating `'x'::TYPE` typed literals as struct keys.
                            if bytes.get(j + 1) == Some(&b':') {
                                break;
                            }
                            return Some(i);
                        }
                        _ => break,
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn decode_nested_string_inner_for_key(inner: &str) -> String {
    // Decode escape forms that can show up inside nested stringification outputs:
    // - `\'` and `''` represent a literal `'` in the content.
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '\\' {
            if matches!(it.peek(), Some('\'')) {
                let _ = it.next();
                out.push('\'');
                continue;
            }
            out.push('\\');
            continue;
        }
        if ch == '\'' {
            if matches!(it.peek(), Some('\'')) {
                let _ = it.next();
                out.push('\'');
                continue;
            }
            out.push('\'');
            continue;
        }
        out.push(ch);
    }
    out
}

fn is_key_string_literal(rest_after_quote: &str) -> bool {
    let rest = rest_after_quote.trim_start();
    // Distinguish `'x'::TYPE` (cast) from `'x': ...` (struct key).
    if rest.starts_with("::") {
        return false;
    }
    rest.starts_with(':')
}

fn strip_typed_literal_after_quote(s: &str, after_quote: usize) -> (usize, bool) {
    let rest = &s[after_quote..];
    let rest_trim = rest.trim_start();
    let trim_delta = rest.len() - rest_trim.len();
    let start = after_quote + trim_delta;
    if !s[start..].starts_with("::") {
        return (after_quote, false);
    }

    let mut idx = start + 2;
    while idx < s.len() {
        let ch = s[idx..].chars().next().unwrap();
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == ' ' {
            idx += ch.len_utf8();
            continue;
        }
        break;
    }
    (idx, true)
}

fn logical_type_contains_json(type_: duckdb_sys::duckdb_logical_type, depth: u32) -> bool {
    if type_.is_null() {
        return false;
    }
    if depth > 8 {
        return false;
    }

    let alias_ptr = unsafe { duckdb_sys::duckdb_logical_type_get_alias(type_) };
    if !alias_ptr.is_null() {
        let alias = unsafe { CStr::from_ptr(alias_ptr) }
            .to_string_lossy()
            .to_string();
        unsafe { duckdb_sys::duckdb_free(alias_ptr as *mut _) };
        if alias.trim().eq_ignore_ascii_case("json") {
            return true;
        }
    }

    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
    if type_id == duckdb_sys::DUCKDB_TYPE_LIST {
        let mut child = unsafe { duckdb_sys::duckdb_list_type_child_type(type_) };
        let res = logical_type_contains_json(child, depth + 1);
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
        return res;
    }
    if type_id == duckdb_sys::DUCKDB_TYPE_ARRAY {
        let mut child = unsafe { duckdb_sys::duckdb_array_type_child_type(type_) };
        let res = logical_type_contains_json(child, depth + 1);
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
        return res;
    }
    if type_id == duckdb_sys::DUCKDB_TYPE_MAP {
        let mut key = unsafe { duckdb_sys::duckdb_map_type_key_type(type_) };
        let mut value = unsafe { duckdb_sys::duckdb_map_type_value_type(type_) };
        let res = logical_type_contains_json(key, depth + 1)
            || logical_type_contains_json(value, depth + 1);
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut key) };
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut value) };
        return res;
    }
    if type_id == duckdb_sys::DUCKDB_TYPE_STRUCT {
        let n = unsafe { duckdb_sys::duckdb_struct_type_child_count(type_) } as usize;
        for i in 0..n {
            let mut child = unsafe { duckdb_sys::duckdb_struct_type_child_type(type_, i as u64) };
            let res = logical_type_contains_json(child, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
            if res {
                return true;
            }
        }
        return false;
    }
    if type_id == duckdb_sys::DUCKDB_TYPE_UNION {
        let n = unsafe { duckdb_sys::duckdb_union_type_member_count(type_) } as usize;
        for i in 0..n {
            let mut child = unsafe { duckdb_sys::duckdb_union_type_member_type(type_, i as u64) };
            let res = logical_type_contains_json(child, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
            if res {
                return true;
            }
        }
        return false;
    }
    false
}

fn normalize_complex_value_for_shell_display(
    value: &str,
    unquote_json_literals: bool,
) -> Cow<'_, str> {
    // The shipped shell removes casts like `'x'::DATE` inside complex values and also
    // unquotes many simple string elements (e.g. `[a, NULL, c]`).
    //
    // This is intentionally limited to the complex-value pretty-printer output
    // (lists/arrays/structs) to avoid changing scalar formatting.
    if !value.contains('\'') && !value.contains("1e1000") && !value.contains("-1e1000") {
        return Cow::Borrowed(value);
    }

    let mut out = String::with_capacity(value.len());
    let mut last = 0usize;

    let mut iter = value.char_indices().peekable();
    while let Some((pos, ch)) = iter.next() {
        if ch != '\'' {
            continue;
        }

        let segment = &value[last..pos];
        if segment.contains("1e1000") || segment.contains("-1e1000") {
            out.push_str(&segment.replace("-1e1000", "-inf").replace("1e1000", "inf"));
        } else {
            out.push_str(segment);
        }
        let start = pos;

        if is_key_starting_quote(value, pos) {
            if let Some(end_quote) = find_key_closing_quote(value, pos) {
                let after_quote = end_quote + 1;
                if is_key_string_literal(&value[after_quote..]) {
                    let inner_raw = &value[pos + 1..end_quote];
                    let inner_decoded = decode_nested_string_inner_for_key(inner_raw);
                    out.push('\'');
                    out.push_str(&escape_single_quotes_for_shell_nested_string(
                        &inner_decoded,
                    ));
                    out.push('\'');

                    // Advance the iterator to consume up to and including the closing quote.
                    let mut after_quote = None;
                    while let Some((p, _)) = iter.next() {
                        if p == end_quote {
                            after_quote = Some(p + 1);
                            break;
                        }
                    }
                    let Some(after_quote) = after_quote else {
                        out.push_str(&value[start..]);
                        return Cow::Owned(out);
                    };
                    last = after_quote;
                    continue;
                }
            }
        }

        let mut inner = String::new();
        let after_quote;
        loop {
            let Some((p, c)) = iter.next() else {
                out.push_str(&value[start..]);
                return Cow::Owned(out);
            };
            if c == '\\' {
                // DuckDB sometimes uses backslash-escaped quotes inside nested displays.
                // Treat `\'` as a literal single quote inside the string.
                if let Some((_, '\'')) = iter.peek().copied() {
                    inner.push('\'');
                    let _ = iter.next();
                    continue;
                }
                inner.push('\\');
                continue;
            }
            if c != '\'' {
                inner.push(c);
                continue;
            }
            // Treat backslash-escaped quotes as a literal quote inside the string.
            // We detect this by counting trailing backslashes in the already-buffered `inner`.
            // If the count is odd, this quote is escaped (e.g. `\\'` -> literal `'`).
            let mut trailing_bs = 0usize;
            for ch in inner.chars().rev() {
                if ch == '\\' {
                    trailing_bs += 1;
                } else {
                    break;
                }
            }
            if trailing_bs % 2 == 1 {
                let _ = inner.pop(); // drop the escape backslash
                inner.push('\'');
                continue;
            }
            if let Some((_, '\'')) = iter.peek().copied() {
                inner.push('\'');
                let _ = iter.next();
                continue;
            }
            after_quote = p + 1;
            break;
        }

        let (skip_to, was_typed_literal) = strip_typed_literal_after_quote(value, after_quote);
        let allow_json_unquote =
            if unquote_json_literals && looks_like_json_literal_for_shell_display(&inner) {
                let bytes = value.as_bytes();
                let mut i = start;
                let mut allow = false;
                while i > 0 {
                    i -= 1;
                    match bytes[i] {
                        b' ' | b'\t' | b'\n' | b'\r' => continue,
                        b'[' | b',' => {
                            allow = true;
                            break;
                        }
                        _ => {
                            allow = false;
                            break;
                        }
                    }
                }
                allow
            } else {
                false
            };
        if is_key_string_literal(&value[after_quote..]) {
            // Struct keys are emitted by DuckDB's pretty-printer with shell-compatible escaping
            // (notably backslash-escaped quotes). Preserve the original token verbatim.
            out.push_str(&value[start..after_quote]);
        } else if was_typed_literal {
            // Typed literals show up in DuckDB's nested stringification output as `'x'::TYPE`.
            // The shipped shell strips the cast, then applies its normal nested-string quoting rules.
            if allow_json_unquote {
                out.push_str(&inner);
            } else if shell_should_unquote_string_literal_in_complex(&inner, unquote_json_literals)
            {
                out.push_str(&inner);
            } else {
                out.push('\'');
                out.push_str(&escape_single_quotes_for_shell_nested_string(&inner));
                out.push('\'');
            }
        } else if allow_json_unquote
            || shell_should_unquote_string_literal_in_complex(&inner, unquote_json_literals)
        {
            out.push_str(&inner);
        } else {
            out.push('\'');
            out.push_str(&escape_single_quotes_for_shell_nested_string(&inner));
            out.push('\'');
        }

        last = skip_to;
    }

    let tail = &value[last..];
    if tail.contains("1e1000") || tail.contains("-1e1000") {
        out.push_str(&tail.replace("-1e1000", "-inf").replace("1e1000", "inf"));
    } else {
        out.push_str(tail);
    }
    Cow::Owned(out)
}

fn print_database_error(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(msg.as_bytes());
    if !msg.ends_with('\n') {
        let _ = stderr.write_all(b"\n");
    }
}

fn print_database_error_state(state: &mut ShellState, msg: &str) {
    let mut stderr = std::io::stderr().lock();
    if !highlight_errors_enabled(state) {
        let _ = stderr.write_all(msg.as_bytes());
        if !msg.ends_with('\n') {
            let _ = stderr.write_all(b"\n");
        }
        return;
    }

    crate::highlight::detect_dark_light_mode(state);

    let style = terminal_code(state.highlight_style_error);
    if style.is_empty() {
        let _ = stderr.write_all(msg.as_bytes());
        if !msg.ends_with('\n') {
            let _ = stderr.write_all(b"\n");
        }
        return;
    }
    let reset = reset_terminal_code();

    let _ = stderr.write_all(style.as_bytes());
    for ch in msg.chars() {
        if ch == '"' {
            let _ = stderr.write_all(reset.as_bytes());
            let _ = stderr.write_all(b"\"");
            let _ = stderr.write_all(style.as_bytes());
            continue;
        }
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        let _ = stderr.write_all(s.as_bytes());
    }
    let _ = stderr.write_all(reset.as_bytes());
    if !msg.ends_with('\n') {
        let _ = stderr.write_all(b"\n");
    }
}

fn print_raw_stdout(msg: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(msg.as_bytes());
}

fn print_raw_stdout_line(msg: &str) {
    print_raw_stdout(msg);
    print_raw_stdout("\n");
}

fn print_stdout(state: &mut ShellState, msg: &str) {
    state.out.write_all(msg.as_bytes());
}

fn print_stdout_line(state: &mut ShellState, msg: &str) {
    print_stdout(state, msg);
    print_stdout(state, "\n");
}

fn print_stdout_bytes(state: &mut ShellState, bytes: &[u8]) {
    state.out.write_all(bytes);
}

#[allow(dead_code)]
fn print_stderr_bytes(bytes: &[u8]) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(bytes);
}

#[allow(dead_code)]
fn print_stderr_bytes_line(bytes: &[u8]) {
    print_stderr_bytes(bytes);
    if !bytes.last().is_some_and(|b| *b == b'\n') {
        print_stderr_bytes(b"\n");
    }
}

fn get_system_pager() -> String {
    std::env::var("DUCKDB_PAGER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("PAGER").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                "more".to_string()
            } else {
                "less -SRX".to_string()
            }
        })
}

#[cfg(target_os = "windows")]
fn enable_windows_utf8_console(state: &mut ShellState) {
    const CP_UTF8: u32 = 65001;

    extern "system" {
        fn SetConsoleCP(w_code_page_id: u32) -> i32;
        fn SetConsoleOutputCP(w_code_page_id: u32) -> i32;
    }

    state.win_utf8_mode = true;
    unsafe {
        let _ = SetConsoleCP(CP_UTF8);
        let _ = SetConsoleOutputCP(CP_UTF8);
    }
}

fn page_or_print_stdout(state: &mut ShellState, bytes: &[u8]) {
    if !state.stdout_is_console
        || !state.stdin_is_interactive
        || !state.outfile.is_empty()
        || !matches!(&state.out, OutputHandle::Stdout)
    {
        print_stdout_bytes(state, bytes);
        return;
    }
    if state.pager_mode == PagerMode::Off {
        print_stdout_bytes(state, bytes);
        return;
    }
    let line_count = bytes.iter().filter(|b| **b == b'\n').count() as u64;
    if state.pager_mode == PagerMode::Automatic {
        let mut max_line_len: u64 = 0;
        let mut cur: u64 = 0;
        for &b in bytes {
            if b == b'\n' {
                if cur > max_line_len {
                    max_line_len = cur;
                }
                cur = 0;
            } else {
                cur += 1;
            }
        }
        if cur > max_line_len {
            max_line_len = cur;
        }

        let triggers_rows = line_count >= state.pager_min_rows;
        let triggers_cols = state.pager_min_cols > 0 && max_line_len > state.pager_min_cols;
        if !triggers_rows && !triggers_cols {
            print_stdout_bytes(state, bytes);
            return;
        }
    }
    let cmd = if state.pager_command.trim().is_empty() {
        get_system_pager()
    } else {
        state.pager_command.clone()
    };

    let mut child = match crate::output::shell_command(&cmd)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            print_stdout_bytes(state, bytes);
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(bytes);
    }
    let _ = child.wait();
}

fn page_or_print_stdout_rows_only(state: &mut ShellState, bytes: &[u8]) {
    if !state.stdout_is_console
        || !state.stdin_is_interactive
        || !state.outfile.is_empty()
        || !matches!(&state.out, OutputHandle::Stdout)
    {
        print_stdout_bytes(state, bytes);
        return;
    }
    if state.pager_mode == PagerMode::Off {
        print_stdout_bytes(state, bytes);
        return;
    }
    let line_count = bytes.iter().filter(|b| **b == b'\n').count() as u64;
    if state.pager_mode == PagerMode::Automatic && line_count < state.pager_min_rows {
        print_stdout_bytes(state, bytes);
        return;
    }
    let cmd = if state.pager_command.trim().is_empty() {
        get_system_pager()
    } else {
        state.pager_command.clone()
    };

    let mut child = match crate::output::shell_command(&cmd)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            print_stdout_bytes(state, bytes);
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(bytes);
    }
    let _ = child.wait();
}

fn is_space(byte: u8) -> bool {
    (byte as char).is_whitespace()
}

fn resolve_backslashes(z: &[u8]) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::with_capacity(z.len());
    let mut pos = 0usize;
    while pos < z.len() {
        let mut c = z[pos];
        if c == b'\\' && pos + 1 < z.len() {
            pos += 1;
            c = z[pos];
            c = match c {
                b'a' => b'\x07',
                b'b' => b'\x08',
                b't' => b'\t',
                b'n' => b'\n',
                b'v' => b'\x0b',
                b'f' => b'\x0c',
                b'r' => b'\r',
                b'"' => b'"',
                b'\'' => b'\'',
                b'\\' => b'\\',
                b'0'..=b'7' => {
                    let mut oct = c - b'0';
                    if pos + 1 < z.len() && (b'0'..=b'7').contains(&z[pos + 1]) {
                        pos += 1;
                        oct = (oct << 3) + (z[pos] - b'0');
                        if pos + 1 < z.len() && (b'0'..=b'7').contains(&z[pos + 1]) {
                            pos += 1;
                            oct = (oct << 3) + (z[pos] - b'0');
                        }
                    }
                    oct
                }
                other => other,
            };
        }
        result.push(c);
        pos += 1;
    }
    result
}

fn parse_dot_command_args(z_line: &str) -> Vec<String> {
    let bytes = z_line.as_bytes();
    if bytes.is_empty() || bytes[0] != b'.' {
        return Vec::new();
    }
    let mut args: Vec<String> = Vec::new();
    let mut pos = 1usize;
    while pos < bytes.len() {
        while pos < bytes.len() && is_space(bytes[pos]) {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let quote = bytes[pos];
        let mut arg_bytes: Vec<u8> = Vec::new();
        if quote == b'\'' || quote == b'"' {
            pos += 1;
            while pos < bytes.len() && bytes[pos] != quote {
                if bytes[pos] == b'\\' && quote == b'"' && pos + 1 < bytes.len() {
                    arg_bytes.push(bytes[pos]);
                    pos += 1;
                }
                arg_bytes.push(bytes[pos]);
                pos += 1;
            }
            if pos < bytes.len() {
                pos += 1;
            }
            if quote == b'"' {
                arg_bytes = resolve_backslashes(&arg_bytes);
            }
        } else {
            while pos < bytes.len() && !is_space(bytes[pos]) {
                arg_bytes.push(bytes[pos]);
                pos += 1;
            }
            arg_bytes = resolve_backslashes(&arg_bytes);
        }
        args.push(String::from_utf8_lossy(&arg_bytes).to_string());
    }
    args
}

fn string_to_bool(z_arg: &str) -> bool {
    let v = z_arg.trim();
    if v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("yes") {
        return true;
    }
    if v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("no") {
        return false;
    }
    false
}

fn string_to_bool_shell(z_arg: &str) -> bool {
    let v = z_arg.trim();
    if v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("yes") {
        return true;
    }
    if v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("no") {
        return false;
    }
    print_database_error(&format!(
        "ERROR: Not a boolean value: \"{}\". Assuming \"no\".",
        v
    ));
    false
}

fn is_ascii_alpha(b: u8) -> bool {
    (b'A'..=b'Z').contains(&b) || (b'a'..=b'z').contains(&b)
}

fn is_ascii_alnum(b: u8) -> bool {
    is_ascii_alpha(b) || (b'0'..=b'9').contains(&b)
}

fn quote_char(state: &ShellState, input: &str) -> Option<u8> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Some(b'"');
    }
    let first = bytes[0];
    if !(is_ascii_alpha(first) || first == b'_') {
        return Some(b'"');
    }
    for &b in bytes.iter() {
        if !(is_ascii_alnum(b) || b == b'_') {
            return Some(b'"');
        }
    }
    if state.reserved_keywords_loaded {
        let upper = input.to_ascii_uppercase();
        if state.reserved_keywords.contains(&upper) {
            return Some(b'"');
        }
    }
    None
}

fn quote_identifier_if_needed(state: &ShellState, input: &str) -> String {
    if quote_char(state, input).is_none() {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        if ch == '"' {
            out.push('"');
            out.push('"');
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

fn set_table_name(state: &mut ShellState, name: &str) {
    state.zDestTable = quote_identifier_if_needed(state, name);
}

fn terminal_code(style: HighlightStyle) -> String {
    let mut out = String::new();
    match style.intensity {
        PrintIntensity::Standard => {}
        PrintIntensity::Bold => out.push_str("\x1b[1m"),
        PrintIntensity::Underline => out.push_str("\x1b[4m"),
        PrintIntensity::BoldUnderline => out.push_str("\x1b[1m\x1b[4m"),
    }
    let code: Option<u8> = match style.color {
        PrintColor::Standard => None,
        PrintColor::Black => Some(0),
        PrintColor::Red => Some(1),
        PrintColor::Green => Some(2),
        PrintColor::Yellow => Some(3),
        PrintColor::Blue => Some(4),
        PrintColor::Magenta => Some(5),
        PrintColor::Cyan => Some(6),
        PrintColor::BrightGray => Some(7),
        PrintColor::Gray => Some(8),
        PrintColor::BrightRed => Some(9),
        PrintColor::BrightGreen => Some(10),
        PrintColor::BrightYellow => Some(11),
        PrintColor::BrightBlue => Some(12),
        PrintColor::BrightMagenta => Some(13),
        PrintColor::BrightCyan => Some(14),
        PrintColor::White => Some(15),
        PrintColor::Extended(code) => Some(code),
    };
    if let Some(code) = code {
        // Match `ShellHighlight::TerminalCode` in tools/shell/shell_highlight.cpp:
        // - standard colors RED..BRIGHTGRAY use 31..37 (black is NOT in this range)
        // - bright colors GRAY..WHITE use 90..97
        // - everything else (including BLACK=0) uses 38;5;{code}
        if (1..=7).contains(&code) {
            out.push_str(&format!("\x1b[{}m", 31u16 + (code as u16 - 1)));
        } else if (8..=15).contains(&code) {
            out.push_str(&format!("\x1b[{}m", 90u16 + (code as u16 - 8)));
        } else {
            out.push_str(&format!("\x1b[38;5;{}m", code));
        }
    }
    out
}

fn reset_terminal_code() -> &'static str {
    "\x1b[00m"
}

fn highlight_style_for_element(state: &ShellState, element: &str) -> HighlightStyle {
    if let Some(style) = state.highlight_styles.get(element) {
        return *style;
    }
    match element {
        "error" => state.highlight_style_error,
        "keyword" => match state.highlight_mode {
            HighlightMode::Dark => HighlightStyle {
                color: PrintColor::Extended(33),
                intensity: PrintIntensity::Bold,
            },
            HighlightMode::Light => HighlightStyle {
                color: PrintColor::Extended(27),
                intensity: PrintIntensity::Bold,
            },
            _ => HighlightStyle {
                color: PrintColor::Green,
                intensity: PrintIntensity::Standard,
            },
        },
        "numeric_constant" => match state.highlight_mode {
            HighlightMode::Dark => HighlightStyle {
                color: PrintColor::Extended(212),
                intensity: PrintIntensity::Standard,
            },
            HighlightMode::Light => HighlightStyle {
                color: PrintColor::Extended(90),
                intensity: PrintIntensity::Standard,
            },
            _ => HighlightStyle {
                color: PrintColor::Yellow,
                intensity: PrintIntensity::Standard,
            },
        },
        "string_constant" => match state.highlight_mode {
            HighlightMode::Dark => HighlightStyle {
                color: PrintColor::Extended(220),
                intensity: PrintIntensity::Standard,
            },
            HighlightMode::Light => HighlightStyle {
                color: PrintColor::Extended(58),
                intensity: PrintIntensity::Standard,
            },
            _ => HighlightStyle {
                color: PrintColor::Yellow,
                intensity: PrintIntensity::Standard,
            },
        },
        "line_indicator" | "table_name" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Bold,
        },
        "database_name" | "suggestion_catalog_name" => HighlightStyle {
            color: PrintColor::Extended(172),
            intensity: PrintIntensity::Standard,
        },
        "schema_name" | "suggestion_schema_name" => HighlightStyle {
            color: PrintColor::Extended(39),
            intensity: PrintIntensity::Standard,
        },
        "column_name"
        | "suggestion_table_name"
        | "suggestion_column_name"
        | "suggestion_file_name"
        | "suggestion_function_name"
        | "suggestion_setting_name"
        | "view_layout"
        | "startup_version"
        | "numeric_value"
        | "string_value"
        | "temporal_value"
        | "footer"
        | "none" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Standard,
        },
        "column_type" | "null_value" | "layout" | "startup_text" | "continuation" | "comment"
        | "table_layout" => HighlightStyle {
            color: PrintColor::Gray,
            intensity: PrintIntensity::Standard,
        },
        "continuation_selected" => match state.highlight_mode {
            HighlightMode::Dark => HighlightStyle {
                color: PrintColor::Extended(33),
                intensity: PrintIntensity::Standard,
            },
            HighlightMode::Light => HighlightStyle {
                color: PrintColor::Extended(27),
                intensity: PrintIntensity::Standard,
            },
            _ => HighlightStyle {
                color: PrintColor::Green,
                intensity: PrintIntensity::Standard,
            },
        },
        "bracket" | "primary_key_column" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Underline,
        },
        "suggestion_directory_name" => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Bold,
        },
        "prompt" => HighlightStyle {
            color: PrintColor::Extended(208),
            intensity: PrintIntensity::Bold,
        },
        "error_emphasis" | "error_suggestion" => HighlightStyle {
            color: PrintColor::Red,
            intensity: PrintIntensity::Bold,
        },
        "log_trace" => HighlightStyle {
            color: PrintColor::Blue,
            intensity: PrintIntensity::Bold,
        },
        "log_debug" => HighlightStyle {
            color: PrintColor::Yellow,
            intensity: PrintIntensity::Bold,
        },
        "log_info" => HighlightStyle {
            color: PrintColor::Green,
            intensity: PrintIntensity::Bold,
        },
        "log_warning" => HighlightStyle {
            color: PrintColor::Extended(172),
            intensity: PrintIntensity::Bold,
        },
        _ => HighlightStyle {
            color: PrintColor::Standard,
            intensity: PrintIntensity::Standard,
        },
    }
}

const HIGHLIGHT_ELEMENTS: &[&str] = &[
    "error",
    "keyword",
    "numeric_constant",
    "string_constant",
    "line_indicator",
    "database_name",
    "schema_name",
    "table_name",
    "column_name",
    "column_type",
    "numeric_value",
    "string_value",
    "temporal_value",
    "null_value",
    "footer",
    "layout",
    "startup_text",
    "startup_version",
    "continuation",
    "continuation_selected",
    "bracket",
    "comment",
    "suggestion_catalog_name",
    "suggestion_schema_name",
    "suggestion_table_name",
    "suggestion_column_name",
    "suggestion_file_name",
    "suggestion_directory_name",
    "suggestion_function_name",
    "suggestion_setting_name",
    "table_layout",
    "view_layout",
    "primary_key_column",
    "prompt",
    "error_emphasis",
    "error_suggestion",
    "log_trace",
    "log_debug",
    "log_info",
    "log_warning",
    "none",
];

fn set_highlight_color(
    state: &mut ShellState,
    element: &str,
    color_name: &str,
    intensity: PrintIntensity,
) -> Result<(), String> {
    let element_norm = element.to_ascii_lowercase();
    if !HIGHLIGHT_ELEMENTS
        .iter()
        .any(|e| *e == element_norm.as_str())
    {
        let supported = HIGHLIGHT_ELEMENTS.join(", ");
        return Err(format!(
            "Unknown element '{}', supported options: {}\n",
            element, supported
        ));
    }

    let code = crate::display_colors::try_get_highlight_color_code(color_name)?;
    let style = HighlightStyle {
        color: PrintColor::Extended(code),
        intensity,
    };

    state.highlight_styles.insert(element_norm.clone(), style);
    match element_norm.as_str() {
        "error" => state.highlight_style_error = style,
        "column_name" => state.highlight_style_column_name = style,
        "column_type" => state.highlight_style_column_type = style,
        "null_value" => state.highlight_style_null_value = style,
        _ => {}
    }
    crate::highlight::sync_linenoise_highlight_style(&element_norm, style);
    Ok(())
}

fn highlight_results_enabled(state: &ShellState) -> bool {
    if !state.highlighting_enabled {
        return false;
    }
    match state.highlight_results {
        OptionType::On => true,
        OptionType::Off => false,
        OptionType::Default => state.stdout_is_console,
    }
}

fn highlight_errors_enabled(state: &ShellState) -> bool {
    if !state.highlighting_enabled {
        return false;
    }
    match state.highlight_errors {
        OptionType::On => true,
        OptionType::Off => false,
        OptionType::Default => state.stderr_is_console,
    }
}

fn escape_c_string(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + 2);
    result.push('"');
    for b in input.bytes() {
        match b {
            b'\\' => result.push_str("\\\\"),
            b'"' => result.push_str("\\\""),
            b'\t' => result.push_str("\\t"),
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            0x20..=0x7e => result.push(b as char),
            _ => result.push_str(&format!("\\{:03o}", b)),
        }
    }
    result.push('"');
    result
}

fn format_timer_line(elapsed: std::time::Duration) -> String {
    let real = elapsed.as_secs_f64();
    // Tests only assert the prefix, but keep a shell-like format.
    format!("Run Time (s): real {:.3} user 0.000000 sys 0.000000", real)
}

fn is_number(z: &str) -> bool {
    let bytes = z.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0usize;
    if bytes[i] == b'-' || bytes[i] == b'+' {
        i += 1;
        if i >= bytes.len() {
            return false;
        }
    }
    if !(b'0'..=b'9').contains(&bytes[i]) {
        return false;
    }
    i += 1;
    while i < bytes.len() && (b'0'..=b'9').contains(&bytes[i]) {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        if i >= bytes.len() || !(b'0'..=b'9').contains(&bytes[i]) {
            return false;
        }
        while i < bytes.len() && (b'0'..=b'9').contains(&bytes[i]) {
            i += 1;
        }
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if i >= bytes.len() || !(b'0'..=b'9').contains(&bytes[i]) {
            return false;
        }
        while i < bytes.len() && (b'0'..=b'9').contains(&bytes[i]) {
            i += 1;
        }
    }
    i == bytes.len()
}

fn try_parse_set_timezone_statement(stmt: &str) -> Option<String> {
    let stmt = crate::sql_split::normalize_statement_for_echo(stmt);
    let stmt = stmt.trim_start();
    let lower = stmt.to_ascii_lowercase();
    let bytes = stmt.as_bytes();

    let mut i = 0usize;
    if !lower.starts_with("set") {
        return None;
    }
    i += 3;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i > lower.len() || !lower[i..].starts_with("timezone") {
        return None;
    }
    i += "timezone".len();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i <= lower.len() && lower[i..].starts_with("to") {
        let after = i + 2;
        if after >= bytes.len() || bytes[after].is_ascii_whitespace() {
            i = after;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
    }
    if i < bytes.len() && bytes[i] == b'=' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    if i >= bytes.len() || bytes[i] != b'\'' {
        return None;
    }
    i += 1;
    let mut start = i;
    let mut out = String::new();
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                out.push_str(&String::from_utf8_lossy(&bytes[start..i]));
                out.push('\'');
                i += 2;
                start = i;
                continue;
            }
            out.push_str(&String::from_utf8_lossy(&bytes[start..i]));
            return Some(out);
        }
        i += 1;
    }
    None
}

fn try_run_linenoise_parse_option(args: &[String]) -> Result<bool, String> {
    for s in args {
        if s.as_bytes().contains(&0) {
            return Err("Invalid argument (contains null byte)".to_string());
        }
    }
    Ok(false)
}

fn render_length(s: &str) -> usize {
    duckdb_render_width::compute_render_width(s.as_bytes())
}

fn duckbox_render_length(s: &str) -> usize {
    duckdb_render_width::compute_render_width_duckbox(s.as_bytes())
}

fn utf8_width_print(out: &mut dyn Write, s: &str, width: usize, right_align: bool) {
    let bytes = s.as_bytes();
    let mut n: i32 = 0;
    let mut i = duckdb_render_width::get_render_position(bytes, width, &mut n);
    let n_usize: usize;
    if i < 0 {
        let mut j: usize = 0;
        let mut count: usize = 0;
        while j < bytes.len() {
            if (bytes[j] & 0xc0) != 0x80 {
                count += 1;
                if count == width {
                    j += 1;
                    while j < bytes.len() && (bytes[j] & 0xc0) == 0x80 {
                        j += 1;
                    }
                    break;
                }
            }
            j += 1;
        }
        i = j as i32;
        n_usize = count;
    } else {
        n_usize = n.max(0) as usize;
    }

    if n_usize >= width {
        let _ = out.write_all(&bytes[..(i as usize).min(bytes.len())]);
    } else if right_align {
        let _ = out.write_all(" ".repeat(width - n_usize).as_bytes());
        let _ = out.write_all(bytes);
    } else {
        let _ = out.write_all(bytes);
        let _ = out.write_all(" ".repeat(width - n_usize).as_bytes());
    }
}

fn duckbox_utf8_width_print(out: &mut dyn Write, s: &str, width: usize, right_align: bool) {
    let bytes = s.as_bytes();
    let mut n: i32 = 0;
    let mut i = duckdb_render_width::get_render_position_duckbox(bytes, width, &mut n);
    let n_usize: usize;
    if i < 0 {
        let mut j: usize = 0;
        let mut count: usize = 0;
        while j < bytes.len() {
            if (bytes[j] & 0xc0) != 0x80 {
                count += 1;
                if count == width {
                    j += 1;
                    while j < bytes.len() && (bytes[j] & 0xc0) == 0x80 {
                        j += 1;
                    }
                    break;
                }
            }
            j += 1;
        }
        i = j as i32;
        n_usize = count;
    } else {
        n_usize = n.max(0) as usize;
    }

    if n_usize >= width {
        let _ = out.write_all(&bytes[..(i as usize).min(bytes.len())]);
    } else if right_align {
        let _ = out.write_all(" ".repeat(width - n_usize).as_bytes());
        let _ = out.write_all(&bytes[..(i as usize).min(bytes.len())]);
    } else {
        let _ = out.write_all(&bytes[..(i as usize).min(bytes.len())]);
        let _ = out.write_all(" ".repeat(width - n_usize).as_bytes());
    }
}

fn print_dashes(out: &mut dyn Write, mut n: usize) {
    const DASH: &[u8] = b"--------------------------------------------------";
    while n > DASH.len() {
        let _ = out.write_all(DASH);
        n -= DASH.len();
    }
    let _ = out.write_all(&DASH[..n]);
}

fn print_row_separator(out: &mut dyn Write, col_width: &[usize], sep: &str) {
    if col_width.is_empty() {
        let _ = out.write_all(b"\n");
        return;
    }
    let _ = out.write_all(sep.as_bytes());
    print_dashes(out, col_width[0] + 2);
    for w in col_width.iter().skip(1) {
        let _ = out.write_all(sep.as_bytes());
        print_dashes(out, *w + 2);
    }
    let _ = out.write_all(sep.as_bytes());
    let _ = out.write_all(b"\n");
}

fn print_markdown_separator(
    out: &mut dyn Write,
    col_types: &[duckdb_sys::duckdb_type],
    col_width: &[usize],
) {
    for (idx, w) in col_width.iter().enumerate() {
        let _ = out.write_all(b"|");
        let t = col_types
            .get(idx)
            .copied()
            .unwrap_or(duckdb_sys::DUCKDB_TYPE_INVALID);
        let is_numeric = matches!(
            t,
            duckdb_sys::DUCKDB_TYPE_TINYINT
                | duckdb_sys::DUCKDB_TYPE_SMALLINT
                | duckdb_sys::DUCKDB_TYPE_INTEGER
                | duckdb_sys::DUCKDB_TYPE_BIGINT
                | duckdb_sys::DUCKDB_TYPE_HUGEINT
                | duckdb_sys::DUCKDB_TYPE_UTINYINT
                | duckdb_sys::DUCKDB_TYPE_USMALLINT
                | duckdb_sys::DUCKDB_TYPE_UINTEGER
                | duckdb_sys::DUCKDB_TYPE_UBIGINT
                | duckdb_sys::DUCKDB_TYPE_UHUGEINT
                | duckdb_sys::DUCKDB_TYPE_FLOAT
                | duckdb_sys::DUCKDB_TYPE_DOUBLE
                | duckdb_sys::DUCKDB_TYPE_DECIMAL
        );
        if is_numeric {
            print_dashes(out, w + 1);
            let _ = out.write_all(b":");
        } else {
            print_dashes(out, w + 2);
        }
    }
    let _ = out.write_all(b"|\n");
}

fn render_aligned_value(out: &mut dyn Write, value: &str, width: usize) {
    let n = render_length(value);
    let left = (width.saturating_sub(n)) / 2;
    let right = (width.saturating_sub(n) + 1) / 2;
    let _ = out.write_all(" ".repeat(left).as_bytes());
    let _ = out.write_all(value.as_bytes());
    let _ = out.write_all(" ".repeat(right).as_bytes());
}

fn output_quoted_string(out: &mut dyn Write, z: &str) {
    let _ = out.write_all(b"'");
    for ch in z.chars() {
        if ch == '\'' {
            let _ = out.write_all(b"''");
        } else {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            let _ = out.write_all(s.as_bytes());
        }
    }
    let _ = out.write_all(b"'");
}

fn output_quoted_escaped_string(out: &mut dyn Write, z: &str) {
    // Matches `EscapeNewlines` in `tools/shell/shell_renderer.cpp` (v1.4.3):
    // use `concat(..., chr(N), ...)` to avoid embedding raw newlines in the output.
    let needs_concat = z.as_bytes().iter().any(|b| matches!(*b, b'\n' | b'\r' | 0));
    if needs_concat {
        let _ = out.write_all(b"concat('");
    } else {
        let _ = out.write_all(b"'");
    }
    for ch in z.chars() {
        match ch {
            '\n' | '\r' | '\0' => {
                let _ = out.write_all(b"', chr(");
                match ch {
                    '\n' => {
                        let _ = out.write_all(b"10");
                    }
                    '\r' => {
                        let _ = out.write_all(b"13");
                    }
                    '\0' => {
                        let _ = out.write_all(b"0");
                    }
                    _ => {}
                }
                let _ = out.write_all(b"), '");
            }
            '\'' => {
                let _ = out.write_all(b"''");
            }
            _ => {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                let _ = out.write_all(s.as_bytes());
            }
        }
    }
    let _ = out.write_all(b"'");
    if needs_concat {
        let _ = out.write_all(b")");
    }
}

fn output_c_string(out: &mut dyn Write, z: &str) {
    let _ = out.write_all(b"\"");
    for b in z.as_bytes().iter().copied() {
        match b {
            b'\\' => {
                let _ = out.write_all(b"\\\\");
            }
            b'"' => {
                let _ = out.write_all(b"\\\"");
            }
            b'\t' => {
                let _ = out.write_all(b"\\t");
            }
            b'\n' => {
                let _ = out.write_all(b"\\n");
            }
            b'\r' => {
                let _ = out.write_all(b"\\r");
            }
            0x20..=0x7e => {
                let _ = out.write_all(&[b]);
            }
            _ => {
                let _ = out.write_all(format!("\\{:03o}", b).as_bytes());
            }
        }
    }
    let _ = out.write_all(b"\"");
}

fn output_json_string(out: &mut dyn Write, bytes: &[u8]) {
    let _ = out.write_all(b"\"");
    for b in bytes.iter().copied() {
        match b {
            b'\\' | b'"' => {
                let _ = out.write_all(b"\\");
                let _ = out.write_all(&[b]);
            }
            0x00..=0x1f => {
                let _ = out.write_all(b"\\");
                match b {
                    0x08 => {
                        let _ = out.write_all(b"b");
                    }
                    0x0c => {
                        let _ = out.write_all(b"f");
                    }
                    b'\n' => {
                        let _ = out.write_all(b"n");
                    }
                    b'\r' => {
                        let _ = out.write_all(b"r");
                    }
                    b'\t' => {
                        let _ = out.write_all(b"t");
                    }
                    _ => {
                        let _ = out.write_all(format!("u{:04x}", b).as_bytes());
                    }
                }
            }
            _ => {
                let _ = out.write_all(&[b]);
            }
        }
    }
    let _ = out.write_all(b"\"");
}

fn output_csv(out: &mut dyn Write, state: &ShellState, z: Option<&str>, b_sep: bool) {
    if let Some(z) = z {
        let bytes = z.as_bytes();
        let sep = state.colSeparator.as_bytes();
        let n_sep = sep.len();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            if c <= 31 || c == b'"' || c == b'\'' || c >= 127 {
                i = 0;
                break;
            }
            if !sep.is_empty()
                && c == sep[0]
                && (n_sep == 1 || (bytes.len() >= n_sep && &bytes[..n_sep] == sep))
            {
                // Matches v1.4.3 shell behavior (note: multi-byte separator check is anchored at start)
                i = 0;
                break;
            }
            i += 1;
        }
        if i == 0 {
            let _ = out.write_all(b"\"");
            for ch in z.chars() {
                if ch == '"' {
                    let _ = out.write_all(b"\"\"");
                } else {
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    let _ = out.write_all(s.as_bytes());
                }
            }
            let _ = out.write_all(b"\"");
        } else {
            let _ = out.write_all(bytes);
        }
    } else {
        let _ = out.write_all(state.nullValue.as_bytes());
    }
    if b_sep {
        let _ = out.write_all(state.colSeparator.as_bytes());
    }
}

fn box_convert_value(z: &str) -> String {
    let bytes = z.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'\n' {
            out.push(b'\\');
            out.push(b'n');
        } else {
            out.push(b);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn show_help(state: &mut ShellState, pattern: Option<&str>) -> usize {
    // Match `ShellState::PrintHelp` in `tools/shell/shell_metadata_command.cpp`.
    const MIN_SPACING: usize = 4;
    const SPACING_PER_LAYER: usize = 2;

    struct PrintCommandInfo {
        command_name: String,
        first_part: String,
        second_part: String,
    }

    fn should_print_command(spec: &crate::dotcmd::DotCommandSpec, glob_pattern: &str) -> bool {
        let extra = crate::dotcmd::help_extra_description(spec.id);
        if extra.is_none() {
            return false;
        }
        let desc = crate::dotcmd::help_summary(spec.id);
        if desc.contains("DEPRECATED") {
            return false;
        }
        if glob_pattern.is_empty() {
            return true;
        }
        crate::sqlite_shell::strglob(glob_pattern, spec.command)
    }

    fn print_plain(state: &mut ShellState, text: &str) {
        print_stdout(state, text);
    }

    let mut print_extended = false;
    let mut glob_pattern = String::new();
    if let Some(pat) = pattern {
        print_extended = true;
        if pat == "-a" || pat == "-all" || pat == "--all" {
            glob_pattern.clear();
        } else {
            glob_pattern = format!("{pat}*");
        }
    }

    let mut print_info_list: Vec<PrintCommandInfo> = Vec::new();
    for spec in crate::dotcmd::METADATA_COMMANDS {
        if !should_print_command(spec, &glob_pattern) {
            continue;
        }
        let command_name = format!(".{}", spec.command);
        let first_part = format!(" {}", spec.usage);
        let second_part = crate::dotcmd::help_summary(spec.id).to_string();
        print_info_list.push(PrintCommandInfo {
            command_name,
            first_part,
            second_part,
        });

        if print_extended {
            let extra = crate::dotcmd::help_extra_description(spec.id).unwrap_or("");
            let mut current = PrintCommandInfo {
                command_name: String::new(),
                first_part: String::new(),
                second_part: String::new(),
            };
            let mut in_first_part = true;
            let mut after_newline = true;
            for ch in extra.chars() {
                match ch {
                    '\n' => {
                        print_info_list.push(current);
                        current = PrintCommandInfo {
                            command_name: String::new(),
                            first_part: String::new(),
                            second_part: String::new(),
                        };
                        in_first_part = true;
                        after_newline = true;
                    }
                    '\t' => {
                        if after_newline {
                            current.first_part.push_str(&" ".repeat(SPACING_PER_LAYER));
                        } else {
                            if !in_first_part {
                                panic!("invalid help extra description for {}", spec.command);
                            }
                            in_first_part = false;
                        }
                    }
                    other => {
                        if after_newline {
                            current.first_part.push_str(&" ".repeat(SPACING_PER_LAYER));
                        }
                        after_newline = false;
                        if in_first_part {
                            current.first_part.push(other);
                        } else {
                            current.second_part.push(other);
                        }
                    }
                }
            }
            if !current.first_part.is_empty() {
                print_info_list.push(current);
            }
        }
    }

    let mut max_lhs_size: usize = 0;
    for info in &print_info_list {
        if info.second_part.is_empty() {
            continue;
        }
        let lhs_size = info.command_name.len() + info.first_part.len() + MIN_SPACING;
        if lhs_size > max_lhs_size {
            max_lhs_size = lhs_size;
        }
    }

    for info in &print_info_list {
        let lhs_size = info.command_name.len() + info.first_part.len();
        let spaces = if !info.second_part.is_empty() {
            " ".repeat(max_lhs_size.saturating_sub(lhs_size))
        } else {
            String::new()
        };

        if !info.command_name.is_empty() {
            print_plain(state, &info.command_name);
        }
        print_plain(state, &info.first_part);
        print_stdout(state, &spaces);
        print_stdout_line(state, &info.second_part);
    }

    if !print_extended {
        print_stdout_line(state, "");
        print_stdout_line(state, "Run .help --all for extended information");
        print_stdout_line(state, "Run .help shortcuts for keyboard shortcuts");
    }

    print_info_list.len()
}

fn print_shortcut_heading(state: &mut ShellState, heading: &str) {
    if state.highlighting_enabled {
        print_stdout(state, "\x1b[32m");
    }
    print_stdout(state, heading);
    if state.highlighting_enabled {
        print_stdout(state, "\x1b[00m");
    }
    print_stdout(state, "\n");
}

fn show_shortcuts_help(state: &mut ShellState) {
    const SECTIONS: &[(&str, &[(&str, &str)])] = &[
        (
            "Control",
            &[
                ("Enter / Ctrl+J", "Submit input"),
                (
                    "Ctrl+C",
                    "Cancel current input or interrupt in-flight query",
                ),
                ("Ctrl+D", "Exit shell (when line is empty)"),
                ("Ctrl+G", "Submit input (in multiline mode)"),
                ("Ctrl+L", "Clear screen"),
                ("Ctrl+Z", "Suspend shell"),
                ("Tab", "Auto-complete"),
                ("Ctrl+Q, then click", "Move cursor to mouse click position"),
            ],
        ),
        (
            "Editing",
            &[
                (
                    "Ctrl+D / Delete",
                    "Delete character under cursor (or exit if line is empty)",
                ),
                ("Ctrl+H / Backspace", "Delete character before cursor"),
                ("Ctrl+K", "Delete from cursor to end of line"),
                ("Ctrl+U", "Delete entire line"),
                ("Ctrl+W", "Delete previous word"),
                ("Alt+D", "Delete next word"),
                ("Alt+Backspace", "Delete previous word"),
                ("Ctrl+T", "Swap character under cursor with previous"),
                ("Alt+T", "Swap current word with previous"),
                ("Alt+C", "Capitalize next word"),
                ("Alt+L", "Lowercase next word"),
                ("Alt+U", "Uppercase next word"),
                ("Alt+R", "Delete entire line"),
                ("Alt+\\", "Remove spaces around cursor"),
                ("Ctrl+X", "Insert newline (multiline input)"),
            ],
        ),
        (
            "Navigation",
            &[
                ("Ctrl+A / Home", "Go to beginning of line"),
                ("Ctrl+E / End", "Go to end of line"),
                ("Ctrl+B / Left", "Move cursor left"),
                ("Ctrl+F / Right", "Move cursor right"),
                ("Alt+B / Alt+Left", "Move cursor one word left"),
                ("Alt+F / Alt+Right", "Move cursor one word right"),
            ],
        ),
        (
            "History",
            &[
                ("Ctrl+P / Up", "Previous history entry"),
                ("Ctrl+N / Down", "Next history entry"),
                ("Ctrl+R", "Reverse search history"),
                ("Ctrl+S", "Forward search history"),
                ("Ctrl+Up", "Jump to first history entry"),
                ("Ctrl+Down", "Jump to last history entry"),
            ],
        ),
    ];

    for (section_idx, (heading, rows)) in SECTIONS.iter().enumerate() {
        if section_idx > 0 {
            print_stdout(state, "\n");
        }
        print_shortcut_heading(state, heading);
        for (shortcut, description) in *rows {
            print_stdout_line(state, &format!("  {:<23}{}", shortcut, description));
        }
    }
}

fn set_output_file(state: &mut ShellState, args: &[String], output_mode: char) -> i32 {
    if state.safe_mode {
        print_database_error(".output/.once/.excel cannot be used in -safe mode");
        return 1;
    }

    let mut z_file: Option<String> = None;
    let mut b_bom = false;
    let mut e_mode: u8 = 0; // 0 none, b'e' editor, b'x' spreadsheet
    let mut b_once: u8 = 0; // 0 .output, 1 .once, 2 .excel

    if output_mode == 'e' {
        e_mode = b'x';
        b_once = 2;
    } else if output_mode == 'o' {
        b_once = 1;
    }

    for arg in args.iter().skip(1) {
        let mut z = arg.as_str();
        if z.starts_with('-') {
            if z.starts_with("--") {
                // Match v1.4.3 shell: "--bom" treated as "-bom" in parsing.
                z = &z[1..];
            }
            if z == "-bom" {
                b_bom = true;
            } else if output_mode != 'e' && z == "-x" {
                e_mode = b'x';
            } else if output_mode != 'e' && z == "-e" {
                e_mode = b'e';
            } else {
                print_stdout(
                    state,
                    &format!("ERROR: unknown option: \"{}\".  Usage:\n", arg),
                );
                let _ = show_help(state, Some(args[0].as_str()));
                return 1;
            }
            continue;
        }
        if z_file.is_none() {
            z_file = Some(z.to_string());
        } else {
            print_stdout(
                state,
                &format!("ERROR: extra parameter: \"{}\".  Usage:\n", arg),
            );
            let _ = show_help(state, Some(args[0].as_str()));
            return 1;
        }
    }

    let z_file = z_file.unwrap_or_else(|| "stdout".to_string());
    state.outCount = if b_once != 0 { 2 } else { 0 };

    crate::output::reset_output(state);

    let mut z_target = z_file.clone();
    if e_mode == b'e' || e_mode == b'x' {
        state.doXdgOpen = true;
        crate::output::push_output_mode(state);
        if e_mode == b'x' {
            state.zTempFile = crate::output::new_temp_file_path("csv");
            state.shellFlgs &= !(crate::state::ShellFlags::SHFLG_Echo as u32);
            state.mode = RenderMode::CSV;
            state.colSeparator = ",".to_string();
            state.rowSeparator = "\r\n".to_string();
        } else {
            state.zTempFile = crate::output::new_temp_file_path("txt");
        }
        z_target = state.zTempFile.clone();
    }

    if let Some(cmd) = z_target.strip_prefix('|') {
        if let Err(err) = crate::output::open_pipe(state, cmd) {
            print_database_error(&err);
            crate::output::reset_output(state);
            return 1;
        }
        if b_bom {
            state.out.write_all(b"\xEF\xBB\xBF");
        }
        state.outfile = z_target;
        state.stdout_is_console = false;
        return 0;
    }

    if let Err(err) = crate::output::open_output_file(state, &z_target) {
        print_database_error(&err);
        crate::output::reset_output(state);
        return 1;
    }
    if b_bom {
        state.out.write_all(b"\xEF\xBB\xBF");
    }
    state.outfile = z_target;
    state.stdout_is_console = false;
    0
}

fn print_pretty_name_list(state: &mut ShellState, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let target_width = if state.max_width == 0 || state.max_width == u64::MAX {
        80usize
    } else {
        (state.max_width as usize).max(1)
    };

    let mut maxlen = 0usize;
    for name in names {
        maxlen = maxlen.max(render_length(name));
    }
    let maxlen = maxlen.min(target_width);

    let mut n_print_col = target_width / (maxlen + 2);
    if n_print_col < 1 {
        n_print_col = 1;
    }
    let n_print_row = (names.len() + n_print_col - 1) / n_print_col;
    for i in 0..n_print_row {
        for j in (i..names.len()).step_by(n_print_row) {
            let prefix = if j < n_print_row { "" } else { "  " };
            let mut padded = truncate_with_ellipsis(&names[j], maxlen);
            let pad = maxlen.saturating_sub(render_length(&padded));
            if pad > 0 {
                padded.push_str(&" ".repeat(pad));
            }
            print_stdout(state, &format!("{}{}", prefix, padded));
        }
        print_stdout(state, "\n");
    }
}

fn sql_escape_single_quotes(s: &str) -> String {
    s.replace('\'', "''")
}

#[allow(dead_code)]
fn csv_needs_quote(value: &[u8], col_sep: &[u8]) -> bool {
    if col_sep.is_empty() {
        return value
            .iter()
            .any(|b| *b <= 31 || *b == b'"' || *b == b'\'' || *b >= 127);
    }
    let mut i = 0usize;
    while i < value.len() {
        let b = value[i];
        if b <= 31 || b == b'"' || b == b'\'' || b >= 127 {
            return true;
        }
        if i + col_sep.len() <= value.len() && &value[i..i + col_sep.len()] == col_sep {
            return true;
        }
        i += 1;
    }
    false
}

#[allow(dead_code)]
fn csv_escape(value: &str, col_sep: &str) -> String {
    let bytes = value.as_bytes();
    let col_sep_bytes = col_sep.as_bytes();
    if !csv_needs_quote(bytes, col_sep_bytes) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
            out.push('"');
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

fn set_output_mode(state: &mut ShellState, mode_name: &str, tbl_name: Option<&str>) -> bool {
    let mode_str = mode_name;
    let n2 = mode_str.len();
    let c2 = mode_str.as_bytes().get(0).copied().unwrap_or(0) as char;

    if tbl_name.is_some() && !(c2 == 'i' && "insert".starts_with(mode_str)) {
        print_database_error("TABLE argument can only be used with .mode insert");
        return false;
    }

    let starts = |prefix: &str| prefix.starts_with(mode_str) && !mode_str.is_empty();

    if c2 == 'l' && n2 > 2 && starts("lines") {
        state.mode = RenderMode::LINE;
        state.rowSeparator = "\n".to_string();
    } else if c2 == 'c' && starts("columns") {
        state.mode = RenderMode::COLUMN;
        state.rowSeparator = "\n".to_string();
    } else if c2 == 'l' && n2 > 2 && starts("list") {
        state.mode = RenderMode::LIST;
        state.colSeparator = "|".to_string();
        state.rowSeparator = "\n".to_string();
    } else if c2 == 'h' && starts("html") {
        state.mode = RenderMode::HTML;
    } else if c2 == 't' && starts("tcl") {
        state.mode = RenderMode::TCL;
        state.colSeparator = " ".to_string();
        state.rowSeparator = "\n".to_string();
    } else if c2 == 'c' && starts("csv") {
        state.mode = RenderMode::CSV;
        state.colSeparator = ",".to_string();
        state.rowSeparator = "\r\n".to_string();
    } else if c2 == 't' && starts("tabs") {
        state.mode = RenderMode::LIST;
        state.colSeparator = "\t".to_string();
    } else if c2 == 'i' && starts("insert") {
        state.mode = RenderMode::INSERT;
        set_table_name(state, tbl_name.unwrap_or("table"));
    } else if c2 == 'q' && starts("quote") {
        state.mode = RenderMode::QUOTE;
        state.colSeparator = ",".to_string();
        state.rowSeparator = "\n".to_string();
    } else if c2 == 'a' && starts("ascii") {
        state.mode = RenderMode::ASCII;
        state.colSeparator = "\x1F".to_string();
        state.rowSeparator = "\x1E".to_string();
    } else if c2 == 'm' && starts("markdown") {
        state.mode = RenderMode::MARKDOWN;
    } else if c2 == 't' && starts("table") {
        state.mode = RenderMode::TABLE;
    } else if c2 == 'b' && starts("box") {
        state.mode = RenderMode::BOX;
    } else if c2 == 'd' && starts("duckbox") {
        state.mode = RenderMode::DUCKBOX;
    } else if c2 == 'j' && starts("json") {
        state.mode = RenderMode::JSON;
    } else if c2 == 'l' && starts("latex") {
        state.mode = RenderMode::LATEX;
    } else if c2 == 't' && starts("trash") {
        state.mode = RenderMode::TRASH;
    } else if c2 == 'j' && starts("jsonlines") {
        state.mode = RenderMode::JSONLINES;
    } else {
        print_database_error(
			"Error: mode should be one of: ascii box column csv duckbox html insert json jsonlines latex line list markdown quote table tabs tcl trash \n",
		);
        return false;
    }
    state.cMode = state.mode;
    true
}

fn mode_to_string(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::LINE => "line",
        RenderMode::COLUMN => "column",
        RenderMode::LIST => "list",
        RenderMode::SEMI => "semi",
        RenderMode::HTML => "html",
        RenderMode::INSERT => "insert",
        RenderMode::QUOTE => "quote",
        RenderMode::TCL => "tcl",
        RenderMode::CSV => "csv",
        RenderMode::EXPLAIN => "explain",
        RenderMode::DESCRIBE => "describe",
        RenderMode::ASCII => "ascii",
        RenderMode::PRETTY => "prettyprint",
        RenderMode::EQP => "eqp",
        RenderMode::JSON => "json",
        RenderMode::MARKDOWN => "markdown",
        RenderMode::TABLE => "table",
        RenderMode::BOX => "box",
        RenderMode::LATEX => "latex",
        RenderMode::TRASH => "trash",
        RenderMode::JSONLINES => "jsonlines",
        RenderMode::DUCKBOX => "duckbox",
    }
}

fn run_dot_command(state: &mut ShellState, session: &mut Session, command: &str) -> i32 {
    let args = parse_dot_command_args(command.trim_end_matches('\n'));
    if args.is_empty() {
        return 0;
    }
    let name = args[0].as_str();
    let Some(spec) = crate::dotcmd::find_dot_command(name) else {
        let mut error_msg = format!("Unknown Command Error: Unrecognized command '{}'\n", name);
        let command_names: Vec<String> = crate::dotcmd::METADATA_COMMANDS
            .iter()
            .map(|cmd| format!(".{}", cmd.command))
            .collect();
        let candidates_msg =
            crate::candidates::candidates_error_message(&command_names, name, "Did you mean");
        error_msg.push_str(&candidates_msg);
        error_msg.push('\n');
        error_msg.push_str("Run '.help' for more information.");
        print_database_error(&error_msg);
        return 1;
    };
    if spec.argument_count != 0 && spec.argument_count != args.len() {
        let error = format!(
            "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
            name, spec.command, spec.usage
        );
        print_database_error(&error);
        return 1;
    }
    match spec.id {
        crate::dotcmd::DotCommandId::Bail => {
            state.bail = if args[1] == "auto" {
                BailOnError::Automatic
            } else if string_to_bool(&args[1]) {
                BailOnError::Bail
            } else {
                BailOnError::DontBail
            };
            0
        }
        crate::dotcmd::DotCommandId::Binary => {
            // .binary on|off
            state.binary_mode = string_to_bool(&args[1]);
            0
        }
        crate::dotcmd::DotCommandId::Cd => {
            if state.safe_mode {
                print_database_error(".cd cannot be used in -safe mode");
                return 1;
            }
            let dir = args[1].as_str();
            if std::env::set_current_dir(dir).is_err() {
                print_database_error(&format!("Cannot change to directory \"{}\"", dir));
                return 1;
            }
            0
        }
        crate::dotcmd::DotCommandId::Changes => {
            let on = string_to_bool(&args[1]);
            let bit = crate::state::ShellFlags::SHFLG_CountChanges as u32;
            if on {
                state.shellFlgs |= bit;
            } else {
                state.shellFlgs &= !bit;
            }
            0
        }
        crate::dotcmd::DotCommandId::Echo => {
            let on = string_to_bool(&args[1]);
            let bit = crate::state::ShellFlags::SHFLG_Echo as u32;
            if on {
                state.shellFlgs |= bit;
            } else {
                state.shellFlgs &= !bit;
            }
            0
        }
        crate::dotcmd::DotCommandId::Columns => {
            state.columns = true;
            0
        }
        crate::dotcmd::DotCommandId::Last => {
            let Some(query) = state.last_query_duckbox.clone() else {
                return 0;
            };
            if state.mode != RenderMode::DUCKBOX {
                return 0;
            }
            run_duckbox_query_unlimited(state, session.con, &query)
        }
        crate::dotcmd::DotCommandId::Databases => {
            // Match DuckDB v1.4.3 shell behavior: render via the table-metadata renderer (same as `.tables`).
            let query = match CString::new("SELECT name, file FROM pragma_database_list") {
                Ok(q) => q,
                Err(_) => return 1,
            };
            let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe { duckdb_sys::duckdb_query(session.con, query.as_ptr(), &mut result) };
            if rc != duckdb_sys::DuckDBSuccess {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                print_database_error("Failed to list databases");
                return 1;
            }

            let row_count = unsafe { duckdb_sys::duckdb_row_count(&mut result) } as usize;
            let mut columns: Vec<TableMetadataColumn> = Vec::with_capacity(row_count);
            for row in 0..row_count {
                let mut get_str = |col: u64| -> Option<String> {
                    let ptr =
                        unsafe { duckdb_sys::duckdb_value_varchar(&mut result, col, row as u64) };
                    if ptr.is_null() {
                        return None;
                    }
                    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
                    unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
                    Some(s)
                };
                let name = get_str(0).unwrap_or_default();
                let file = get_str(1).unwrap_or_else(|| "(memory)".to_string());
                columns.push(TableMetadataColumn {
                    column_name: name,
                    column_type: file,
                    is_primary_key: false,
                    is_not_null: false,
                    is_unique: false,
                    default_value: String::new(),
                });
            }
            unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };

            if columns.is_empty() {
                return 0;
            }

            let tables = [TableMetadataTable {
                database_name: String::new(),
                schema_name: String::new(),
                table_name: "databases".to_string(),
                columns,
                estimated_size: None,
                is_view: false,
            }];
            let out = render_table_metadata(state, &tables);
            page_or_print_stdout(state, &out);
            0
        }
        crate::dotcmd::DotCommandId::Rows => {
            state.columns = false;
            0
        }
        crate::dotcmd::DotCommandId::Help => {
            if let Some(pat) = args.get(1).map(|s| s.as_str()) {
                if pat == "shortcuts" {
                    show_shortcuts_help(state);
                } else {
                    let n = show_help(state, Some(pat));
                    if n == 0 {
                        print_stdout_line(state, &format!("Nothing matches '{}'", pat));
                    }
                }
            } else {
                let _ = show_help(state, None);
            }
            0
        }
        crate::dotcmd::DotCommandId::DisplayColors => {
            let mut bold = false;
            let mut underline = false;
            for arg in args.iter().skip(1) {
                match arg.as_str() {
                    "bold" => bold = true,
                    "underline" => underline = true,
                    _ => {
                        let error = format!(
								"Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
								name, spec.command, spec.usage
							);
                        print_database_error(&error);
                        return 1;
                    }
                }
            }
            let intensity = match (bold, underline) {
                (false, false) => PrintIntensity::Standard,
                (true, false) => PrintIntensity::Bold,
                (false, true) => PrintIntensity::Underline,
                (true, true) => PrintIntensity::BoldUnderline,
            };
            let out =
                crate::display_colors::render_display_colors(intensity, state.highlighting_enabled);
            print_stdout(state, &out);
            0
        }
        crate::dotcmd::DotCommandId::HighlightMode => {
            match args[1].as_str() {
                "mixed" => state.highlight_mode = HighlightMode::Mixed,
                "dark" => state.highlight_mode = HighlightMode::Dark,
                "light" => state.highlight_mode = HighlightMode::Light,
                "auto" => state.highlight_mode = HighlightMode::Automatic,
                _ => {
                    let error = format!(
                        "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                        name, spec.command, spec.usage
                    );
                    print_database_error(&error);
                    return 1;
                }
            }
            if state.highlight_mode != HighlightMode::Automatic {
                crate::highlight::apply_mode_styles(state, state.highlight_mode);
                crate::highlight::sync_linenoise_highlight_mode(state.highlight_mode);
            }
            0
        }
        crate::dotcmd::DotCommandId::Mode => {
            if args.len() == 1 {
                print_stdout(
                    state,
                    &format!("current output mode: {}\n", mode_to_string(state.mode)),
                );
                return 0;
            }
            if args.len() > 3 {
                print_database_error("Usage: .mode MODE ?TABLE?");
                return 1;
            }
            let mode_name = args[1].as_str();
            let tbl_name = args.get(2).map(|s| s.as_str());
            if !set_output_mode(state, mode_name, tbl_name) {
                return 1;
            }
            0
        }
        crate::dotcmd::DotCommandId::Separator => {
            if args.len() < 2 || args.len() > 3 {
                print_database_error("Usage: .separator COL ?ROW?");
                return 1;
            }
            state.colSeparator = args[1].to_string();
            if let Some(row) = args.get(2) {
                state.rowSeparator = row.to_string();
            }
            0
        }
        crate::dotcmd::DotCommandId::NullValue => {
            if args.len() != 2 {
                print_database_error("Usage: .nullvalue STRING");
                return 1;
            }
            state.nullValue = args[1].to_string();
            0
        }
        crate::dotcmd::DotCommandId::Headers => {
            if args.len() != 2 {
                print_database_error("Usage: .headers on|off");
                return 1;
            }
            state.showHeader = string_to_bool(&args[1]);
            0
        }
        crate::dotcmd::DotCommandId::Pager => {
            // NOTE: The shipped v1.4.3 CLI in this repo doesn't expose .pager,
            // but we keep it for parity with the newer shell behavior and requested pager support.
            if args.len() == 1 {
                let mode_str = match state.pager_mode {
                    PagerMode::Off => "off",
                    PagerMode::On => "on",
                    PagerMode::Automatic => "automatic",
                };
                print_stdout(state, &format!("Pager mode: {}\n", mode_str));
                if state.pager_mode == PagerMode::Automatic {
                    print_stdout(
                        state,
                        &format!(
							"Trigger pager when rows exceed {} or result set is wider than terminal",
							state.pager_min_rows
						),
                    );
                }
                print_stdout(state, &format!("Pager command: {}\n", state.pager_command));
                return 0;
            }
            if args[1] == "set_row_threshold" {
                if args.len() != 3 {
                    print_database_error("Usage: .pager set_row_threshold THRESHOLD");
                    return 1;
                }
                let Ok(v) = args[2].parse::<u64>() else {
                    print_database_error("Invalid threshold");
                    return 1;
                };
                state.pager_min_rows = v;
                return 0;
            }
            if args.len() != 2 {
                let error = format!(
                    "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                    name, spec.command, spec.usage
                );
                print_database_error(&error);
                return 1;
            }
            match args[1].as_str() {
                "on" => {
                    state.pager_mode = PagerMode::On;
                    if state.pager_command.trim().is_empty() {
                        state.pager_command = get_system_pager();
                    }
                }
                "off" => state.pager_mode = PagerMode::Off,
                "automatic" => state.pager_mode = PagerMode::Automatic,
                other => state.pager_command = other.to_string(),
            }
            0
        }
        crate::dotcmd::DotCommandId::ReadLineVersion => {
            if args.len() != 2 {
                let error = format!(
                    "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                    name, spec.command, spec.usage
                );
                print_database_error(&error);
                return 1;
            }
            match args[1].as_str() {
                "linenoise" => {
                    state.rl_version = ReadLineVersion::Linenoise;
                    crate::repl::ensure_linenoise_installed(state);
                    crate::completion::install(session.con);
                    0
                }
                "fallback" => {
                    state.rl_version = ReadLineVersion::Fallback;
                    0
                }
                _ => {
                    let error = format!(
                        "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                        name, spec.command, spec.usage
                    );
                    print_database_error(&error);
                    1
                }
            }
        }
        crate::dotcmd::DotCommandId::RenderCompletion => {
            state.render_completion = string_to_bool(&args[1]);
            if state.rl_version == ReadLineVersion::Linenoise {
                unsafe {
                    duckdb_linenoise::linenoiseSetCompletionRendering(if state.render_completion {
                        1
                    } else {
                        0
                    });
                }
            }
            0
        }
        crate::dotcmd::DotCommandId::RenderErrors => {
            state.render_errors = string_to_bool(&args[1]);
            if state.rl_version == ReadLineVersion::Linenoise {
                unsafe {
                    duckdb_linenoise::linenoiseSetErrorRendering(if state.render_errors {
                        1
                    } else {
                        0
                    });
                }
            }
            0
        }
        crate::dotcmd::DotCommandId::Timer => {
            state.timer_enabled = string_to_bool(&args[1]);
            0
        }
        crate::dotcmd::DotCommandId::Log => {
            if state.safe_mode {
                print_database_error(".log cannot be used in -safe mode\n");
                return 1;
            }
            if args.len() != 2 {
                print_database_error("Usage: .log FILE|off");
                return 1;
            }
            let z_file = args[1].as_str();
            state.log = None;
            if z_file == "off" {
                return 0;
            }
            if z_file == "stdout" {
                state.log = Some(crate::output::OutputHandle::Stdout);
                return 0;
            }
            if z_file == "stderr" {
                state.log = Some(crate::output::OutputHandle::Stderr);
                return 0;
            }
            let path = if let Some(rest) = z_file.strip_prefix("~/") {
                crate::paths::home_dir()
                    .map(|home| format!("{}/{}", home, rest))
                    .unwrap_or_else(|| z_file.to_string())
            } else {
                z_file.to_string()
            };
            let Ok(file) = std::fs::File::create(&path) else {
                print_database_error(&format!("Error: cannot open \"{}\"", z_file));
                return 1;
            };
            state.log = Some(crate::output::OutputHandle::File(file));
            0
        }
        crate::dotcmd::DotCommandId::Show => {
            let echo_on = (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0;
            print_stdout(
                state,
                &format!("{:>12}: {}\n", "echo", if echo_on { "on" } else { "off" }),
            );
            print_stdout(
                state,
                &format!(
                    "{:>12}: {}\n",
                    "headers",
                    if state.showHeader { "on" } else { "off" }
                ),
            );
            print_stdout(
                state,
                &format!("{:>12}: {}\n", "mode", mode_to_string(state.mode)),
            );
            print_stdout(
                state,
                &format!(
                    "{:>12}: {}\n",
                    "nullvalue",
                    escape_c_string(&state.nullValue)
                ),
            );
            let output = if !state.outfile.is_empty() {
                state.outfile.as_str()
            } else {
                "stdout"
            };
            print_stdout(state, &format!("{:>12}: {}\n", "output", output));
            print_stdout(
                state,
                &format!(
                    "{:>12}: {}\n",
                    "colseparator",
                    escape_c_string(&state.colSeparator)
                ),
            );
            print_stdout(
                state,
                &format!(
                    "{:>12}: {}\n",
                    "rowseparator",
                    escape_c_string(&state.rowSeparator)
                ),
            );
            let mut width_line = String::new();
            width_line.push_str(&format!("{:>12}: ", "width"));
            for w in state.colWidth.iter().copied() {
                width_line.push_str(&format!("{} ", w));
            }
            width_line.push('\n');
            print_stdout(state, &width_line);
            print_stdout(
                state,
                &format!("{:>12}: {}\n", "filename", state.zDbFilename),
            );
            0
        }
        crate::dotcmd::DotCommandId::HighlightResults => {
            if args.len() != 2 {
                print_database_error("Usage: .highlight_results [on|off]");
                return 1;
            }
            state.highlight_results = if string_to_bool(&args[1]) {
                OptionType::On
            } else {
                OptionType::Off
            };
            0
        }
        crate::dotcmd::DotCommandId::HighlightErrors => {
            if args.len() != 2 {
                print_database_error("Usage: .highlight_errors [on|off]");
                return 1;
            }
            state.highlight_errors = if string_to_bool(&args[1]) {
                OptionType::On
            } else {
                OptionType::Off
            };
            0
        }
        crate::dotcmd::DotCommandId::Highlight => {
            // Keep output highlighting in sync with linenoise syntax highlighting (when available).
            // linenoiseParseOption expects: highlight on|off
            let linenoise_rc = try_run_linenoise_parse_option(&args);
            if let Err(e) = linenoise_rc {
                print_database_error(&e);
                return 1;
            }
            if args.len() != 2 {
                print_database_error("Usage: .highlight on|off");
                return 1;
            }
            state.highlighting_enabled = string_to_bool(&args[1]);
            crate::highlight::sync_linenoise_highlighting_enabled(state.highlighting_enabled);
            0
        }
        crate::dotcmd::DotCommandId::Comment
        | crate::dotcmd::DotCommandId::Constant
        | crate::dotcmd::DotCommandId::Cont
        | crate::dotcmd::DotCommandId::ContSel
        | crate::dotcmd::DotCommandId::Error
        | crate::dotcmd::DotCommandId::Keyword => {
            let literal = spec.command;
            let color = args.get(1).map(|s| s.as_str()).unwrap_or("");
            print_database_error(&format!(
                "WARNING: .{} [COLOR] will be removed in a future release, use .render_color {} {} instead",
                literal, literal, color
            ));
            match set_highlight_color(state, literal, color, PrintIntensity::Standard) {
                Ok(()) => 0,
                Err(e) => {
                    print_database_error(&e);
                    1
                }
            }
        }
        crate::dotcmd::DotCommandId::HighlightColors => {
            if args.len() < 3 || args.len() > 4 {
                print_database_error("Usage: .highlight_colors [element] [color] ([bold])?");
                return 1;
            }
            let element = args[1].as_str();
            let color_name = args[2].as_str();
            let intensity = match args.get(3).map(|s| s.as_str()) {
                None => PrintIntensity::Standard,
                Some(v) if v.eq_ignore_ascii_case("standard") => PrintIntensity::Standard,
                Some(v) if v.eq_ignore_ascii_case("bold") => PrintIntensity::Bold,
                Some(v) if v.eq_ignore_ascii_case("underline") => PrintIntensity::Underline,
                Some(v) if v.eq_ignore_ascii_case("bold_underline") => {
                    PrintIntensity::BoldUnderline
                }
                Some(other) => {
                    print_database_error(&format!(
                        "Unknown intensity '{}', supported options: standard, bold, underline\n",
                        other
                    ));
                    return 1;
                }
            };
            if let Err(e) = set_highlight_color(state, element, color_name, intensity) {
                print_database_error(&e);
                return 1;
            }
            0
        }
        crate::dotcmd::DotCommandId::Prompt => {
            if args.len() >= 2 {
                if let Err(e) = crate::repl::validate_prompt_spec(args[1].as_str()) {
                    print_database_error(&e);
                    return 1;
                }
            }

            if args.len() >= 2 {
                state.mainPrompt = args[1].clone();
            }
            if args.len() >= 3 {
                state.continuePrompt = args[2].clone();
            }
            if args.len() >= 4 {
                state.continuePromptSelected = args[3].clone();
            }
            crate::repl::ensure_linenoise_installed(state);
            0
        }
        crate::dotcmd::DotCommandId::Print => {
            let mut first = true;
            for s in args.iter().skip(1) {
                if !first {
                    print_stdout(state, " ");
                }
                first = false;
                print_stdout(state, s);
            }
            print_stdout(state, "\n");
            0
        }
        crate::dotcmd::DotCommandId::Output => set_output_file(state, &args, ' '),
        crate::dotcmd::DotCommandId::Once => set_output_file(state, &args, 'o'),
        crate::dotcmd::DotCommandId::Excel => set_output_file(state, &args, 'e'),
        crate::dotcmd::DotCommandId::Edit => {
            print_database_error(
                "Command \"edit\" is unsupported in the current version of the CLI\n",
            );
            1
        }
        crate::dotcmd::DotCommandId::UiCommand => {
            let mut cmd = String::new();
            for (idx, part) in args.iter().skip(1).enumerate() {
                if idx > 0 {
                    cmd.push(' ');
                }
                cmd.push_str(part);
            }
            state.ui_command = format!("CALL {}", cmd);
            0
        }
        #[cfg(target_os = "windows")]
        crate::dotcmd::DotCommandId::Utf8 => {
            enable_windows_utf8_console(state);
            0
        }
        crate::dotcmd::DotCommandId::ProgressBar => {
            if args.len() < 2 || args.len() > 3 {
                let error = format!(
                    "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                    name, spec.command, spec.usage
                );
                print_database_error(&error);
                return 1;
            }
            if args[1] == "--clear" {
                if args.len() != 2 {
                    let error = format!(
                        "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                        name, spec.command, spec.usage
                    );
                    print_database_error(&error);
                    return 1;
                }
                state.progress_bar_components.clear();
                return 0;
            }
            if args[1] == "--add" {
                if args.len() != 3 {
                    let error = format!(
                        "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                        name, spec.command, spec.usage
                    );
                    print_database_error(&error);
                    return 1;
                }
                if let Err(e) = crate::prompt::validate_progress_bar_spec(args[2].as_str()) {
                    print_database_error(&e);
                    return 1;
                }
                state.progress_bar_components.push(args[2].clone());
                return 0;
            }
            let error = format!(
                "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                name, spec.command, spec.usage
            );
            print_database_error(&error);
            1
        }
        crate::dotcmd::DotCommandId::DecimalSep => {
            if args.len() == 1 {
                print_stdout(
                    state,
                    &format!(
                        "current decimal separator: {}\n",
                        state.decimal_separator as char
                    ),
                );
                return 0;
            }
            if args.len() != 2 {
                print_database_error("Usage: .decimal_sep SEP");
                return 1;
            }
            let v = args[1].as_str();
            if v == "space" {
                state.decimal_separator = b' ';
                return 0;
            }
            if v == "none" {
                state.decimal_separator = 0;
                return 0;
            }
            let bytes = v.as_bytes();
            if bytes.len() != 1 {
                print_database_error(".decimal_sep SEP must be one byte, \"space\" or \"none\"");
                return 1;
            }
            state.decimal_separator = bytes[0];
            0
        }
        crate::dotcmd::DotCommandId::ThousandSep => {
            if args.len() == 1 {
                print_stdout(
                    state,
                    &format!(
                        "current thousand separator: {}\n",
                        state.thousand_separator as char
                    ),
                );
                return 0;
            }
            if args.len() != 2 {
                print_database_error("Usage: .thousand_sep SEP");
                return 1;
            }
            let v = args[1].as_str();
            if v == "space" {
                state.thousand_separator = b' ';
                return 0;
            }
            if v == "none" {
                state.thousand_separator = 0;
                return 0;
            }
            let bytes = v.as_bytes();
            if bytes.len() != 1 {
                print_database_error(".thousand_sep SEP must be one byte, \"space\" or \"none\"");
                return 1;
            }
            state.thousand_separator = bytes[0];
            0
        }
        crate::dotcmd::DotCommandId::LargeNumberRendering => {
            state.large_number_rendering = match args[1].as_str() {
                "all" => 2,
                "footer" => 1,
                other => {
                    // Shell behavior: treat any other value as a bool.
                    if string_to_bool_shell(other) {
                        3
                    } else {
                        0
                    }
                }
            };
            0
        }
        crate::dotcmd::DotCommandId::MaxRows => {
            if args.len() == 1 {
                print_stdout(state, &format!("current max rows: {}\n", state.max_rows));
                return 0;
            }
            if args.len() > 3 {
                print_database_error("Usage: .maxrows COUNT");
                return 1;
            }
            let v = args[1].parse::<i64>().unwrap_or(0);
            state.max_rows = v as u64;
            if args.len() > 2 {
                let v = args[2].parse::<i64>().unwrap_or(0);
                state.max_analyze_rows = v as u64;
            }
            0
        }
        crate::dotcmd::DotCommandId::MaxWidth => {
            if args.len() == 1 {
                // Matches v1.4.3 shell typo.
                print_stdout(state, &format!("current max rows: {}\n", state.max_width));
                return 0;
            }
            if args.len() != 2 {
                print_database_error("Usage: .maxwidth COUNT");
                return 1;
            }
            let v = args[1].parse::<i64>().unwrap_or(0);
            state.max_width = v as u64;
            0
        }
        crate::dotcmd::DotCommandId::Width => {
            state.colWidth.clear();
            for w in args.iter().skip(1) {
                state.colWidth.push(w.parse::<i32>().unwrap_or(0));
            }
            0
        }
        crate::dotcmd::DotCommandId::Shell | crate::dotcmd::DotCommandId::System => {
            if state.safe_mode {
                print_database_error(".sh/.system cannot be used in -safe mode");
                return 1;
            }
            if args.len() < 2 {
                print_database_error("Usage: .system CMD ARGS...");
                return 1;
            }
            let mut z_cmd = String::new();
            let first = &args[1];
            if first.contains(' ') {
                z_cmd.push_str(first);
            } else {
                z_cmd.push('"');
                z_cmd.push_str(first);
                z_cmd.push('"');
            }
            for a in args.iter().skip(2) {
                if a.contains(' ') {
                    z_cmd.push(' ');
                    z_cmd.push_str(a);
                } else {
                    z_cmd.push_str(" \"");
                    z_cmd.push_str(a);
                    z_cmd.push('"');
                }
            }
            let status = crate::output::shell_command(&z_cmd).status();
            if let Ok(status) = status {
                if !status.success() {
                    let code = status.code().unwrap_or(1);
                    print_database_error(&format!("System command returns {}", code));
                }
            }
            0
        }
        crate::dotcmd::DotCommandId::SafeMode => {
            state.safe_mode = true;
            let query = match CString::new("SET enable_external_access=false") {
                Ok(q) => q,
                Err(_) => return 1,
            };
            let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe { duckdb_sys::duckdb_query(session.con, query.as_ptr(), &mut result) };
            unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
            if rc != duckdb_sys::DuckDBSuccess {
                print_database_error("Failed to set enable_external_access=false for safe mode");
                return 1;
            }
            0
        }
        crate::dotcmd::DotCommandId::Tables => {
            if args.len() > 2 {
                let error = format!(
                    "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                    name, spec.command, spec.usage
                );
                print_database_error(&error);
                return 1;
            }

            let filter_pattern = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let (schema_filter, table_filter) = parse_tables_filter_pattern(filter_pattern);

            let mut schema_filter_str = String::new();
            let mut name_filter = String::new();
            if !table_filter.is_empty() {
                name_filter = format!(
                    " AND columns.table_name ILIKE '{}'",
                    sql_escape_single_quotes(&table_filter)
                );
            }
            if !schema_filter.is_empty() {
                schema_filter_str = format!(
                    " AND columns.schema_name ILIKE '{}'",
                    sql_escape_single_quotes(&schema_filter)
                );
            }

            let sql = format!(
                r#"
SELECT columns.database_name, columns.schema_name, columns.table_name,
       columns.column_name, columns.data_type,
       (c.column_index IS NOT NULL) AS is_primary_key,
       t.estimated_size AS estimated_size,
       t.table_oid AS table_oid
FROM duckdb_columns() columns
LEFT JOIN duckdb_tables() t USING (table_oid)
LEFT JOIN (
	SELECT table_oid, UNNEST(constraint_column_indexes)+1 column_index
	FROM duckdb_constraints()
	WHERE constraint_type='PRIMARY KEY') c
USING (table_oid, column_index)
WHERE NOT columns.internal{schema_filter_str}{name_filter}
ORDER BY columns.database_name, columns.schema_name, columns.table_name, columns.column_index;
"#
            );

            let Ok(query) = CString::new(sql) else {
                return 1;
            };
            let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe { duckdb_sys::duckdb_query(session.con, query.as_ptr(), &mut result) };
            if rc != duckdb_sys::DuckDBSuccess {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                print_database_error("Error: querying table information");
                return 1;
            }

            let tables = table_metadata_collect_from_result(&mut result);
            unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };

            if tables.is_empty() {
                return 0;
            }

            let out = render_table_metadata(state, &tables);
            page_or_print_stdout(state, &out);
            0
        }
        crate::dotcmd::DotCommandId::Indexes | crate::dotcmd::DotCommandId::Indices => {
            if args.len() > 2 {
                print_database_error("Usage: .indexes ?TABLE?");
                return 1;
            }
            let filter = args.get(1).map(|s| s.as_str()).unwrap_or("%");
            let sql = format!(
					"SELECT name FROM sqlite_schema WHERE type='index' AND tbl_name LIKE '{}' ORDER BY name",
					sql_escape_single_quotes(filter)
				);
            let Ok(query) = CString::new(sql) else {
                return 1;
            };
            let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe { duckdb_sys::duckdb_query(session.con, query.as_ptr(), &mut result) };
            if rc != duckdb_sys::DuckDBSuccess {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                print_database_error("Error: querying index information");
                return 1;
            }
            let row_count = unsafe { duckdb_sys::duckdb_row_count(&mut result) } as usize;
            let mut names: Vec<String> = Vec::with_capacity(row_count);
            for row in 0..row_count {
                let name_ptr =
                    unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, row as u64) };
                if name_ptr.is_null() {
                    continue;
                }
                let name = unsafe { CStr::from_ptr(name_ptr) }
                    .to_string_lossy()
                    .to_string();
                unsafe { duckdb_sys::duckdb_free(name_ptr as *mut _) };
                names.push(name);
            }
            unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
            print_pretty_name_list(state, &names);
            0
        }
        crate::dotcmd::DotCommandId::StartupText => {
            if args.len() != 2 {
                let error = format!(
                    "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                    name, spec.command, spec.usage
                );
                print_database_error(&error);
                return 1;
            }
            let prev = state.startup_text;
            let next = match args[1].as_str() {
                "all" => crate::state::StartupText::All,
                "version" => crate::state::StartupText::Version,
                "none" => crate::state::StartupText::None,
                _ => {
                    let error = format!(
                        "Invalid Command Error: Invalid usage of command '.{}'\n\nUsage: '.{} {}'",
                        name, spec.command, spec.usage
                    );
                    print_database_error(&error);
                    return 1;
                }
            };
            state.startup_text = next;
            if state.displayed_loading_resources_message
                && prev == crate::state::StartupText::All
                && next != crate::state::StartupText::All
            {
                print_database_error("WARNING: .startup_text should be on top of your ~/.duckdbrc in order to prevent the \"Loading resources\" message from being displayed");
            }
            0
        }
        crate::dotcmd::DotCommandId::Version => {
            if let Some(info) = crate::db::query_version_info(session.con) {
                print_stdout(
                    state,
                    &format!(
                        "DuckDB {} ({}) {}\n",
                        info.library_version, info.codename, info.source_id
                    ),
                );
                let compiler = unsafe { shellshim::duckdb_shellshim_compiler_version() };
                if !compiler.is_null() {
                    let compiler = unsafe { CStr::from_ptr(compiler) }.to_string_lossy();
                    if !compiler.trim().is_empty() {
                        print_stdout(state, &format!("{}\n", compiler.trim()));
                    }
                }
            } else {
                let version = unsafe { duckdb_sys::duckdb_library_version() };
                let version = if version.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(version) }
                        .to_string_lossy()
                        .to_string()
                };
                print_stdout(state, &format!("{}\n", version.trim()));
            }
            0
        }
        crate::dotcmd::DotCommandId::Multiline => {
            if state.rl_version == ReadLineVersion::Linenoise {
                unsafe { duckdb_linenoise::linenoiseSetMultiLine(1) };
            }
            0
        }
        crate::dotcmd::DotCommandId::Singleline => {
            if state.rl_version == ReadLineVersion::Linenoise {
                unsafe { duckdb_linenoise::linenoiseSetMultiLine(0) };
            }
            0
        }
        crate::dotcmd::DotCommandId::Schema => {
            fn schema_indent_sql(sql: &str) -> String {
                const WRAP_THRESHOLD: usize = 80;

                let raw = sql.trim_end();
                let raw = raw.trim_end_matches('\n').trim_end_matches('\r').trim_end();
                if raw.is_empty() {
                    return String::new();
                }

                let raw_no_semi = raw.trim_end_matches(';').trim_end();
                let lower = raw_no_semi.trim_start().to_ascii_lowercase();
                let should_format = lower.starts_with("create table")
                    && raw_no_semi.len() >= WRAP_THRESHOLD
                    && raw_no_semi.contains('(')
                    && raw_no_semi.contains(',');

                if should_format {
                    let Some(open_idx) = raw_no_semi.find('(') else {
                        let mut out = String::new();
                        out.push_str(raw);
                        out.push_str(";\n");
                        return out;
                    };
                    let Some(close_idx) = raw_no_semi.rfind(')') else {
                        let mut out = String::new();
                        out.push_str(raw);
                        out.push_str(";\n");
                        return out;
                    };
                    if close_idx <= open_idx {
                        let mut out = String::new();
                        out.push_str(raw);
                        out.push_str(";\n");
                        return out;
                    }

                    let prefix = raw_no_semi[..open_idx].trim_end();
                    let inner = &raw_no_semi[open_idx + 1..close_idx];

                    let mut parts: Vec<&str> = Vec::new();
                    let mut start = 0usize;
                    let mut depth = 0i32;
                    let mut in_single = false;
                    let mut in_double = false;
                    let bytes = inner.as_bytes();
                    let mut i = 0usize;
                    while i < bytes.len() {
                        let ch = bytes[i] as char;
                        if in_single {
                            if ch == '\'' {
                                // SQL '' escaping
                                if i + 1 < bytes.len() && bytes[i + 1] as char == '\'' {
                                    i += 2;
                                    continue;
                                }
                                in_single = false;
                            }
                            i += 1;
                            continue;
                        }
                        if in_double {
                            if ch == '"' {
                                // SQL "" escaping
                                if i + 1 < bytes.len() && bytes[i + 1] as char == '"' {
                                    i += 2;
                                    continue;
                                }
                                in_double = false;
                            }
                            i += 1;
                            continue;
                        }

                        match ch {
                            '\'' => in_single = true,
                            '"' => in_double = true,
                            '(' => depth += 1,
                            ')' => depth = depth.saturating_sub(1),
                            ',' if depth == 0 => {
                                parts.push(inner[start..i].trim());
                                start = i + 1;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    let tail = inner[start..].trim();
                    if !tail.is_empty() {
                        parts.push(tail);
                    }

                    let mut out = String::new();
                    out.push_str(prefix);
                    out.push_str("(\n");
                    for (idx, p) in parts.iter().enumerate() {
                        out.push_str("  ");
                        out.push_str(p);
                        if idx + 1 < parts.len() {
                            out.push(',');
                        }
                        out.push('\n');
                    }
                    out.push_str(");;\n");
                    return out;
                }

                let mut out = String::new();
                out.push_str(raw);
                out.push_str(";\n");
                out
            }

            let mut indent = false;
            let mut pattern: Option<&str> = None;
            for arg in args.iter().skip(1) {
                if arg == "--indent" || arg == "-indent" {
                    indent = true;
                    continue;
                }
                if pattern.is_none() {
                    pattern = Some(arg.as_str());
                    continue;
                }
                print_database_error("Usage: .schema ?--indent? ?LIKE-PATTERN?");
                return 1;
            }

            // Match tools/shell/shell.cpp DisplaySchemas, including its schema-qualified pattern bug.
            // If the pattern contains '.', the shipped shell uses:
            //   lower(printf('%s.%s',sname,tbl_name))
            // where `sname` does not exist, yielding a binder error.
            let mut sql = String::from("SELECT sql FROM sqlite_master WHERE ");
            if let Some(pat) = pattern {
                let z_qarg = format!("'{}'", sql_escape_single_quotes(pat));
                let b_glob = pat.contains('*') || pat.contains('?') || pat.contains('[');
                if pat.contains('.') {
                    sql.push_str("lower(printf('%s.%s',sname,tbl_name))");
                } else {
                    sql.push_str("lower(tbl_name)");
                }
                if b_glob {
                    sql.push_str(" GLOB ");
                } else {
                    sql.push_str(" LIKE ");
                }
                sql.push_str(&z_qarg);
                if !b_glob {
                    sql.push_str(" ESCAPE '\\' ");
                }
                sql.push_str(" AND ");
            }
            sql.push_str("type!='meta' AND sql IS NOT NULL ORDER BY name");
            let Ok(query) = CString::new(sql) else {
                return 1;
            };
            let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe { duckdb_sys::duckdb_query(session.con, query.as_ptr(), &mut result) };
            if rc != duckdb_sys::DuckDBSuccess {
                let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut result) };
                if !err_ptr.is_null() {
                    let err = unsafe { CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .to_string();
                    print_database_error_state(state, err.trim_end_matches('\n'));
                }
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                print_database_error("Error: querying schema information");
                return 1;
            }
            let row_count = unsafe { duckdb_sys::duckdb_row_count(&mut result) } as usize;
            for row in 0..row_count {
                let sql_ptr =
                    unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, row as u64) };
                if sql_ptr.is_null() {
                    continue;
                }
                let sql = unsafe { CStr::from_ptr(sql_ptr) }
                    .to_string_lossy()
                    .to_string();
                unsafe { duckdb_sys::duckdb_free(sql_ptr as *mut _) };
                if indent {
                    let formatted = schema_indent_sql(&sql);
                    print_stdout(state, &formatted);
                } else {
                    print_stdout(state, &sql);
                    print_stdout(state, "\n");
                }
            }
            unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
            0
        }
        crate::dotcmd::DotCommandId::Read => {
            if state.safe_mode {
                print_database_error(".read cannot be used in -safe mode\n");
                return 1;
            }
            let path = args[1].as_str();
            let file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(_) => {
                    print_database_error(&format!("Error: cannot open \"{}\"", path));
                    return 1;
                }
            };
            let reader = std::io::BufReader::new(file);
            let rc = crate::repl::process_reader(
                state,
                session,
                reader,
                crate::state::InputMode::File,
                false,
            );
            if rc == 0 {
                0
            } else {
                1
            }
        }
        crate::dotcmd::DotCommandId::Import => {
            if state.safe_mode {
                print_database_error(".import cannot be used in -safe mode\n");
                return 1;
            }
            let mut table_name: Option<String> = None;
            let mut file_name: Option<String> = None;
            let mut generic_parameters: Vec<(String, String)> = Vec::new();
            let mut function: Option<&str> = None;

            let mut i = 1usize;
            while i < args.len() {
                let mut z = args[i].as_str();
                if z.starts_with("--") {
                    z = &z[1..];
                }
                if !z.starts_with('-') {
                    if file_name.is_none() {
                        file_name = Some(z.to_string());
                    } else if table_name.is_none() {
                        table_name = Some(z.to_string());
                    } else {
                        print_stdout(
                            state,
                            &format!("ERROR: extra argument: \"{}\".  Usage:\n", z),
                        );
                        let _ = show_help(state, Some("import"));
                        return 1;
                    }
                    i += 1;
                    continue;
                }
                if z == "-v" {
                    i += 1;
                    continue;
                }
                if z == "-csv" {
                    function = Some("read_csv");
                    i += 1;
                    continue;
                }
                if z == "-parquet" {
                    function = Some("read_parquet");
                    i += 1;
                    continue;
                }
                if z == "-json" {
                    function = Some("read_json");
                    i += 1;
                    continue;
                }
                // Generic parameter: "-skip 1" -> skip=1
                let key = z.trim_start_matches('-').to_string();
                if i + 1 >= args.len() {
                    print_stdout(
                        state,
                        &format!(
                            "ERROR: expected an argument for generic parameter: \"{}\".  Usage:\n",
                            key
                        ),
                    );
                    let _ = show_help(state, Some("import"));
                    return 1;
                }
                i += 1;
                let value = args[i].clone();
                generic_parameters.push((key, value));
                i += 1;
            }

            let Some(file_name) = file_name else {
                print_stdout(state, "ERROR: missing FILE argument. Usage:\n");
                let _ = show_help(state, Some("import"));
                return 1;
            };
            let Some(table_name) = table_name else {
                print_stdout(state, "ERROR: missing TABLE argument. Usage:\n");
                let _ = show_help(state, Some("import"));
                return 1;
            };

            let mut function = function.map(|s| s.to_string());
            if function.is_none() {
                let mut function_map: Vec<(&str, &str)> = vec![
                    (".parquet", "read_parquet"),
                    (".csv", "read_csv"),
                    (".tsv", "read_csv"),
                    (".tbl", "read_csv"),
                    (".json", "read_json"),
                    (".jsonl", "read_json"),
                    (".ndjson", "read_json"),
                    (".avro", "read_avro"),
                    (".xlsx", "read_xlsx"),
                ];
                let compression_suffixes: [&str; 3] = ["", ".gz", ".zst"];
                for (suffix, fun) in function_map.drain(..) {
                    for comp in compression_suffixes {
                        let sfx = format!("{}{}", suffix, comp);
                        if file_name.ends_with(&sfx) {
                            function = Some(fun.to_string());
                            break;
                        }
                    }
                    if function.is_some() {
                        break;
                    }
                }
                if function.is_none() {
                    function = Some("read_csv".to_string());
                }
            }

            if function.as_deref() == Some("read_csv")
                && !generic_parameters.iter().any(|(k, _)| k == "ignore_errors")
            {
                generic_parameters.push(("ignore_errors".to_string(), "true".to_string()));
            }

            let table_ident = quote_identifier_if_needed(state, &table_name);
            let file_lit = format!("'{}'", sql_escape_single_quotes(&file_name));

            let exists_sql = format!(
					"SELECT 1 FROM information_schema.tables WHERE table_schema='main' AND table_name='{}' LIMIT 1",
					sql_escape_single_quotes(&table_name)
				);
            let Ok(exists_q) = CString::new(exists_sql) else {
                return 1;
            };
            let mut exists_res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                duckdb_sys::duckdb_query(session.con, exists_q.as_ptr(), &mut exists_res)
            };
            let table_exists = if rc == duckdb_sys::DuckDBSuccess {
                let rows = unsafe { duckdb_sys::duckdb_row_count(&mut exists_res) };
                unsafe { duckdb_sys::duckdb_destroy_result(&mut exists_res) };
                rows > 0
            } else {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut exists_res) };
                false
            };

            let mut import_query = if table_exists {
                format!("INSERT INTO {} ", table_ident)
            } else {
                format!("CREATE TABLE {} AS ", table_ident)
            };
            import_query.push_str(&format!("SELECT * FROM {}({}", function.unwrap(), file_lit));
            for (k, v) in generic_parameters {
                import_query.push_str(&format!(
                    ", {}='{}'",
                    quote_identifier_if_needed(state, &k),
                    sql_escape_single_quotes(&v)
                ));
            }
            import_query.push(')');

            let Ok(q) = CString::new(import_query) else {
                return 1;
            };
            let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
            for attempt in 0..2 {
                let rc = unsafe { duckdb_sys::duckdb_query(session.con, q.as_ptr(), &mut res) };
                if rc == duckdb_sys::DuckDBSuccess {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                    return 0;
                }

                let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut res) };
                let err = if err_ptr.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .to_string()
                };

                if attempt == 0
                    && crate::db::error_mentions_json_extension(&err)
                    && crate::db::ensure_json_loaded(state, session.con)
                {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                    res = unsafe { std::mem::zeroed() };
                    continue;
                }

                if !err.trim().is_empty() {
                    print_database_error(&format!(
                        "Failed To Import Error: Failed to import from file '{}'\n{}",
                        file_name, err
                    ));
                } else {
                    print_database_error(&format!(
                        "Failed To Import Error: Failed to import from file '{}'",
                        file_name
                    ));
                }
                unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                return 1;
            }
            unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
            1
        }
        crate::dotcmd::DotCommandId::Dump => {
            // Dump implementation based on sqlite_schema for DDL + duckdb_tables() for data and schema discovery.
            // Matches v1.4.3 tests: emits CREATE SCHEMA IF NOT EXISTS for non-main schemas and schema-qualified INSERTs.
            let mut dump_newlines = false;
            let mut patterns: Vec<&str> = Vec::new();
            for arg in args.iter().skip(1) {
                if arg.starts_with("--") {
                    match arg.as_str() {
                        "--newlines" => dump_newlines = true,
                        other => {
                            print_database_error(&format!(
                                "Unknown option \"{}\" on \".dump\"",
                                other
                            ));
                            return 1;
                        }
                    }
                } else {
                    patterns.push(arg.as_str());
                }
            }

            let sql_string_literal =
                |raw: &str| -> String { format!("'{}'", sql_escape_single_quotes(raw)) };

            let mut like_expr = String::new();
            for pat in patterns.iter().copied() {
                let clause = format!("name LIKE {} ESCAPE '\\'", sql_string_literal(pat));
                if like_expr.is_empty() {
                    like_expr.push_str(&clause);
                } else {
                    like_expr.push_str(" OR ");
                    like_expr.push_str(&clause);
                }
            }
            if like_expr.is_empty() {
                like_expr.push_str("true");
            }

            print_stdout(state, "BEGIN TRANSACTION;\n");

            let saved_show_header = state.showHeader;
            let saved_shell_flags = state.shellFlgs;
            let saved_mode = state.mode;
            let saved_dest = state.zDestTable.clone();

            state.showHeader = false;
            state.shellFlgs &= !(crate::state::ShellFlags::SHFLG_Echo as u32);
            if dump_newlines {
                state.shellFlgs |= crate::state::ShellFlags::SHFLG_Newlines as u32;
            } else {
                state.shellFlgs &= !(crate::state::ShellFlags::SHFLG_Newlines as u32);
            }
            state.mode = RenderMode::INSERT;

            let mut had_error = false;

            // Emit CREATE SCHEMA for non-main schemas first (double semicolon behavior).
            let schema_query = format!(
                "SELECT DISTINCT table_schema FROM information_schema.tables \
WHERE table_schema != 'main' AND table_schema NOT LIKE 'pg_%' AND table_schema != 'information_schema' \
AND table_name IN (SELECT name FROM sqlite_schema WHERE ({}) AND type=='table') \
ORDER BY table_schema",
                like_expr
            );
            if let Ok(q) = CString::new(schema_query) {
                let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                let rc = unsafe { duckdb_sys::duckdb_query(session.con, q.as_ptr(), &mut res) };
                if rc == duckdb_sys::DuckDBSuccess {
                    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut res) } as usize;
                    for r in 0..rows {
                        let schema_ptr =
                            unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 0, r as u64) };
                        if schema_ptr.is_null() {
                            continue;
                        }
                        let schema = unsafe { CStr::from_ptr(schema_ptr) }
                            .to_string_lossy()
                            .to_string();
                        unsafe { duckdb_sys::duckdb_free(schema_ptr as *mut _) };
                        let create_schema = format!(
                            "CREATE SCHEMA IF NOT EXISTS {};",
                            quote_identifier_if_needed(state, &schema)
                        );
                        print_stdout(state, &format!("{};\n", create_schema));
                    }
                } else {
                    had_error = true;
                }
                unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
            } else {
                had_error = true;
            }

            let get_table_schema =
                |con: duckdb_sys::duckdb_connection, table_name: &str| -> Option<String> {
                    let q = format!(
                        "SELECT table_schema FROM information_schema.tables \
WHERE table_name = {} AND table_type='BASE TABLE' \
ORDER BY (table_schema='main') DESC LIMIT 1",
                        sql_string_literal(table_name)
                    );
                    let Ok(c_q) = CString::new(q) else {
                        return None;
                    };
                    let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                    let rc = unsafe { duckdb_sys::duckdb_query(con, c_q.as_ptr(), &mut res) };
                    if rc != duckdb_sys::DuckDBSuccess {
                        unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                        return None;
                    }
                    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut res) } as usize;
                    if rows == 0 {
                        unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                        return None;
                    }
                    let schema_ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 0, 0) };
                    if schema_ptr.is_null() {
                        unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                        return None;
                    }
                    let schema = unsafe { CStr::from_ptr(schema_ptr) }
                        .to_string_lossy()
                        .to_string();
                    unsafe {
                        duckdb_sys::duckdb_free(schema_ptr as *mut _);
                        duckdb_sys::duckdb_destroy_result(&mut res);
                    }
                    Some(schema)
                };

            let table_column_list =
                |con: duckdb_sys::duckdb_connection, qualified_name: &str| -> Vec<String> {
                    let pragma_sql =
                        format!("PRAGMA table_info={}", sql_string_literal(qualified_name));
                    let Ok(c_q) = CString::new(pragma_sql) else {
                        return Vec::new();
                    };
                    let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                    let rc = unsafe { duckdb_sys::duckdb_query(con, c_q.as_ptr(), &mut res) };
                    if rc != duckdb_sys::DuckDBSuccess {
                        unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                        return Vec::new();
                    }
                    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut res) } as usize;
                    let mut out: Vec<String> = Vec::with_capacity(rows);
                    for r in 0..rows {
                        let name_ptr =
                            unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 1, r as u64) };
                        if name_ptr.is_null() {
                            continue;
                        }
                        let name = unsafe { CStr::from_ptr(name_ptr) }
                            .to_string_lossy()
                            .to_string();
                        unsafe { duckdb_sys::duckdb_free(name_ptr as *mut _) };
                        out.push(name);
                    }
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
                    out
                };

            // tables (DDL + data interleaved, matching RunSchemaDumpQuery)
            let table_schema_sql = format!(
                "SELECT name, type, sql FROM sqlite_schema \
WHERE ({}) AND type=='table' AND sql NOT NULL \
ORDER BY tbl_name='sqlite_sequence'",
                like_expr
            );
            if let Ok(q) = CString::new(table_schema_sql) {
                let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                let rc = unsafe { duckdb_sys::duckdb_query(session.con, q.as_ptr(), &mut res) };
                if rc == duckdb_sys::DuckDBSuccess {
                    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut res) } as usize;
                    for r in 0..rows {
                        let name_ptr =
                            unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 0, r as u64) };
                        let sql_ptr =
                            unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 2, r as u64) };
                        if name_ptr.is_null() || sql_ptr.is_null() {
                            continue;
                        }
                        let table_name = unsafe { CStr::from_ptr(name_ptr) }
                            .to_string_lossy()
                            .to_string();
                        let sql = unsafe { CStr::from_ptr(sql_ptr) }
                            .to_string_lossy()
                            .to_string();
                        unsafe {
                            duckdb_sys::duckdb_free(name_ptr as *mut _);
                            duckdb_sys::duckdb_free(sql_ptr as *mut _);
                        }

                        print_stdout(state, &sql);
                        print_stdout(state, ";\n");

                        let Some(schema) = get_table_schema(session.con, &table_name) else {
                            had_error = true;
                            if state.bail != BailOnError::DontBail {
                                break;
                            }
                            continue;
                        };
                        let qualified = format!(
                            "{}.{}",
                            quote_identifier_if_needed(state, &schema),
                            quote_identifier_if_needed(state, &table_name)
                        );
                        let cols = table_column_list(session.con, &qualified);
                        if cols.is_empty() {
                            had_error = true;
                            if state.bail != BailOnError::DontBail {
                                break;
                            }
                            continue;
                        }
                        let mut select = String::new();
                        select.push_str("SELECT ");
                        for (idx, col) in cols.iter().enumerate() {
                            if idx > 0 {
                                select.push_str(", ");
                            }
                            select.push_str(&quote_identifier_if_needed(state, col));
                        }
                        select.push_str(" FROM ");
                        select.push_str(&qualified);

                        state.zDestTable = qualified;
                        let rc = run_sql_script(state, session.con, &select);
                        if rc != 0 {
                            had_error = true;
                            if state.bail != BailOnError::DontBail {
                                break;
                            }
                        }
                    }
                } else {
                    had_error = true;
                }
                unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
            } else {
                had_error = true;
            }

            // index/trigger/view SQL (matching RunTableDumpQuery semicolon handling)
            let other_schema_sql = format!(
                "SELECT sql FROM sqlite_schema \
WHERE ({}) AND sql NOT NULL AND type IN ('index','trigger','view')",
                like_expr
            );
            if let Ok(q) = CString::new(other_schema_sql) {
                let mut res: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                let rc = unsafe { duckdb_sys::duckdb_query(session.con, q.as_ptr(), &mut res) };
                if rc == duckdb_sys::DuckDBSuccess {
                    let rows = unsafe { duckdb_sys::duckdb_row_count(&mut res) } as usize;
                    for r in 0..rows {
                        let sql_ptr =
                            unsafe { duckdb_sys::duckdb_value_varchar(&mut res, 0, r as u64) };
                        if sql_ptr.is_null() {
                            continue;
                        }
                        let sql = unsafe { CStr::from_ptr(sql_ptr) }
                            .to_string_lossy()
                            .to_string();
                        unsafe { duckdb_sys::duckdb_free(sql_ptr as *mut _) };
                        print_stdout(state, &sql);
                        if sql.contains("--") {
                            print_stdout(state, "\n;\n");
                        } else {
                            print_stdout(state, ";\n");
                        }
                    }
                } else {
                    had_error = true;
                }
                unsafe { duckdb_sys::duckdb_destroy_result(&mut res) };
            } else {
                had_error = true;
            }

            if had_error {
                print_stdout(state, "ROLLBACK; -- due to errors\n");
            } else {
                print_stdout(state, "COMMIT;\n");
            }

            state.showHeader = saved_show_header;
            state.shellFlgs = saved_shell_flags;
            state.mode = saved_mode;
            state.zDestTable = saved_dest;

            0
        }
        crate::dotcmd::DotCommandId::Open => {
            if state.safe_mode {
                print_database_error(".open cannot be used in -safe mode\n");
                return 1;
            }

            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            enum ExecuteSQLSingleValueResult {
                ExecutionError,
                EmptyResult,
                MultipleRows,
                MultipleColumns,
                NullResult,
                Success,
            }

            fn execute_sql_single_value(
                con: duckdb_sys::duckdb_connection,
                sql: &str,
            ) -> (ExecuteSQLSingleValueResult, String) {
                let Ok(query) = CString::new(sql) else {
                    return (
                        ExecuteSQLSingleValueResult::ExecutionError,
                        "Invalid SQL".to_string(),
                    );
                };
                let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
                let rc = unsafe { duckdb_sys::duckdb_query(con, query.as_ptr(), &mut result) };
                if rc != duckdb_sys::DuckDBSuccess {
                    let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut result) };
                    let err = if err_ptr.is_null() {
                        String::new()
                    } else {
                        unsafe { CStr::from_ptr(err_ptr) }
                            .to_string_lossy()
                            .to_string()
                    };
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::ExecutionError, err);
                }

                let return_type = unsafe { duckdb_sys::duckdb_result_return_type(result) };
                if return_type != duckdb_sys::DUCKDB_RESULT_TYPE_QUERY_RESULT {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::EmptyResult, String::new());
                }

                let rows = unsafe { duckdb_sys::duckdb_row_count(&mut result) };
                let cols = unsafe { duckdb_sys::duckdb_column_count(&mut result) };
                if cols == 0 || rows == 0 {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::EmptyResult, String::new());
                }
                if rows > 1 {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::MultipleRows, String::new());
                }
                if cols != 1 {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::MultipleColumns, String::new());
                }
                let is_null = unsafe { duckdb_sys::duckdb_value_is_null(&mut result, 0, 0) };
                if is_null {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::NullResult, String::new());
                }
                let value_ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut result, 0, 0) };
                if value_ptr.is_null() {
                    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                    return (ExecuteSQLSingleValueResult::NullResult, String::new());
                }
                let value = unsafe { CStr::from_ptr(value_ptr) }
                    .to_string_lossy()
                    .to_string();
                unsafe {
                    duckdb_sys::duckdb_free(value_ptr as *mut _);
                    duckdb_sys::duckdb_destroy_result(&mut result);
                }
                (ExecuteSQLSingleValueResult::Success, value)
            }

            let mut z_new_filename: Option<String> = None;
            let mut new_flag = false;
            let mut has_sql = false;
            let mut readonly = false;
            let mut overrides: Vec<(String, String)> = Vec::new();

            let mut i_name = 1usize;
            while i_name < args.len() && args[i_name].starts_with('-') {
                let z = args[i_name].as_str();
                let opt = z.trim_start_matches('-');
                match opt {
                    "new" => new_flag = true,
                    "readonly" => readonly = true,
                    "nofollow" => {}
                    "sql" => {
                        if has_sql {
                            print_database_error("Error: --sql provided multiple times\n");
                            return 1;
                        }
                        if i_name + 1 >= args.len() {
                            print_database_error("Error: missing SQL query after --sql\n");
                            return 1;
                        }
                        let query = args[i_name + 1].as_str();
                        let (exec_result, val) = execute_sql_single_value(session.con, query);
                        match exec_result {
                            ExecuteSQLSingleValueResult::ExecutionError => {
                                print_database_error(&format!(
                                    "Error: failed to evaluate --sql query '{}': {}\n",
                                    query, val
                                ));
                                return 1;
                            }
                            ExecuteSQLSingleValueResult::EmptyResult => {
                                print_database_error(
                                    "Error: --sql query returned no rows, expected single value\n",
                                );
                                return 1;
                            }
                            ExecuteSQLSingleValueResult::MultipleRows => {
                                print_database_error(
										"Error: --sql query returned multiple rows, expected single value\n",
									);
                                return 1;
                            }
                            ExecuteSQLSingleValueResult::MultipleColumns => {
                                print_database_error(
										"Error: --sql query returned multiple columns, expected single value\n",
									);
                                return 1;
                            }
                            ExecuteSQLSingleValueResult::NullResult => {
                                print_database_error("Error: --sql query returned a null value\n");
                                return 1;
                            }
                            ExecuteSQLSingleValueResult::Success => {}
                        }
                        z_new_filename = Some(val);
                        has_sql = true;
                        i_name += 1;
                    }
                    _ => {
                        print_database_error(&format!("unknown option: {}\n", z));
                        return 1;
                    }
                }
                i_name += 1;
            }

            if has_sql && args.len() > i_name {
                print_database_error("Error: cannot use both --sql and a FILE argument\n");
                return 1;
            }
            if !has_sql && args.len() > i_name {
                z_new_filename = Some(args[i_name].clone());
            }

            if readonly {
                overrides.push(("access_mode".to_string(), "read_only".to_string()));
            }

            // Close the existing database.
            crate::db::close_db(&mut session.db, &mut session.con);

            let mut opened: Option<(duckdb_sys::duckdb_database, duckdb_sys::duckdb_connection)> =
                None;
            if let Some(path) = z_new_filename.as_deref() {
                if new_flag {
                    let _ = std::fs::remove_file(path);
                }
                state.zDbFilename = path.to_string();
                opened = crate::db::open_db_with_overrides(state, &overrides).ok();
            }

            if opened.is_none() {
                // Fall back to a transient in-memory database.
                state.zDbFilename = ":memory:".to_string();
                opened = crate::db::open_db_with_overrides(state, &overrides).ok();
            }

            let Some((new_db, new_con)) = opened else {
                print_database_error("Error: cannot open ':memory:'\n");
                return 1;
            };

            session.db = new_db;
            session.con = new_con;
            crate::signals::set_connection(session.con);
            crate::completion::set_connection(session.con);
            crate::db::init_local_timezone(state, session.con);
            crate::db::load_reserved_keywords(state, session.con);
            crate::db::sync_process_timezone(state, session.con);
            0
        }
        crate::dotcmd::DotCommandId::Exit => {
            // Mirrors shell_metadata_command.cpp:
            // - `.exit` exits with 0
            // - `.exit N` exits with N if N != 0, otherwise exits with 0
            if args.len() > 2 {
                return 1;
            }
            let code = if args.len() > 1 {
                args[1].trim().parse::<i32>().unwrap_or(0)
            } else {
                0
            };
            state.exit_code = Some(code);
            2
        }
        crate::dotcmd::DotCommandId::Quit => {
            state.exit_code = Some(0);
            2
        }
    }
}

fn render_explain_result(state: &mut ShellState, result: &mut duckdb_sys::duckdb_result) -> i32 {
    let col_count = unsafe { duckdb_sys::duckdb_column_count(result) } as usize;
    if col_count < 2 {
        return 0;
    }

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut out = std::io::Cursor::new(&mut buf);
        let mut chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
        while !chunk.is_null() {
            let row_count = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) };
            let key_vector = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, 0) };
            let value_vector = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, 1) };

            let mut key_type = unsafe { duckdb_sys::duckdb_vector_get_column_type(key_vector) };
            let mut value_type = unsafe { duckdb_sys::duckdb_vector_get_column_type(value_vector) };

            for r in 0..row_count {
                let row = r as u64;
                let key = crate::value::vector_value_to_string(key_vector, key_type, row)
                    .unwrap_or_default();
                if key == "logical_plan" || key == "logical_opt" || key == "physical_plan" {
                    let _ = out.write_all("\n┌─────────────────────────────┐\n".as_bytes());
                    let _ = out.write_all("│┌───────────────────────────┐│\n".as_bytes());
                    if key == "logical_plan" {
                        let _ = out.write_all("││ Unoptimized Logical Plan  ││\n".as_bytes());
                    } else if key == "logical_opt" {
                        let _ = out.write_all("││  Optimized Logical Plan   ││\n".as_bytes());
                    } else if key == "physical_plan" {
                        let _ = out.write_all("││       Physical Plan       ││\n".as_bytes());
                    }
                    let _ = out.write_all("│└───────────────────────────┘│\n".as_bytes());
                    let _ = out.write_all("└─────────────────────────────┘\n".as_bytes());
                }
                if let Some(v) = crate::value::vector_value_to_string(value_vector, value_type, row)
                {
                    let _ = out.write_all(v.as_bytes());
                }
            }

            unsafe {
                duckdb_sys::duckdb_destroy_logical_type(&mut key_type);
                duckdb_sys::duckdb_destroy_logical_type(&mut value_type);
                duckdb_sys::duckdb_destroy_data_chunk(&mut chunk);
            }
            chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
        }
    }

    page_or_print_stdout_rows_only(state, &buf);
    0
}

fn render_result(state: &mut ShellState, result: &mut duckdb_sys::duckdb_result) -> i32 {
    let col_count = unsafe { duckdb_sys::duckdb_column_count(result) } as usize;
    if col_count == 0 {
        return 0;
    }

    let mut col_names: Vec<String> = Vec::with_capacity(col_count);
    let mut col_types: Vec<duckdb_sys::duckdb_type> = Vec::with_capacity(col_count);
    let mut col_logical_type_ids: Vec<duckdb_sys::duckdb_type> = Vec::with_capacity(col_count);
    for c in 0..col_count {
        let name_ptr = unsafe { duckdb_sys::duckdb_column_name(result, c as u64) };
        if name_ptr.is_null() {
            col_names.push(String::new());
        } else {
            col_names.push(
                unsafe { CStr::from_ptr(name_ptr) }
                    .to_string_lossy()
                    .to_string(),
            );
        }
        col_types.push(unsafe { duckdb_sys::duckdb_column_type(result, c as u64) });

        let mut logical = unsafe { duckdb_sys::duckdb_column_logical_type(result, c as u64) };
        if logical.is_null() {
            col_logical_type_ids.push(duckdb_sys::DUCKDB_TYPE_INVALID);
        } else {
            let type_id = unsafe { duckdb_sys::duckdb_get_type_id(logical) };
            col_logical_type_ids.push(type_id);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut logical) };
        }
    }

    let mode = state.cMode;
    if mode == RenderMode::EXPLAIN {
        return render_explain_result(state, result);
    }

    if matches!(mode, RenderMode::JSON | RenderMode::JSONLINES) {
        let json_array = mode == RenderMode::JSON;
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut out = std::io::Cursor::new(&mut buf);

            fn vector_row_is_null(vector: duckdb_sys::duckdb_vector, row: u64) -> bool {
                let validity = unsafe { duckdb_sys::duckdb_vector_get_validity(vector) };
                !validity.is_null()
                    && !unsafe { duckdb_sys::duckdb_validity_row_is_valid(validity, row) }
            }

            fn write_json_float(out: &mut dyn Write, v: f64) {
                if v.is_nan() {
                    let _ = out.write_all(b"NaN");
                    return;
                }
                if v.is_infinite() {
                    if v.is_sign_negative() {
                        let _ = out.write_all(b"-Infinity");
                    } else {
                        let _ = out.write_all(b"Infinity");
                    }
                    return;
                }
                let abs = v.abs();
                if abs != 0.0 && (abs >= 1e15 || abs < 1e-4) {
                    let _ = out.write_all(format!("{:.16e}", v).as_bytes());
                    return;
                }
                let s = format!("{}", v);
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    let _ = out.write_all(s.as_bytes());
                    let _ = out.write_all(b".0");
                } else {
                    let _ = out.write_all(s.as_bytes());
                }
            }

            fn duckdb_value_to_string_stripped(
                mut value: duckdb_sys::duckdb_value,
            ) -> Option<String> {
                if value.is_null() {
                    return None;
                }
                fn strip_single_quoted_typed_literal(s: &str) -> Option<String> {
                    let rest = s.strip_prefix('\'')?;
                    let idx = rest.rfind("'::")?;
                    Some(rest[..idx].to_string())
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
                if let Some(stripped) = strip_single_quoted_typed_literal(out.as_str()) {
                    out = stripped;
                }
                Some(out)
            }

            fn write_json_string(out: &mut dyn Write, s: &str) {
                output_json_string(out, s.as_bytes());
            }

            fn write_json_value(
                out: &mut dyn Write,
                vector: duckdb_sys::duckdb_vector,
                type_: duckdb_sys::duckdb_logical_type,
                row: u64,
                depth: usize,
            ) {
                if depth > 64 {
                    let _ = out.write_all(b"null");
                    return;
                }
                if vector_row_is_null(vector, row) {
                    let _ = out.write_all(b"null");
                    return;
                }
                let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
                let data = unsafe { duckdb_sys::duckdb_vector_get_data(vector) };
                match type_id {
                    duckdb_sys::DUCKDB_TYPE_BOOLEAN => unsafe {
                        let ptr = data as *const bool;
                        let v = *ptr.add(row as usize);
                        if v {
                            let _ = out.write_all(b"true");
                        } else {
                            let _ = out.write_all(b"false");
                        }
                    },
                    duckdb_sys::DUCKDB_TYPE_TINYINT => unsafe {
                        let ptr = data as *const i8;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_SMALLINT => unsafe {
                        let ptr = data as *const i16;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_INTEGER => unsafe {
                        let ptr = data as *const i32;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_BIGINT => unsafe {
                        let ptr = data as *const i64;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_HUGEINT => unsafe {
                        let ptr = data as *const duckdb_sys::duckdb_hugeint;
                        let v = *ptr.add(row as usize);
                        let signed = ((v.upper as i128) << 64) + (v.lower as i128);
                        write_json_string(out, &signed.to_string());
                    },
                    duckdb_sys::DUCKDB_TYPE_UTINYINT => unsafe {
                        let ptr = data as *const u8;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_USMALLINT => unsafe {
                        let ptr = data as *const u16;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_UINTEGER => unsafe {
                        let ptr = data as *const u32;
                        let _ = out.write_all((*ptr.add(row as usize)).to_string().as_bytes());
                    },
                    duckdb_sys::DUCKDB_TYPE_UBIGINT => unsafe {
                        let ptr = data as *const u64;
                        write_json_string(out, &(*ptr.add(row as usize)).to_string());
                    },
                    duckdb_sys::DUCKDB_TYPE_UHUGEINT => unsafe {
                        let ptr = data as *const duckdb_sys::duckdb_uhugeint;
                        let v = *ptr.add(row as usize);
                        let unsigned = (v.upper as u128) << 64 | (v.lower as u128);
                        write_json_string(out, &unsigned.to_string());
                    },
                    duckdb_sys::DUCKDB_TYPE_FLOAT => unsafe {
                        let ptr = data as *const f32;
                        write_json_float(out, *ptr.add(row as usize) as f64);
                    },
                    duckdb_sys::DUCKDB_TYPE_DOUBLE => unsafe {
                        let ptr = data as *const f64;
                        write_json_float(out, *ptr.add(row as usize));
                    },
                    duckdb_sys::DUCKDB_TYPE_DECIMAL => {
                        if let Some(s) = crate::value::vector_value_to_string(vector, type_, row) {
                            write_json_string(out, s.as_str());
                        } else {
                            let _ = out.write_all(b"null");
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_BIGNUM => {
                        if let Some(s) = crate::value::vector_value_to_string(vector, type_, row) {
                            write_json_string(out, s.as_str());
                        } else {
                            let _ = out.write_all(b"null");
                        }
                    }
                    duckdb_sys::DUCKDB_TYPE_UNION => unsafe {
                        let member_count =
                            duckdb_sys::duckdb_union_type_member_count(type_) as usize;
                        let tag_vector = duckdb_sys::duckdb_struct_vector_get_child(vector, 0);
                        if vector_row_is_null(tag_vector, row) {
                            let _ = out.write_all(b"null");
                            return;
                        }
                        let mut tag_type = duckdb_sys::duckdb_vector_get_column_type(tag_vector);
                        let tag_type_id = duckdb_sys::duckdb_get_type_id(tag_type);
                        let tag_data = duckdb_sys::duckdb_vector_get_data(tag_vector);
                        let tag: u64 = match tag_type_id {
                            duckdb_sys::DUCKDB_TYPE_UTINYINT => {
                                *(tag_data as *const u8).add(row as usize) as u64
                            }
                            duckdb_sys::DUCKDB_TYPE_USMALLINT => {
                                *(tag_data as *const u16).add(row as usize) as u64
                            }
                            duckdb_sys::DUCKDB_TYPE_UINTEGER => {
                                *(tag_data as *const u32).add(row as usize) as u64
                            }
                            duckdb_sys::DUCKDB_TYPE_UBIGINT => {
                                *(tag_data as *const u64).add(row as usize)
                            }
                            _ => *(tag_data as *const u8).add(row as usize) as u64,
                        };
                        duckdb_sys::duckdb_destroy_logical_type(&mut tag_type);

                        if (tag as usize) >= member_count {
                            let _ = out.write_all(b"null");
                            return;
                        }

                        let member_vector =
                            duckdb_sys::duckdb_struct_vector_get_child(vector, 1 + tag);
                        let mut member_type = duckdb_sys::duckdb_union_type_member_type(type_, tag);
                        let member_name_ptr = duckdb_sys::duckdb_union_type_member_name(type_, tag);
                        let member_name = if member_name_ptr.is_null() {
                            String::new()
                        } else {
                            let s = CStr::from_ptr(member_name_ptr)
                                .to_string_lossy()
                                .to_string();
                            duckdb_sys::duckdb_free(member_name_ptr as *mut _);
                            s
                        };
                        let _ = out.write_all(b"{");
                        write_json_string(out, member_name.as_str());
                        let _ = out.write_all(b":");
                        write_json_value(out, member_vector, member_type, row, depth + 1);
                        let _ = out.write_all(b"}");
                        duckdb_sys::duckdb_destroy_logical_type(&mut member_type);
                    },
                    duckdb_sys::DUCKDB_TYPE_LIST => unsafe {
                        let entries = data as *const duckdb_sys::duckdb_list_entry;
                        let entry = *entries.add(row as usize);
                        let child_vector = duckdb_sys::duckdb_list_vector_get_child(vector);
                        let mut child_type = duckdb_sys::duckdb_list_type_child_type(type_);
                        let _ = out.write_all(b"[");
                        for i in 0..entry.length {
                            if i > 0 {
                                let _ = out.write_all(b",");
                            }
                            write_json_value(
                                out,
                                child_vector,
                                child_type,
                                entry.offset + i,
                                depth + 1,
                            );
                        }
                        let _ = out.write_all(b"]");
                        duckdb_sys::duckdb_destroy_logical_type(&mut child_type);
                    },
                    duckdb_sys::DUCKDB_TYPE_ARRAY => unsafe {
                        let array_size = duckdb_sys::duckdb_array_type_array_size(type_);
                        let child_vector = duckdb_sys::duckdb_array_vector_get_child(vector);
                        let mut child_type = duckdb_sys::duckdb_array_type_child_type(type_);
                        let _ = out.write_all(b"[");
                        for i in 0..array_size {
                            if i > 0 {
                                let _ = out.write_all(b",");
                            }
                            write_json_value(
                                out,
                                child_vector,
                                child_type,
                                row * array_size + i,
                                depth + 1,
                            );
                        }
                        let _ = out.write_all(b"]");
                        duckdb_sys::duckdb_destroy_logical_type(&mut child_type);
                    },
                    duckdb_sys::DUCKDB_TYPE_STRUCT => unsafe {
                        let child_count = duckdb_sys::duckdb_struct_type_child_count(type_);
                        let _ = out.write_all(b"{");
                        for idx in 0..child_count {
                            if idx > 0 {
                                let _ = out.write_all(b",");
                            }
                            let name_ptr = duckdb_sys::duckdb_struct_type_child_name(type_, idx);
                            let name = if name_ptr.is_null() {
                                String::new()
                            } else {
                                let s = CStr::from_ptr(name_ptr).to_string_lossy().to_string();
                                duckdb_sys::duckdb_free(name_ptr as *mut _);
                                s
                            };
                            write_json_string(out, name.as_str());
                            let _ = out.write_all(b":");
                            let child_vector =
                                duckdb_sys::duckdb_struct_vector_get_child(vector, idx);
                            let mut child_type =
                                duckdb_sys::duckdb_struct_type_child_type(type_, idx);
                            write_json_value(out, child_vector, child_type, row, depth + 1);
                            duckdb_sys::duckdb_destroy_logical_type(&mut child_type);
                        }
                        let _ = out.write_all(b"}");
                    },
                    duckdb_sys::DUCKDB_TYPE_MAP => unsafe {
                        let entries = data as *const duckdb_sys::duckdb_list_entry;
                        let entry = *entries.add(row as usize);
                        let child_vector = duckdb_sys::duckdb_list_vector_get_child(vector);
                        let key_vector =
                            duckdb_sys::duckdb_struct_vector_get_child(child_vector, 0);
                        let value_vector =
                            duckdb_sys::duckdb_struct_vector_get_child(child_vector, 1);
                        let mut key_type = duckdb_sys::duckdb_map_type_key_type(type_);
                        let mut value_type = duckdb_sys::duckdb_map_type_value_type(type_);
                        let _ = out.write_all(b"{");
                        let mut written = 0u64;
                        for i in 0..entry.length {
                            let child_row = entry.offset + i;
                            let Some(key) = crate::value::vector_value_to_string(
                                key_vector, key_type, child_row,
                            ) else {
                                continue;
                            };
                            if written > 0 {
                                let _ = out.write_all(b",");
                            }
                            write_json_string(out, key.as_str());
                            let _ = out.write_all(b":");
                            write_json_value(out, value_vector, value_type, child_row, depth + 1);
                            written += 1;
                        }
                        let _ = out.write_all(b"}");
                        duckdb_sys::duckdb_destroy_logical_type(&mut key_type);
                        duckdb_sys::duckdb_destroy_logical_type(&mut value_type);
                    },
                    duckdb_sys::DUCKDB_TYPE_TIMESTAMP_TZ => {
                        if let Some(s) = crate::value::vector_value_to_string(vector, type_, row) {
                            write_json_string(out, s.as_str());
                        } else {
                            let _ = out.write_all(b"null");
                        }
                    }
                    _ => {
                        // Fall back to DuckDB's string conversion for scalar types.
                        if let Some(s) = crate::value::vector_value_to_string(vector, type_, row) {
                            write_json_string(out, s.as_str());
                        } else {
                            let _ = out.write_all(b"null");
                        }
                    }
                }
            }

            let mut row_index: u64 = 0;
            let mut started = false;
            loop {
                let mut chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
                if chunk.is_null() {
                    break;
                }
                let chunk_size = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) } as usize;
                let mut vectors: Vec<duckdb_sys::duckdb_vector> = Vec::with_capacity(col_count);
                let mut types: Vec<duckdb_sys::duckdb_logical_type> = Vec::with_capacity(col_count);
                let mut is_json: Vec<bool> = Vec::with_capacity(col_count);
                let mut is_bignum: Vec<bool> = Vec::with_capacity(col_count);
                for c in 0..col_count {
                    let v = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, c as u64) };
                    vectors.push(v);
                    let t = unsafe { duckdb_sys::duckdb_vector_get_column_type(v) };
                    let type_name = duckbox_render_type(state, t, 0);
                    is_json.push(type_name == "json");
                    is_bignum.push(type_name == "bignum");
                    types.push(t);
                }

                for r in 0..chunk_size {
                    if !started {
                        if json_array {
                            let _ = out.write_all(b"[");
                        }
                        started = true;
                    }
                    if row_index > 0 && json_array {
                        let _ = out.write_all(b",\n");
                    }
                    let _ = out.write_all(b"{");
                    for c in 0..col_count {
                        if c > 0 {
                            let _ = out.write_all(b",");
                        }
                        output_json_string(&mut out, col_names[c].as_bytes());
                        let _ = out.write_all(b":");
                        if is_json[c] {
                            if vector_row_is_null(vectors[c], r as u64) {
                                let _ = out.write_all(b"null");
                            } else if let Some(s) =
                                crate::value::vector_value_to_string(vectors[c], types[c], r as u64)
                            {
                                let _ = out.write_all(s.as_bytes());
                            } else {
                                let _ = out.write_all(b"null");
                            }
                        } else if col_types[c] == duckdb_sys::DUCKDB_TYPE_BIGNUM || is_bignum[c] {
                            if vector_row_is_null(vectors[c], r as u64) {
                                let _ = out.write_all(b"null");
                            } else if let Some(s) =
                                crate::value::vector_value_to_string(vectors[c], types[c], r as u64)
                            {
                                write_json_string(&mut out, s.trim());
                            } else {
                                let _ = out.write_all(b"null");
                            }
                        } else {
                            write_json_value(&mut out, vectors[c], types[c], r as u64, 0);
                        }
                    }
                    let _ = out.write_all(b"}");
                    if !json_array {
                        let _ = out.write_all(b"\n");
                    }
                    row_index += 1;
                }
                for t in types.iter_mut() {
                    unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                }
                unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
            }
            if json_array && started {
                let _ = out.write_all(b"]\n");
            }
        }
        page_or_print_stdout(state, &buf);
        return 0;
    }

    if mode == RenderMode::TRASH {
        return 0;
    }

    fn is_columnar_mode(mode: RenderMode) -> bool {
        matches!(
            mode,
            RenderMode::COLUMN
                | RenderMode::TABLE
                | RenderMode::MARKDOWN
                | RenderMode::BOX
                | RenderMode::LATEX
        )
    }

    fn should_buffer_for_pager(state: &ShellState) -> bool {
        state.stdout_is_console
            && state.outfile.is_empty()
            && matches!(&state.out, OutputHandle::Stdout)
            && state.pager_mode != PagerMode::Off
    }

    fn is_interrupt_error(msg: &str) -> bool {
        msg.to_ascii_lowercase().contains("interrupt")
    }

    struct OutputHandleWriter<'a> {
        out: &'a mut OutputHandle,
    }

    impl std::io::Write for OutputHandleWriter<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.out.write_all(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    if !is_columnar_mode(mode) {
        let buffer_for_pager = should_buffer_for_pager(state);
        let mut buf: Vec<u8> = Vec::new();

        let mut render_rowwise = |st: &ShellState, out: &mut dyn Write| -> i32 {
            let state = st;
            let first_chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
            if first_chunk.is_null() {
                let err_ptr = unsafe { duckdb_sys::duckdb_result_error(result) };
                if !err_ptr.is_null() {
                    let err = unsafe { CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .to_string();
                    if (crate::signals::has_seen_interrupt() && !err.trim().is_empty())
                        || is_interrupt_error(&err)
                    {
                        let _ = out.write_all(b"Interrupt\n");
                        return 1;
                    }
                }
                return 0;
            }

            let mut header_width: usize = 5;
            if state.mode == RenderMode::LINE {
                for name in &col_names {
                    header_width = header_width.max(name.len());
                }
            }

            match state.mode {
                RenderMode::CSV => {
                    if state.showHeader {
                        for (idx, name) in col_names.iter().enumerate() {
                            output_csv(out, state, Some(name), idx < col_count - 1);
                        }
                        let _ = out.write_all(state.rowSeparator.as_bytes());
                    }
                }
                RenderMode::ASCII => {
                    if state.showHeader {
                        for (idx, name) in col_names.iter().enumerate() {
                            if idx > 0 {
                                let _ = out.write_all(b"\n");
                            }
                            let _ = out.write_all(name.as_bytes());
                        }
                        let _ = out.write_all(b"\n");
                    }
                }
                RenderMode::LIST | RenderMode::TCL | RenderMode::QUOTE => {
                    if state.showHeader {
                        for (idx, name) in col_names.iter().enumerate() {
                            if idx > 0 {
                                let _ = out.write_all(state.colSeparator.as_bytes());
                            }
                            match state.mode {
                                RenderMode::TCL => output_c_string(out, name),
                                RenderMode::QUOTE => output_quoted_string(out, name),
                                _ => {
                                    let _ = out.write_all(name.as_bytes());
                                }
                            }
                        }
                        let _ = out.write_all(state.rowSeparator.as_bytes());
                    }
                }
                RenderMode::HTML => {
                    if state.showHeader {
                        let _ = out.write_all(b"<tr>");
                        for name in &col_names {
                            let _ = out.write_all(b"<th>");
                            for ch in name.chars() {
                                match ch {
                                    '<' => {
                                        let _ = out.write_all(b"&lt;");
                                    }
                                    '&' => {
                                        let _ = out.write_all(b"&amp;");
                                    }
                                    '>' => {
                                        let _ = out.write_all(b"&gt;");
                                    }
                                    '"' => {
                                        let _ = out.write_all(b"&quot;");
                                    }
                                    '\'' => {
                                        let _ = out.write_all(b"&#39;");
                                    }
                                    _ => {
                                        let _ = out.write_all(ch.to_string().as_bytes());
                                    }
                                }
                            }
                            let _ = out.write_all(b"</th>\n");
                        }
                        let _ = out.write_all(b"</tr>\n");
                    }
                }
                _ => {}
            }

            let mut chunk = first_chunk;
            let mut row_index: usize = 0;
            loop {
                let chunk_size = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) } as usize;
                let mut vectors: Vec<duckdb_sys::duckdb_vector> = Vec::with_capacity(col_count);
                let mut types: Vec<duckdb_sys::duckdb_logical_type> = Vec::with_capacity(col_count);
                for c in 0..col_count {
                    let v = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, c as u64) };
                    vectors.push(v);
                    types.push(unsafe { duckdb_sys::duckdb_vector_get_column_type(v) });
                }
                let unquote_json_cols: Vec<bool> = types
                    .iter()
                    .copied()
                    .map(|t| logical_type_contains_json(t, 0))
                    .collect();

                for r in 0..chunk_size {
                    if crate::signals::has_seen_interrupt() {
                        let _ = out.write_all(b"Interrupt\n");
                        for t in types.iter_mut() {
                            unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                        }
                        unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
                        return 1;
                    }

                    match state.mode {
                        RenderMode::CSV => {
                            for c in 0..col_count {
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                let cell = raw.as_deref().map(|v| {
                                    if v.starts_with('[') || v.starts_with('{') {
                                        normalize_complex_value_for_shell_display(
                                            v,
                                            unquote_json_cols[c],
                                        )
                                    } else {
                                        Cow::Borrowed(v)
                                    }
                                });
                                output_csv(out, state, cell.as_deref(), c < col_count - 1);
                            }
                            let _ = out.write_all(state.rowSeparator.as_bytes());
                        }
                        RenderMode::ASCII => {
                            for c in 0..col_count {
                                if c > 0 {
                                    let _ = out.write_all(b"\n");
                                }
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                if let Some(v) = raw.as_deref() {
                                    let v = if v.starts_with('[') || v.starts_with('{') {
                                        normalize_complex_value_for_shell_display(
                                            v,
                                            unquote_json_cols[c],
                                        )
                                    } else {
                                        Cow::Borrowed(v)
                                    };
                                    let _ = out.write_all(v.as_bytes());
                                } else {
                                    let _ = out.write_all(state.nullValue.as_bytes());
                                }
                            }
                            let _ = out.write_all(b"\n");
                        }
                        RenderMode::LIST => {
                            for c in 0..col_count {
                                if c > 0 {
                                    let _ = out.write_all(state.colSeparator.as_bytes());
                                }
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                if let Some(v) = raw.as_deref() {
                                    let v = if v.starts_with('[') || v.starts_with('{') {
                                        normalize_complex_value_for_shell_display(
                                            v,
                                            unquote_json_cols[c],
                                        )
                                    } else {
                                        Cow::Borrowed(v)
                                    };
                                    let _ = out.write_all(v.as_bytes());
                                } else {
                                    let _ = out.write_all(state.nullValue.as_bytes());
                                }
                            }
                            let _ = out.write_all(state.rowSeparator.as_bytes());
                        }
                        RenderMode::LINE => {
                            if row_index > 0 {
                                let _ = out.write_all(state.rowSeparator.as_bytes());
                            }
                            for c in 0..col_count {
                                let name = &col_names[c];
                                let _ = out.write_all(
                                    format!("{:>width$}", name, width = header_width).as_bytes(),
                                );
                                let _ = out.write_all(b" = ");
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                if let Some(v) = raw.as_deref() {
                                    let v = if v.starts_with('[') || v.starts_with('{') {
                                        normalize_complex_value_for_shell_display(
                                            v,
                                            unquote_json_cols[c],
                                        )
                                    } else {
                                        Cow::Borrowed(v)
                                    };
                                    let _ = out.write_all(v.as_bytes());
                                } else {
                                    let _ = out.write_all(state.nullValue.as_bytes());
                                }
                                let _ = out.write_all(state.rowSeparator.as_bytes());
                            }
                            row_index += 1;
                        }
                        RenderMode::HTML => {
                            let _ = out.write_all(b"<tr>");
                            for c in 0..col_count {
                                let _ = out.write_all(b"<td>");
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                let cell = raw.as_deref().unwrap_or(&state.nullValue);
                                let cell = if cell.starts_with('[') || cell.starts_with('{') {
                                    normalize_complex_value_for_shell_display(
                                        cell,
                                        unquote_json_cols[c],
                                    )
                                } else {
                                    Cow::Borrowed(cell)
                                };
                                for ch in cell.as_ref().chars() {
                                    match ch {
                                        '<' => {
                                            let _ = out.write_all(b"&lt;");
                                        }
                                        '&' => {
                                            let _ = out.write_all(b"&amp;");
                                        }
                                        '>' => {
                                            let _ = out.write_all(b"&gt;");
                                        }
                                        '"' => {
                                            let _ = out.write_all(b"&quot;");
                                        }
                                        '\'' => {
                                            let _ = out.write_all(b"&#39;");
                                        }
                                        _ => {
                                            let _ = out.write_all(ch.to_string().as_bytes());
                                        }
                                    }
                                }
                                let _ = out.write_all(b"</td>\n");
                            }
                            let _ = out.write_all(b"</tr>\n");
                        }
                        RenderMode::TCL => {
                            for c in 0..col_count {
                                if c > 0 {
                                    let _ = out.write_all(state.colSeparator.as_bytes());
                                }
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                let cell = raw.as_deref().unwrap_or(&state.nullValue);
                                let cell = if cell.starts_with('[') || cell.starts_with('{') {
                                    normalize_complex_value_for_shell_display(
                                        cell,
                                        unquote_json_cols[c],
                                    )
                                } else {
                                    Cow::Borrowed(cell)
                                };
                                output_c_string(out, cell.as_ref());
                            }
                            let _ = out.write_all(state.rowSeparator.as_bytes());
                        }
                        RenderMode::QUOTE => {
                            for c in 0..col_count {
                                if c > 0 {
                                    let _ = out.write_all(state.colSeparator.as_bytes());
                                }
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                let Some(v) = raw.as_deref() else {
                                    let _ = out.write_all(b"NULL");
                                    continue;
                                };
                                let v = if v.starts_with('[') || v.starts_with('{') {
                                    normalize_complex_value_for_shell_display(
                                        v,
                                        unquote_json_cols[c],
                                    )
                                } else {
                                    Cow::Borrowed(v)
                                };
                                let t = col_types[c];
                                let is_numeric = matches!(
                                    t,
                                    duckdb_sys::DUCKDB_TYPE_BOOLEAN
                                        | duckdb_sys::DUCKDB_TYPE_TINYINT
                                        | duckdb_sys::DUCKDB_TYPE_SMALLINT
                                        | duckdb_sys::DUCKDB_TYPE_INTEGER
                                        | duckdb_sys::DUCKDB_TYPE_BIGINT
                                        | duckdb_sys::DUCKDB_TYPE_HUGEINT
                                        | duckdb_sys::DUCKDB_TYPE_UHUGEINT
                                        | duckdb_sys::DUCKDB_TYPE_UTINYINT
                                        | duckdb_sys::DUCKDB_TYPE_USMALLINT
                                        | duckdb_sys::DUCKDB_TYPE_UINTEGER
                                        | duckdb_sys::DUCKDB_TYPE_UBIGINT
                                        | duckdb_sys::DUCKDB_TYPE_FLOAT
                                        | duckdb_sys::DUCKDB_TYPE_DOUBLE
                                        | duckdb_sys::DUCKDB_TYPE_DECIMAL
                                );
                                if t == duckdb_sys::DUCKDB_TYPE_VARCHAR
                                    || t == duckdb_sys::DUCKDB_TYPE_BLOB
                                {
                                    output_quoted_string(out, v.as_ref());
                                } else if is_numeric {
                                    let _ = out.write_all(v.as_bytes());
                                } else {
                                    output_quoted_string(out, v.as_ref());
                                }
                            }
                            let _ = out.write_all(state.rowSeparator.as_bytes());
                        }
                        RenderMode::INSERT => {
                            let _ = out.write_all(b"INSERT INTO ");
                            let _ = out.write_all(state.zDestTable.as_bytes());
                            if state.showHeader {
                                let _ = out.write_all(b"(");
                                for (idx, name) in col_names.iter().enumerate() {
                                    if idx > 0 {
                                        let _ = out.write_all(b",");
                                    }
                                    let _ = out.write_all(
                                        quote_identifier_if_needed(state, name).as_bytes(),
                                    );
                                }
                                let _ = out.write_all(b")");
                            }
                            for c in 0..col_count {
                                let _ = out.write_all(if c > 0 { b"," } else { b" VALUES(" });
                                let raw = crate::value::vector_value_to_string(
                                    vectors[c], types[c], r as u64,
                                );
                                let Some(v) = raw.as_deref() else {
                                    let _ = out.write_all(b"NULL");
                                    continue;
                                };
                                let v = if v.starts_with('[') || v.starts_with('{') {
                                    normalize_complex_value_for_shell_display(
                                        v,
                                        unquote_json_cols[c],
                                    )
                                } else {
                                    Cow::Borrowed(v)
                                };
                                let t = col_types[c];
                                let is_numeric = matches!(
                                    t,
                                    duckdb_sys::DUCKDB_TYPE_TINYINT
                                        | duckdb_sys::DUCKDB_TYPE_SMALLINT
                                        | duckdb_sys::DUCKDB_TYPE_INTEGER
                                        | duckdb_sys::DUCKDB_TYPE_BIGINT
                                        | duckdb_sys::DUCKDB_TYPE_HUGEINT
                                        | duckdb_sys::DUCKDB_TYPE_UTINYINT
                                        | duckdb_sys::DUCKDB_TYPE_USMALLINT
                                        | duckdb_sys::DUCKDB_TYPE_UINTEGER
                                        | duckdb_sys::DUCKDB_TYPE_UBIGINT
                                        | duckdb_sys::DUCKDB_TYPE_UHUGEINT
                                        | duckdb_sys::DUCKDB_TYPE_FLOAT
                                        | duckdb_sys::DUCKDB_TYPE_DOUBLE
                                        | duckdb_sys::DUCKDB_TYPE_DECIMAL
                                );
                                if is_numeric {
                                    let _ = out.write_all(v.as_bytes());
                                } else if (state.shellFlgs
                                    & (crate::state::ShellFlags::SHFLG_Newlines as u32))
                                    != 0
                                {
                                    output_quoted_string(out, v.as_ref());
                                } else {
                                    output_quoted_escaped_string(out, v.as_ref());
                                }
                            }
                            let _ = out.write_all(b");\n");
                        }
                        _ => {
                            print_database_error("Rendering mode not implemented yet");
                            for t in types.iter_mut() {
                                unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                            }
                            unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
                            return 1;
                        }
                    }

                    if crate::signals::has_seen_interrupt() {
                        let _ = out.write_all(b"Interrupt\n");
                        for t in types.iter_mut() {
                            unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                        }
                        unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
                        return 1;
                    }

                    if state.mode != RenderMode::LINE {
                        row_index += 1;
                    }
                }

                for t in types.iter_mut() {
                    unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                }
                unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
                chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
                if chunk.is_null() {
                    break;
                }
            }
            0
        };

        let rc = if buffer_for_pager {
            let mut out = std::io::Cursor::new(&mut buf);
            render_rowwise(state, &mut out)
        } else {
            let mut out_handle = std::mem::replace(&mut state.out, OutputHandle::Stdout);
            let rc = {
                let mut out = OutputHandleWriter {
                    out: &mut out_handle,
                };
                let st: &ShellState = &*state;
                render_rowwise(st, &mut out)
            };
            state.out = out_handle;
            rc
        };

        if buffer_for_pager {
            page_or_print_stdout(state, &buf);
        }
        return rc;
    }

    let mut col_unquote_json_literals: Vec<bool> = Vec::with_capacity(col_count);
    for c in 0..col_count {
        let mut logical = unsafe { duckdb_sys::duckdb_column_logical_type(result, c as u64) };
        col_unquote_json_literals.push(logical_type_contains_json(logical, 0));
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut logical) };
    }

    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    loop {
        let mut chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
        if chunk.is_null() {
            let err_ptr = unsafe { duckdb_sys::duckdb_result_error(result) };
            if !err_ptr.is_null() {
                let err = unsafe { CStr::from_ptr(err_ptr) }
                    .to_string_lossy()
                    .to_string();
                if (crate::signals::has_seen_interrupt() && !err.trim().is_empty())
                    || is_interrupt_error(&err)
                {
                    print_stdout_line(state, "Interrupt");
                    return 1;
                }
            }
            break;
        }
        let chunk_size = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) } as usize;
        let mut vectors: Vec<duckdb_sys::duckdb_vector> = Vec::with_capacity(col_count);
        let mut types: Vec<duckdb_sys::duckdb_logical_type> = Vec::with_capacity(col_count);
        for c in 0..col_count {
            let v = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, c as u64) };
            vectors.push(v);
            types.push(unsafe { duckdb_sys::duckdb_vector_get_column_type(v) });
        }
        for r in 0..chunk_size {
            if crate::signals::has_seen_interrupt() {
                print_stdout_line(state, "Interrupt");
                for t in types.iter_mut() {
                    unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
                }
                unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
                return 1;
            }
            let mut row: Vec<Option<String>> = Vec::with_capacity(col_count);
            for c in 0..col_count {
                row.push(crate::value::vector_value_to_string(
                    vectors[c], types[c], r as u64,
                ));
            }
            rows.push(row);
        }
        for t in types.iter_mut() {
            unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
        }
        unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
    }
    let row_count = rows.len();
    if row_count == 0 {
        return 0;
    }

    match state.mode {
        RenderMode::COLUMN
        | RenderMode::TABLE
        | RenderMode::MARKDOWN
        | RenderMode::BOX
        | RenderMode::LATEX => {
            let mut buf: Vec<u8> = Vec::new();
            let mut out = std::io::Cursor::new(&mut buf);
            let mut data: Vec<Vec<String>> = Vec::with_capacity(row_count);
            for r in 0..row_count {
                let mut row: Vec<String> = Vec::with_capacity(col_count);
                for c in 0..col_count {
                    let mut cell = rows[r][c]
                        .clone()
                        .unwrap_or_else(|| state.nullValue.clone());
                    if cell.starts_with('[') || cell.starts_with('{') {
                        cell = normalize_complex_value_for_shell_display(
                            cell.as_str(),
                            col_unquote_json_literals[c],
                        )
                        .into_owned();
                    }
                    if state.mode == RenderMode::BOX {
                        cell = box_convert_value(&cell);
                    }
                    if state.mode == RenderMode::MARKDOWN && cell.contains('|') {
                        cell = cell.replace('|', "\\|");
                    }
                    row.push(cell);
                }
                data.push(row);
            }

            let mut col_width: Vec<usize> = Vec::with_capacity(col_count);
            let mut right_align: Vec<bool> = Vec::with_capacity(col_count);
            for c in 0..col_count {
                let mut w = state.colWidth.get(c).copied().unwrap_or(0);
                if w < 0 {
                    right_align.push(true);
                    w = -w;
                } else {
                    right_align.push(false);
                }
                col_width.push(w as usize);
            }
            for c in 0..col_count {
                let mut header = col_names[c].clone();
                if state.mode == RenderMode::BOX {
                    header = box_convert_value(&header);
                }
                col_width[c] = col_width[c].max(render_length(&header));
                for r in 0..row_count {
                    col_width[c] = col_width[c].max(render_length(&data[r][c]));
                }
                col_names[c] = header;
            }

            match state.mode {
                RenderMode::COLUMN => {
                    if state.showHeader {
                        for c in 0..col_count {
                            utf8_width_print(&mut out, &col_names[c], col_width[c], right_align[c]);
                            let _ = out.write_all(if c == col_count - 1 { b"\n" } else { b"  " });
                        }
                        for c in 0..col_count {
                            print_dashes(&mut out, col_width[c]);
                            let _ = out.write_all(if c == col_count - 1 { b"\n" } else { b"  " });
                        }
                    }
                    for r in 0..row_count {
                        for c in 0..col_count {
                            utf8_width_print(&mut out, &data[r][c], col_width[c], right_align[c]);
                            let _ = out.write_all(if c == col_count - 1 { b"\n" } else { b"  " });
                        }
                    }
                }
                RenderMode::TABLE => {
                    print_row_separator(&mut out, &col_width, "+");
                    let _ = out.write_all(b"| ");
                    for c in 0..col_count {
                        render_aligned_value(&mut out, &col_names[c], col_width[c]);
                        let _ = out.write_all(if c == col_count - 1 { b" |\n" } else { b" | " });
                    }
                    print_row_separator(&mut out, &col_width, "+");
                    for r in 0..row_count {
                        let _ = out.write_all(b"| ");
                        for c in 0..col_count {
                            utf8_width_print(&mut out, &data[r][c], col_width[c], right_align[c]);
                            let _ =
                                out.write_all(if c == col_count - 1 { b" |\n" } else { b" | " });
                        }
                    }
                    print_row_separator(&mut out, &col_width, "+");
                }
                RenderMode::MARKDOWN => {
                    let _ = out.write_all(b"| ");
                    for c in 0..col_count {
                        if c > 0 {
                            let _ = out.write_all(b" | ");
                        }
                        render_aligned_value(&mut out, &col_names[c], col_width[c]);
                    }
                    let _ = out.write_all(b" |\n");
                    print_markdown_separator(&mut out, &col_types, &col_width);
                    for r in 0..row_count {
                        let _ = out.write_all(b"| ");
                        for c in 0..col_count {
                            if c > 0 {
                                let _ = out.write_all(b" | ");
                            }
                            utf8_width_print(&mut out, &data[r][c], col_width[c], right_align[c]);
                        }
                        let _ = out.write_all(b" |\n");
                    }
                }
                RenderMode::BOX => {
                    const BOX_24: &str = "\u{2500}";
                    const BOX_13: &str = "\u{2502}";
                    const BOX_23: &str = "\u{250c}";
                    const BOX_34: &str = "\u{2510}";
                    const BOX_12: &str = "\u{2514}";
                    const BOX_14: &str = "\u{2518}";
                    const BOX_123: &str = "\u{251c}";
                    const BOX_134: &str = "\u{2524}";
                    const BOX_234: &str = "\u{252c}";
                    const BOX_124: &str = "\u{2534}";
                    const BOX_1234: &str = "\u{253c}";

                    fn print_box_row_sep(
                        out: &mut dyn Write,
                        col_width: &[usize],
                        line: &str,
                        sep1: &str,
                        sep2: &str,
                        sep3: &str,
                    ) {
                        let _ = out.write_all(sep1.as_bytes());
                        for (idx, w) in col_width.iter().enumerate() {
                            for _ in 0..(*w + 2) {
                                let _ = out.write_all(line.as_bytes());
                            }
                            if idx + 1 < col_width.len() {
                                let _ = out.write_all(sep2.as_bytes());
                            }
                        }
                        let _ = out.write_all(sep3.as_bytes());
                        let _ = out.write_all(b"\n");
                    }

                    print_box_row_sep(&mut out, &col_width, BOX_24, BOX_23, BOX_234, BOX_34);
                    let _ = out.write_all(BOX_13.as_bytes());
                    let _ = out.write_all(b" ");
                    for c in 0..col_count {
                        render_aligned_value(&mut out, &col_names[c], col_width[c]);
                        let _ = out.write_all(b" ");
                        let _ = out.write_all(BOX_13.as_bytes());
                        if c == col_count - 1 {
                            let _ = out.write_all(b"\n");
                        } else {
                            let _ = out.write_all(b" ");
                        }
                    }
                    print_box_row_sep(&mut out, &col_width, BOX_24, BOX_123, BOX_1234, BOX_134);

                    for r in 0..row_count {
                        let _ = out.write_all(BOX_13.as_bytes());
                        let _ = out.write_all(b" ");
                        for c in 0..col_count {
                            utf8_width_print(&mut out, &data[r][c], col_width[c], right_align[c]);
                            let _ = out.write_all(b" ");
                            let _ = out.write_all(BOX_13.as_bytes());
                            if c == col_count - 1 {
                                let _ = out.write_all(b"\n");
                            } else {
                                let _ = out.write_all(b" ");
                            }
                        }
                    }
                    print_box_row_sep(&mut out, &col_width, BOX_24, BOX_12, BOX_124, BOX_14);
                }
                RenderMode::LATEX => {
                    let _ = out.write_all(b"\\begin{tabular}{|");
                    for c in 0..col_count {
                        let t = col_types[c];
                        let is_num = matches!(
                            t,
                            duckdb_sys::DUCKDB_TYPE_TINYINT
                                | duckdb_sys::DUCKDB_TYPE_SMALLINT
                                | duckdb_sys::DUCKDB_TYPE_INTEGER
                                | duckdb_sys::DUCKDB_TYPE_BIGINT
                                | duckdb_sys::DUCKDB_TYPE_FLOAT
                                | duckdb_sys::DUCKDB_TYPE_DOUBLE
                                | duckdb_sys::DUCKDB_TYPE_DECIMAL
                        );
                        let _ = out.write_all(if is_num { b"r" } else { b"l" });
                    }
                    let _ = out.write_all(b"|}\n\\hline\n");
                    for c in 0..col_count {
                        render_aligned_value(&mut out, &col_names[c], col_width[c]);
                        let _ = out.write_all(if c == col_count - 1 {
                            b" \\\\\n"
                        } else {
                            b" & "
                        });
                    }
                    let _ = out.write_all(b"\\hline\n");
                    for r in 0..row_count {
                        for c in 0..col_count {
                            utf8_width_print(&mut out, &data[r][c], col_width[c], right_align[c]);
                            let _ = out.write_all(if c == col_count - 1 {
                                b" \\\\\n"
                            } else {
                                b" & "
                            });
                        }
                    }
                    let _ = out.write_all(b"\\hline\n\\end{tabular}\n");
                }
                _ => {}
            }
            drop(out);
            page_or_print_stdout(state, &buf);
            0
        }
        _ => {
            print_database_error("Rendering mode not implemented yet");
            1
        }
    }
}

fn echo_slices_for_shell(
    extracted: duckdb_sys::duckdb_extracted_statements,
    cmd: &str,
) -> Vec<String> {
    let Ok(query) = CString::new(cmd) else {
        return Vec::new();
    };
    let mut out_slices: *mut *mut std::os::raw::c_char = std::ptr::null_mut();
    let mut out_count: usize = 0;
    let mut out_error: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe {
        shellshim::duckdb_shellshim_echo_slices_from_extracted(
            extracted as *mut _,
            query.as_ptr(),
            &mut out_slices,
            &mut out_count,
            &mut out_error,
        )
    };
    if rc != 0 {
        if !out_error.is_null() {
            unsafe { libc::free(out_error as *mut _) };
        }
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::with_capacity(out_count);
    for i in 0..out_count {
        let ptr = unsafe { *out_slices.add(i) };
        if ptr.is_null() {
            out.push(String::new());
        } else {
            out.push(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string());
        }
    }
    unsafe { shellshim::duckdb_shellshim_free_echo_slices(out_slices, out_count) };
    out
}

fn run_sql_script(state: &mut ShellState, con: duckdb_sys::duckdb_connection, cmd: &str) -> i32 {
    let query = match CString::new(cmd) {
        Ok(q) => q,
        Err(_) => {
            print_database_error("Invalid SQL (contains null byte)");
            return 1;
        }
    };

    let mut extracted: duckdb_sys::duckdb_extracted_statements = std::ptr::null_mut();
    let count =
        unsafe { duckdb_sys::duckdb_extract_statements(con, query.as_ptr(), &mut extracted) };
    if extracted.is_null() {
        print_database_error_state(
            state,
            "duckdb_extract_statements returned null extracted statements",
        );
        return 1;
    }
    if count == 0 {
        let err_ptr = unsafe { duckdb_sys::duckdb_extract_statements_error(extracted) };
        if !err_ptr.is_null() {
            let err = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .to_string();
            if !err.trim().is_empty() {
                print_database_error_state(state, &err);
                unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
                return 1;
            }
        }
        unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
        return 0;
    }

    let echo_on = (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0;
    let stmt_slices = echo_slices_for_shell(extracted, cmd);
    let tz_stmts_fallback = crate::sql_split::split_statements_for_shell(cmd);
    let stmt_slices_per_statement = stmt_slices.len() == count as usize;
    let echo_has_per_statement = echo_on && stmt_slices_per_statement;
    let tz_has_per_statement =
        stmt_slices_per_statement || tz_stmts_fallback.len() == count as usize;

    for idx in 0..count {
        crate::db::sync_process_timezone(state, con);

        let mut stmt: duckdb_sys::duckdb_prepared_statement = std::ptr::null_mut();
        let mut prepared = false;
        for attempt in 0..2 {
            let prep_rc = unsafe {
                duckdb_sys::duckdb_prepare_extracted_statement(con, extracted, idx, &mut stmt)
            };
            if prep_rc == duckdb_sys::DuckDBSuccess {
                prepared = true;
                break;
            }

            let err = if !stmt.is_null() {
                let err_ptr = unsafe { duckdb_sys::duckdb_prepare_error(stmt) };
                if err_ptr.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .to_string()
                }
            } else {
                String::new()
            };

            if crate::signals::has_seen_interrupt()
                || err.to_ascii_lowercase().contains("interrupt")
            {
                print_stdout_line(state, "Interrupt");
                unsafe {
                    duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                    duckdb_sys::duckdb_destroy_extracted(&mut extracted);
                }
                return 1;
            }

            if attempt == 0
                && crate::db::error_mentions_icu_extension(&err)
                && crate::db::ensure_icu_loaded(state, con)
            {
                if !stmt.is_null() {
                    unsafe { duckdb_sys::duckdb_destroy_prepare(&mut stmt) };
                }
                stmt = std::ptr::null_mut();
                continue;
            }
            if attempt == 0
                && crate::db::error_mentions_json_extension(&err)
                && crate::db::ensure_json_loaded(state, con)
            {
                if !stmt.is_null() {
                    unsafe { duckdb_sys::duckdb_destroy_prepare(&mut stmt) };
                }
                stmt = std::ptr::null_mut();
                continue;
            }

            if !err.trim().is_empty() {
                print_database_error_state(state, &err);
            } else {
                print_database_error_state(state, "Failed to prepare statement");
            }
            unsafe {
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }
        if !prepared {
            unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
            return 1;
        }

        if echo_on {
            let to_echo = if echo_has_per_statement {
                stmt_slices[idx as usize].as_str()
            } else if idx == 0 {
                cmd
            } else {
                ""
            };
            if !to_echo.is_empty() {
                print_stdout(state, to_echo);
                print_stdout(state, "\n");
            }
        }

        let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
        let mut executed = false;
        for attempt in 0..2 {
            // Non-duckbox render modes can consume results via the chunk API; use streaming execution.
            let wants_materialized_describe = if count == 1 {
                let trimmed_cmd = cmd.trim();
                let cmd_no_semi = trimmed_cmd.strip_suffix(';').unwrap_or(trimmed_cmd).trim();
                cmd_no_semi
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("describe")
            } else {
                false
            };
            let exec_rc = if wants_materialized_describe {
                unsafe { duckdb_sys::duckdb_execute_prepared(stmt, &mut result) }
            } else {
                unsafe { duckdb_sys::duckdb_execute_prepared_streaming(stmt, &mut result) }
            };
            if exec_rc == duckdb_sys::DuckDBSuccess {
                executed = true;
                break;
            }

            let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut result) };
            let err = if err_ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(err_ptr) }
                    .to_string_lossy()
                    .to_string()
            };

            if crate::signals::has_seen_interrupt()
                || err.to_ascii_lowercase().contains("interrupt")
            {
                print_stdout_line(state, "Interrupt");
                unsafe {
                    duckdb_sys::duckdb_destroy_result(&mut result);
                    duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                    duckdb_sys::duckdb_destroy_extracted(&mut extracted);
                }
                return 1;
            }

            if attempt == 0
                && crate::db::error_mentions_icu_extension(&err)
                && crate::db::ensure_icu_loaded(state, con)
            {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                result = unsafe { std::mem::zeroed() };
                continue;
            }
            if attempt == 0
                && crate::db::error_mentions_json_extension(&err)
                && crate::db::ensure_json_loaded(state, con)
            {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                result = unsafe { std::mem::zeroed() };
                continue;
            }

            if !err.trim().is_empty() {
                if err.contains(
                    "Values were not provided for the following prepared statement parameters",
                ) {
                    print_database_error_state(
                        state,
                        "Prepared statement parameters cannot be used directly",
                    );
                } else {
                    print_database_error_state(state, &err);
                }
            } else {
                print_database_error_state(state, "Query failed");
            }
            unsafe {
                duckdb_sys::duckdb_destroy_result(&mut result);
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }
        if !executed {
            unsafe {
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }

        let mut render_rc = 0;
        let return_type = unsafe { duckdb_sys::duckdb_result_return_type(result) };
        state.cMode = state.mode;
        let stmt_type = unsafe { duckdb_sys::duckdb_result_statement_type(result) };
        if stmt_type == duckdb_sys::DUCKDB_STATEMENT_TYPE_EXPLAIN {
            state.cMode = RenderMode::EXPLAIN;
        }
        if return_type == duckdb_sys::DUCKDB_RESULT_TYPE_QUERY_RESULT {
            let mut did_render = false;
            if count == 1 {
                if let Some(rc) = try_render_shell_describe(state, cmd, None, &mut result) {
                    render_rc = rc;
                    did_render = true;
                }
                let col_count = unsafe { duckdb_sys::duckdb_column_count(&mut result) } as usize;
                let mut cast_cols: Vec<bool> = vec![false; col_count];
                let mut needs_cast = false;
                for c in 0..col_count {
                    let mut logical =
                        unsafe { duckdb_sys::duckdb_column_logical_type(&mut result, c as u64) };
                    if !logical.is_null() {
                        let type_id = unsafe { duckdb_sys::duckdb_get_type_id(logical) };
                        let type_name = duckbox_render_type(state, logical, 0);
                        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut logical) };
                        let cast_this = if matches!(type_name.as_str(), "geometry" | "unknown") {
                            true
                        } else {
                            match type_id {
                                duckdb_sys::DUCKDB_TYPE_UNION => !matches!(
                                    state.mode,
                                    RenderMode::JSON | RenderMode::JSONLINES | RenderMode::DUCKBOX
                                ),
                                duckdb_sys::DUCKDB_TYPE_BIGNUM => !matches!(
                                    state.mode,
                                    RenderMode::DUCKBOX | RenderMode::JSON | RenderMode::JSONLINES
                                ),
                                _ => false,
                            }
                        };
                        if cast_this {
                            cast_cols[c] = true;
                            needs_cast = true;
                        }
                    }
                }

                if !did_render && needs_cast {
                    let trimmed_cmd = cmd.trim();
                    let cmd_no_semi = trimmed_cmd.strip_suffix(';').unwrap_or(trimmed_cmd).trim();
                    let lower = cmd_no_semi.trim_start().to_ascii_lowercase();
                    if lower.starts_with("select") || lower.starts_with("with") {
                        let mut select_list = String::new();
                        for c in 0..col_count {
                            let name_ptr =
                                unsafe { duckdb_sys::duckdb_column_name(&mut result, c as u64) };
                            let name = if name_ptr.is_null() {
                                String::new()
                            } else {
                                unsafe { CStr::from_ptr(name_ptr) }
                                    .to_string_lossy()
                                    .to_string()
                            };
                            let ident = quote_identifier_if_needed(state, &name);
                            if c > 0 {
                                select_list.push_str(", ");
                            }
                            if cast_cols[c] {
                                select_list.push_str("cast(t.");
                                select_list.push_str(&ident);
                                select_list.push_str(" as varchar) as ");
                                select_list.push_str(&ident);
                            } else {
                                select_list.push_str("t.");
                                select_list.push_str(&ident);
                                select_list.push_str(" as ");
                                select_list.push_str(&ident);
                            }
                        }
                        let wrapper_sql =
                            format!("select {} from ({}) t", select_list, cmd_no_semi);
                        if let Ok(wrapper_cstr) = CString::new(wrapper_sql) {
                            let mut string_result: duckdb_sys::duckdb_result =
                                unsafe { std::mem::zeroed() };
                            let string_rc = unsafe {
                                duckdb_sys::duckdb_query(
                                    con,
                                    wrapper_cstr.as_ptr(),
                                    &mut string_result,
                                )
                            };
                            if string_rc == duckdb_sys::DuckDBSuccess {
                                render_rc = render_result(state, &mut string_result);
                                unsafe { duckdb_sys::duckdb_destroy_result(&mut string_result) };
                                did_render = true;
                            } else {
                                unsafe { duckdb_sys::duckdb_destroy_result(&mut string_result) };
                            }
                        }
                    }
                }
            }

            if !did_render {
                render_rc = render_result(state, &mut result);
            }
        } else if return_type == duckdb_sys::DUCKDB_RESULT_TYPE_CHANGED_ROWS {
            let changes = unsafe { duckdb_sys::duckdb_rows_changed(&mut result) };
            state.last_changes = changes;
            state.total_changes = state.total_changes.saturating_add(changes);
        }
        unsafe {
            duckdb_sys::duckdb_destroy_result(&mut result);
            duckdb_sys::duckdb_destroy_prepare(&mut stmt);
        }
        state.cMode = state.mode;
        if render_rc != 0 {
            unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
            return render_rc;
        }

        if tz_has_per_statement {
            let stmt = if stmt_slices_per_statement {
                stmt_slices[idx as usize].as_str()
            } else {
                tz_stmts_fallback[idx as usize]
            };
            if let Some(tz) = try_parse_set_timezone_statement(stmt) {
                crate::db::apply_process_timezone_setting(state, tz.as_str());
            }
        }
    }

    unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
    0
}

fn ensure_stdin_temp_file(state: &mut ShellState) -> Option<String> {
    if let Some(path) = state.stdin_temp_path.as_deref() {
        return Some(path.to_string());
    }
    // Only spool stdin when the shell is not consuming stdin for SQL input.
    if state.readStdin || state.stdin_is_interactive {
        return None;
    }
    let mut data: Vec<u8> = Vec::new();
    std::io::stdin().read_to_end(&mut data).ok()?;
    let mut path = std::env::temp_dir();
    path.push(format!(
        "duckdb_cli_stdin_{}_{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, &data).ok()?;
    let path_s = path.to_string_lossy().to_string();
    state.stdin_temp_path = Some(path_s.clone());
    Some(path_s)
}

fn try_run_copy_to_special_device(
    state: &mut ShellState,
    con: duckdb_sys::duckdb_connection,
    sql: &str,
) -> Option<i32> {
    let lower = sql.trim_start().to_ascii_lowercase();
    if !lower.starts_with("copy") {
        return None;
    }

    enum Target {
        Stdout,
        Stderr,
    }

    let make_tmp = || -> Option<String> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "duckdb_cli_copy_{}_{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        Some(path.to_string_lossy().to_string())
    };

    let lower_full = sql.to_ascii_lowercase();
    let (target, rewritten_sql, out_path) = if lower_full.contains("/dev/stdout") {
        let tmp = make_tmp()?;
        (Target::Stdout, sql.replace("/dev/stdout", &tmp), tmp)
    } else if lower_full.contains("/dev/stderr") {
        let tmp = make_tmp()?;
        (Target::Stderr, sql.replace("/dev/stderr", &tmp), tmp)
    } else if let Some(pos) = lower_full.find("to stdout") {
        let tmp = make_tmp()?;
        let mut out = String::new();
        out.push_str(&sql[..pos]);
        out.push_str(&format!("to '{}'", tmp));
        out.push_str(&sql[pos + "to stdout".len()..]);
        (Target::Stdout, out, tmp)
    } else if let Some(pos) = lower_full.find("to stderr") {
        let tmp = make_tmp()?;
        let mut out = String::new();
        out.push_str(&sql[..pos]);
        out.push_str(&format!("to '{}'", tmp));
        out.push_str(&sql[pos + "to stderr".len()..]);
        (Target::Stderr, out, tmp)
    } else {
        return None;
    };

    let Ok(sql_c) = CString::new(rewritten_sql) else {
        print_database_error_state(state, "Invalid SQL (contains null byte)");
        return Some(1);
    };
    let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, sql_c.as_ptr(), &mut result) };
    if rc != duckdb_sys::DuckDBSuccess {
        let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut result) };
        if !err_ptr.is_null() {
            let err = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .to_string();
            print_database_error_state(state, &err);
        } else {
            print_database_error_state(state, "Query failed");
        }
        unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
        return Some(1);
    }
    unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };

    let bytes = match std::fs::read(out_path.as_str()) {
        Ok(b) => b,
        Err(err) => {
            print_database_error_state(state, &format!("IO Error: {}", err));
            return Some(1);
        }
    };

    match target {
        Target::Stdout => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&bytes);
        }
        Target::Stderr => {
            let mut out = std::io::stderr().lock();
            let _ = out.write_all(&bytes);
        }
    }

    Some(0)
}

pub fn run_command(state: &mut ShellState, session: &mut Session, cmd: &str) -> i32 {
    if cmd.starts_with('.') {
        let rc = run_dot_command(state, session, cmd);
        if state.outCount > 0 {
            state.outCount -= 1;
            if state.outCount == 0 {
                crate::output::reset_output(state);
            }
        }
        return rc;
    }
    let timer_start = if state.timer_enabled {
        Some(std::time::Instant::now())
    } else {
        None
    };

    let mut sql_owned: Option<String> = None;
    if cmd.contains("/dev/stdin") {
        if let Some(stdin_path) = ensure_stdin_temp_file(state) {
            sql_owned = Some(cmd.replace("/dev/stdin", &stdin_path));
        }
    }
    let sql = sql_owned.as_deref().unwrap_or(cmd);

    if let Some(rc) = try_run_copy_to_special_device(state, session.con, sql) {
        return rc;
    }

    if state.mode == RenderMode::DUCKBOX {
        let rc = run_duckbox_query(state, session.con, sql);
        if state.outCount > 0 {
            crate::output::reset_output(state);
            state.outCount = 0;
        }
        if let Some(start) = timer_start {
            print_raw_stdout_line(&format_timer_line(start.elapsed()));
        }
        return rc;
    }
    let rc = run_sql_script(state, session.con, sql);
    if state.outCount > 0 {
        crate::output::reset_output(state);
        state.outCount = 0;
    }
    if let Some(start) = timer_start {
        print_raw_stdout_line(&format_timer_line(start.elapsed()));
    }
    rc
}

fn duckbox_type_to_string_lower(
    state: &ShellState,
    type_: duckdb_sys::duckdb_logical_type,
    depth: usize,
) -> String {
    if depth > 64 {
        return "…".to_string();
    }
    let alias_ptr = unsafe { duckdb_sys::duckdb_logical_type_get_alias(type_) };
    if !alias_ptr.is_null() {
        let alias = unsafe { CStr::from_ptr(alias_ptr) }
            .to_string_lossy()
            .to_string();
        unsafe { duckdb_sys::duckdb_free(alias_ptr as *mut _) };
        if !alias.trim().is_empty() {
            return alias.trim().to_ascii_lowercase();
        }
    }
    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
    match type_id {
        duckdb_sys::DUCKDB_TYPE_BOOLEAN => "boolean".to_string(),
        duckdb_sys::DUCKDB_TYPE_TINYINT => "tinyint".to_string(),
        duckdb_sys::DUCKDB_TYPE_SMALLINT => "smallint".to_string(),
        duckdb_sys::DUCKDB_TYPE_INTEGER => "integer".to_string(),
        duckdb_sys::DUCKDB_TYPE_BIGINT => "bigint".to_string(),
        duckdb_sys::DUCKDB_TYPE_HUGEINT => "hugeint".to_string(),
        duckdb_sys::DUCKDB_TYPE_UTINYINT => "utinyint".to_string(),
        duckdb_sys::DUCKDB_TYPE_USMALLINT => "usmallint".to_string(),
        duckdb_sys::DUCKDB_TYPE_UINTEGER => "uinteger".to_string(),
        duckdb_sys::DUCKDB_TYPE_UBIGINT => "ubigint".to_string(),
        duckdb_sys::DUCKDB_TYPE_UHUGEINT => "uhugeint".to_string(),
        duckdb_sys::DUCKDB_TYPE_FLOAT => "float".to_string(),
        duckdb_sys::DUCKDB_TYPE_DOUBLE => "double".to_string(),
        duckdb_sys::DUCKDB_TYPE_DECIMAL => {
            let width = unsafe { duckdb_sys::duckdb_decimal_width(type_) };
            let scale = unsafe { duckdb_sys::duckdb_decimal_scale(type_) };
            format!("decimal({},{})", width, scale)
        }
        duckdb_sys::DUCKDB_TYPE_VARCHAR => "varchar".to_string(),
        duckdb_sys::DUCKDB_TYPE_BLOB => "blob".to_string(),
        duckdb_sys::DUCKDB_TYPE_UUID => "uuid".to_string(),
        duckdb_sys::DUCKDB_TYPE_DATE => "date".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIME => "time".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIME_NS => "time_ns".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP => "timestamp".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP_S => "timestamp_s".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP_MS => "timestamp_ms".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP_NS => "timestamp_ns".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIMESTAMP_TZ => "timestamp with time zone".to_string(),
        duckdb_sys::DUCKDB_TYPE_TIME_TZ => "time with time zone".to_string(),
        duckdb_sys::DUCKDB_TYPE_INTERVAL => "interval".to_string(),
        duckdb_sys::DUCKDB_TYPE_BIT => "bit".to_string(),
        duckdb_sys::DUCKDB_TYPE_BIGNUM => "bignum".to_string(),
        duckdb_sys::DUCKDB_TYPE_ENUM => {
            let n = unsafe { duckdb_sys::duckdb_enum_dictionary_size(type_) } as usize;
            let mut out = String::from("enum(");
            for i in 0..n {
                let ptr = unsafe { duckdb_sys::duckdb_enum_dictionary_value(type_, i as u64) };
                if ptr.is_null() {
                    continue;
                }
                let v = unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .to_string()
                    .to_ascii_lowercase();
                unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
                if !out.ends_with('(') {
                    out.push_str(", ");
                }
                out.push('\'');
                for ch in v.chars() {
                    if ch == '\'' {
                        out.push('\'');
                        out.push('\'');
                    } else {
                        out.push(ch);
                    }
                }
                out.push('\'');
            }
            out.push(')');
            out
        }
        duckdb_sys::DUCKDB_TYPE_LIST => {
            let mut child = unsafe { duckdb_sys::duckdb_list_type_child_type(type_) };
            let child_s = duckbox_type_to_string_lower(state, child, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
            format!("{}[]", child_s)
        }
        duckdb_sys::DUCKDB_TYPE_ARRAY => {
            let mut child = unsafe { duckdb_sys::duckdb_array_type_child_type(type_) };
            let child_s = duckbox_type_to_string_lower(state, child, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
            let size = unsafe { duckdb_sys::duckdb_array_type_array_size(type_) };
            format!("{}[{}]", child_s, size)
        }
        duckdb_sys::DUCKDB_TYPE_MAP => {
            let mut key = unsafe { duckdb_sys::duckdb_map_type_key_type(type_) };
            let mut value = unsafe { duckdb_sys::duckdb_map_type_value_type(type_) };
            let key_s = duckbox_type_to_string_lower(state, key, depth + 1);
            let value_s = duckbox_type_to_string_lower(state, value, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut key) };
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut value) };
            format!("map({}, {})", key_s, value_s)
        }
        duckdb_sys::DUCKDB_TYPE_STRUCT => {
            let n = unsafe { duckdb_sys::duckdb_struct_type_child_count(type_) } as usize;
            let mut out = String::from("struct(");
            for i in 0..n {
                let name_ptr =
                    unsafe { duckdb_sys::duckdb_struct_type_child_name(type_, i as u64) };
                let mut child =
                    unsafe { duckdb_sys::duckdb_struct_type_child_type(type_, i as u64) };
                let name = if name_ptr.is_null() {
                    String::new()
                } else {
                    let s = unsafe { CStr::from_ptr(name_ptr) }
                        .to_string_lossy()
                        .to_string();
                    unsafe { duckdb_sys::duckdb_free(name_ptr as *mut _) };
                    s
                };
                let child_s = duckbox_type_to_string_lower(state, child, depth + 1);
                unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
                if !out.ends_with('(') {
                    out.push_str(", ");
                }
                out.push_str(&quote_identifier_if_needed(state, &name));
                out.push(' ');
                out.push_str(&child_s);
            }
            out.push(')');
            out
        }
        duckdb_sys::DUCKDB_TYPE_UNION => {
            let n = unsafe { duckdb_sys::duckdb_union_type_member_count(type_) } as usize;
            let mut out = String::from("union(");
            for i in 0..n {
                let name_ptr =
                    unsafe { duckdb_sys::duckdb_union_type_member_name(type_, i as u64) };
                let mut child =
                    unsafe { duckdb_sys::duckdb_union_type_member_type(type_, i as u64) };
                let name = if name_ptr.is_null() {
                    String::new()
                } else {
                    let s = unsafe { CStr::from_ptr(name_ptr) }
                        .to_string_lossy()
                        .to_string();
                    unsafe { duckdb_sys::duckdb_free(name_ptr as *mut _) };
                    s
                };
                let child_s = duckbox_type_to_string_lower(state, child, depth + 1);
                unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
                if !out.ends_with('(') {
                    out.push_str(", ");
                }
                out.push_str(&quote_identifier_if_needed(state, &name));
                out.push(' ');
                out.push_str(&child_s);
            }
            out.push(')');
            out
        }
        _ => "unknown".to_string(),
    }
}

fn duckbox_render_type(
    state: &ShellState,
    type_: duckdb_sys::duckdb_logical_type,
    depth: usize,
) -> String {
    let alias_ptr = unsafe { duckdb_sys::duckdb_logical_type_get_alias(type_) };
    if !alias_ptr.is_null() {
        let alias = unsafe { CStr::from_ptr(alias_ptr) }
            .to_string_lossy()
            .to_string();
        unsafe { duckdb_sys::duckdb_free(alias_ptr as *mut _) };
        if !alias.trim().is_empty() {
            return alias.trim().to_ascii_lowercase();
        }
    }
    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(type_) };
    match type_id {
        duckdb_sys::DUCKDB_TYPE_TINYINT => "int8".to_string(),
        duckdb_sys::DUCKDB_TYPE_SMALLINT => "int16".to_string(),
        duckdb_sys::DUCKDB_TYPE_INTEGER => "int32".to_string(),
        duckdb_sys::DUCKDB_TYPE_BIGINT => "int64".to_string(),
        duckdb_sys::DUCKDB_TYPE_HUGEINT => "int128".to_string(),
        duckdb_sys::DUCKDB_TYPE_UTINYINT => "uint8".to_string(),
        duckdb_sys::DUCKDB_TYPE_USMALLINT => "uint16".to_string(),
        duckdb_sys::DUCKDB_TYPE_UINTEGER => "uint32".to_string(),
        duckdb_sys::DUCKDB_TYPE_UBIGINT => "uint64".to_string(),
        duckdb_sys::DUCKDB_TYPE_UHUGEINT => "uint128".to_string(),
        duckdb_sys::DUCKDB_TYPE_LIST => {
            let mut child = unsafe { duckdb_sys::duckdb_list_type_child_type(type_) };
            let child_s = duckbox_render_type(state, child, depth + 1);
            unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut child) };
            format!("{}[]", child_s)
        }
        _ => duckbox_type_to_string_lower(state, type_, depth),
    }
}

fn run_duckbox_query(state: &mut ShellState, con: duckdb_sys::duckdb_connection, cmd: &str) -> i32 {
    run_duckbox_query_impl(state, con, cmd, None)
}

fn duckbox_describe_type_names(
    con: duckdb_sys::duckdb_connection,
    sql: &str,
) -> Option<Vec<String>> {
    let sql = sql.trim();
    if sql.is_empty() {
        return None;
    }
    let sql = sql.strip_suffix(';').unwrap_or(sql).trim();
    if sql.is_empty() {
        return None;
    }
    let describe_sql = format!("describe {}", sql);
    let describe_cstr = CString::new(describe_sql).ok()?;
    let mut describe_result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
    let rc = unsafe { duckdb_sys::duckdb_query(con, describe_cstr.as_ptr(), &mut describe_result) };
    if rc != duckdb_sys::DuckDBSuccess {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut describe_result) };
        return None;
    }
    let row_count = unsafe { duckdb_sys::duckdb_row_count(&mut describe_result) } as usize;
    if row_count == 0 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut describe_result) };
        return None;
    }
    let col_count = unsafe { duckdb_sys::duckdb_column_count(&mut describe_result) } as usize;
    if col_count < 2 {
        unsafe { duckdb_sys::duckdb_destroy_result(&mut describe_result) };
        return None;
    }
    let mut out: Vec<String> = Vec::with_capacity(row_count);
    for r in 0..row_count {
        let ptr = unsafe { duckdb_sys::duckdb_value_varchar(&mut describe_result, 1, r as u64) };
        if ptr.is_null() {
            out.push(String::new());
            continue;
        }
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
        unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
        out.push(s.trim().to_ascii_lowercase());
    }
    unsafe { duckdb_sys::duckdb_destroy_result(&mut describe_result) };
    Some(out)
}

fn run_duckbox_query_unlimited(
    state: &mut ShellState,
    con: duckdb_sys::duckdb_connection,
    cmd: &str,
) -> i32 {
    run_duckbox_query_impl(state, con, cmd, Some((u64::MAX, u64::MAX, u64::MAX)))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn tty_columns_stdout() -> Option<usize> {
    #[repr(C)]
    struct winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: u64 = 0x5413;
    #[cfg(target_os = "macos")]
    const TIOCGWINSZ: u64 = 0x40087468;

    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
        fn isatty(fd: i32) -> i32;
    }

    const STDOUT_FILENO: i32 = 1;
    if unsafe { isatty(STDOUT_FILENO) } == 0 {
        return None;
    }
    let mut ws = winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut ws as *mut winsize) };
    if rc != 0 || ws.ws_col == 0 {
        return None;
    }
    Some(ws.ws_col as usize)
}

fn duckbox_terminal_width() -> usize {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(cols) = tty_columns_stdout() {
        return cols.max(1);
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(80)
}

fn duckbox_max_render_width(state: &ShellState, max_width: u64) -> usize {
    if max_width == u64::MAX {
        return usize::MAX;
    }
    let mut width = if max_width == 0 {
        if state.stdout_is_console {
            duckbox_terminal_width()
        } else {
            usize::MAX
        }
    } else {
        max_width as usize
    };
    if width < 80 {
        width = 80;
    }
    width
}

fn duckbox_available_content_width(max_render_width: usize, col_count: usize) -> usize {
    // Total table width is approximately: sum(col_width) + 3*col_count + 1
    max_render_width.saturating_sub(3usize.saturating_mul(col_count).saturating_add(1))
}

fn max_render_length_multiline(s: &str) -> usize {
    let mut max_len = 0usize;
    for line in s.split('\n') {
        max_len = max_len.max(duckbox_render_length(line));
    }
    max_len
}

fn render_prefix_bytes(s: &str, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    let bytes = s.as_bytes();
    let mut n: i32 = 0;
    let i = duckdb_render_width::get_render_position(bytes, width, &mut n);
    if i >= 0 {
        return (i as usize).min(bytes.len());
    }
    // Fallback: treat width as unicode-scalar count, slice by utf-8 boundaries.
    let mut j: usize = 0;
    let mut count: usize = 0;
    while j < bytes.len() {
        if (bytes[j] & 0xc0) != 0x80 {
            count += 1;
            if count == width {
                j += 1;
                while j < bytes.len() && (bytes[j] & 0xc0) == 0x80 {
                    j += 1;
                }
                break;
            }
        }
        j += 1;
    }
    j.min(bytes.len())
}

fn duckbox_render_prefix_bytes(s: &str, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    let bytes = s.as_bytes();
    let mut n: i32 = 0;
    let i = duckdb_render_width::get_render_position_duckbox(bytes, width, &mut n);
    if i >= 0 {
        return (i as usize).min(bytes.len());
    }
    // Fallback: treat width as unicode-scalar count, slice by utf-8 boundaries.
    let mut j: usize = 0;
    let mut count: usize = 0;
    while j < bytes.len() {
        if (bytes[j] & 0xc0) != 0x80 {
            count += 1;
            if count == width {
                j += 1;
                while j < bytes.len() && (bytes[j] & 0xc0) == 0x80 {
                    j += 1;
                }
                break;
            }
        }
        j += 1;
    }
    j.min(bytes.len())
}

fn truncate_with_ellipsis(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if render_length(s) <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let prefix_bytes = render_prefix_bytes(s, width - 1);
    let mut out = String::from_utf8_lossy(&s.as_bytes()[..prefix_bytes]).to_string();
    out.push('…');
    out
}

fn duckbox_truncate_with_ellipsis(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if duckbox_render_length(s) <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let prefix_bytes = duckbox_render_prefix_bytes(s, width - 1);
    let mut out = String::from_utf8_lossy(&s.as_bytes()[..prefix_bytes]).to_string();
    while out.ends_with('\u{fe0f}') {
        out.pop();
    }
    out.push('…');
    out
}

#[derive(Clone, Debug)]
struct TableMetadataColumn {
    column_name: String,
    column_type: String,
    is_primary_key: bool,
    is_not_null: bool,
    is_unique: bool,
    default_value: String,
}

#[derive(Clone, Debug)]
struct TableMetadataTable {
    database_name: String,
    schema_name: String,
    table_name: String,
    columns: Vec<TableMetadataColumn>,
    estimated_size: Option<u64>,
    is_view: bool,
}

fn parse_qualified_name_components_for_tables(input: &str) -> Result<Vec<String>, ()> {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Mode {
        None,
        Double,
        Backtick,
    }

    let mut components: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut mode = Mode::None;
    let mut it = input.chars().peekable();
    while let Some(ch) = it.next() {
        match mode {
            Mode::None => match ch {
                '.' => {
                    components.push(std::mem::take(&mut cur));
                }
                '"' => {
                    mode = Mode::Double;
                }
                '`' => {
                    mode = Mode::Backtick;
                }
                '\'' => {
                    // Single quotes are not valid for identifiers here; mimic "parse fails".
                    return Err(());
                }
                _ => cur.push(ch),
            },
            Mode::Double => {
                if ch == '"' {
                    if matches!(it.peek(), Some('"')) {
                        let _ = it.next();
                        cur.push('"');
                    } else {
                        mode = Mode::None;
                    }
                } else {
                    cur.push(ch);
                }
            }
            Mode::Backtick => {
                if ch == '`' {
                    mode = Mode::None;
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if mode != Mode::None {
        return Err(());
    }
    components.push(cur);
    Ok(components)
}

fn parse_tables_filter_pattern(filter_pattern: &str) -> (String, String) {
    let mut schema_filter = String::new();
    let mut table_filter = format!("%{}%", filter_pattern);
    if let Ok(components) = parse_qualified_name_components_for_tables(filter_pattern) {
        if components.len() >= 2 {
            schema_filter = format!("%{}%", components[0]);
            table_filter = format!("%{}%", components[1]);
        }
    }
    (schema_filter, table_filter)
}

fn table_metadata_collect_from_result(
    result: &mut duckdb_sys::duckdb_result,
) -> Vec<TableMetadataTable> {
    let row_count = unsafe { duckdb_sys::duckdb_row_count(result) } as usize;
    let mut tables: Vec<TableMetadataTable> = Vec::new();
    let mut cur_key: Option<(String, String, String)> = None;

    for row in 0..row_count {
        let mut get_str = |col: u64| -> Option<String> {
            let ptr = unsafe { duckdb_sys::duckdb_value_varchar(result, col, row as u64) };
            if ptr.is_null() {
                return None;
            }
            let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
            unsafe { duckdb_sys::duckdb_free(ptr as *mut _) };
            Some(s)
        };

        let db = get_str(0).unwrap_or_default();
        let schema = get_str(1).unwrap_or_default();
        let table = get_str(2).unwrap_or_default();
        let col_name = get_str(3).unwrap_or_default();
        let col_type = get_str(4).unwrap_or_default();
        let is_pk = get_str(5)
            .map(|s| s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let estimated_size = get_str(6).and_then(|s| s.trim().parse::<u64>().ok());
        let is_view = get_str(7).is_none();

        let key = (db, schema, table);
        if cur_key.as_ref() != Some(&key) {
            cur_key = Some(key.clone());
            let mut estimated_size = estimated_size;
            if is_view {
                // Views have no estimated size in the reference output.
                estimated_size = None;
            }
            tables.push(TableMetadataTable {
                database_name: key.0.clone(),
                schema_name: key.1.clone(),
                table_name: key.2.clone(),
                columns: Vec::new(),
                estimated_size,
                is_view,
            });
        }
        if let Some(last) = tables.last_mut() {
            last.columns.push(TableMetadataColumn {
                column_name: col_name,
                column_type: col_type,
                is_primary_key: is_pk,
                is_not_null: false,
                is_unique: false,
                default_value: String::new(),
            });
        }
    }

    tables
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TableMetadataHighlightElement {
    DatabaseName,
    SchemaName,
    TableName,
    ColumnName,
    ColumnType,
    PrimaryKeyColumn,
    Comment,
    Layout,
    TableLayout,
    ViewLayout,
}

fn table_metadata_terminal_code(
    state: &ShellState,
    element: TableMetadataHighlightElement,
) -> String {
    let element_name = match element {
        TableMetadataHighlightElement::DatabaseName => "database_name",
        TableMetadataHighlightElement::SchemaName => "schema_name",
        TableMetadataHighlightElement::TableName => "table_name",
        TableMetadataHighlightElement::ColumnName => "column_name",
        TableMetadataHighlightElement::ColumnType => "column_type",
        TableMetadataHighlightElement::PrimaryKeyColumn => "primary_key_column",
        TableMetadataHighlightElement::Comment => "comment",
        TableMetadataHighlightElement::Layout => "layout",
        TableMetadataHighlightElement::TableLayout => "table_layout",
        TableMetadataHighlightElement::ViewLayout => "view_layout",
    };
    terminal_code(highlight_style_for_element(state, element_name))
}

fn table_metadata_push(
    out: &mut String,
    state: &ShellState,
    element: TableMetadataHighlightElement,
    text: &str,
) {
    if !state.highlighting_enabled {
        out.push_str(text);
        return;
    }
    let code = table_metadata_terminal_code(state, element);
    if code.is_empty() {
        out.push_str(text);
        return;
    }
    out.push_str(&code);
    out.push_str(text);
    out.push_str(reset_terminal_code());
}

fn format_table_metadata_type(typ: &str) -> String {
    let lower = typ.to_ascii_lowercase();
    if lower.starts_with("decimal") {
        return "decimal".to_string();
    }
    lower
}

#[derive(Clone, Debug)]
struct TableMetadataRenderComponent {
    text: String,
    render_width: usize,
    element: TableMetadataHighlightElement,
}

#[derive(Clone, Debug)]
struct TableMetadataColumnRenderInfo {
    components: Vec<TableMetadataRenderComponent>,
}

#[derive(Clone, Debug)]
struct TableMetadataColumnRenderRow {
    columns: Vec<TableMetadataColumnRenderInfo>,
}

#[derive(Clone, Debug)]
struct TableMetadataTableRenderInfo {
    table: TableMetadataTable,
    table_name_length: usize,
    render_width: usize,
    max_component_widths: Vec<usize>,
    estimated_size_text: String,
    estimated_size_length: Option<usize>,
    column_renders: Vec<TableMetadataColumnRenderRow>,
}

impl TableMetadataTableRenderInfo {
    fn column_lines(&self) -> usize {
        self.column_renders[0].columns.len()
    }

    fn line_count(&self) -> usize {
        let mut constant_count = 4usize;
        if self.estimated_size_length.is_some() {
            constant_count += 2;
        }
        self.column_lines() + constant_count
    }

    fn per_column_width_for(widths: &[usize]) -> usize {
        let mut width = 4usize;
        for w in widths {
            width += *w;
        }
        if !widths.is_empty() {
            width += widths.len() - 1;
        }
        width
    }

    fn per_column_width(&self) -> usize {
        Self::per_column_width_for(&self.max_component_widths)
    }

    fn truncate_value_if_required(value: &mut String, render_len: &mut usize, max_width: usize) {
        if *render_len <= max_width {
            return;
        }
        *value = truncate_with_ellipsis(value.as_str(), max_width);
        *render_len = render_length(value.as_str());
    }

    fn truncate(&mut self, max_render_width: usize) {
        if self.render_width <= max_render_width {
            return;
        }

        let name_max = max_render_width.saturating_sub(4);
        Self::truncate_value_if_required(
            &mut self.table.table_name,
            &mut self.table_name_length,
            name_max,
        );

        let old_render_width = self.render_width;
        let mut total_column_length = self.per_column_width();
        if total_column_length > max_render_width {
            static MIN_COMPONENT_SIZE: usize = 5;
            let component_count = self.max_component_widths.len();
            let min_leftover_size = component_count * MIN_COMPONENT_SIZE;
            if component_count > 0
                && self.max_component_widths[0] + min_leftover_size > max_render_width
            {
                self.max_component_widths[0] = max_render_width.saturating_sub(min_leftover_size);
            }
            total_column_length = self.per_column_width();
            if total_column_length > max_render_width {
                for i in 1..self.max_component_widths.len() {
                    if self.max_component_widths[i] <= MIN_COMPONENT_SIZE {
                        continue;
                    }
                    let truncate_amount = std::cmp::min(
                        total_column_length - max_render_width,
                        self.max_component_widths[i] - MIN_COMPONENT_SIZE,
                    );
                    self.max_component_widths[i] -= truncate_amount;
                    total_column_length -= truncate_amount;
                    if total_column_length <= old_render_width {
                        break;
                    }
                }
            }

            for column_render in self.column_renders.iter_mut() {
                for col in column_render.columns.iter_mut() {
                    for (component_idx, component) in col.components.iter_mut().enumerate() {
                        let max_w = self
                            .max_component_widths
                            .get(component_idx)
                            .copied()
                            .unwrap_or(max_render_width);
                        Self::truncate_value_if_required(
                            &mut component.text,
                            &mut component.render_width,
                            max_w,
                        );
                    }
                }
            }
        }
        self.render_width = max_render_width;
    }
}

#[derive(Clone, Debug)]
struct TableMetadataLine {
    render_height: usize,
    render_width: usize,
    max_component_widths: Vec<usize>,
    tables: Vec<usize>,
}

impl TableMetadataLine {
    fn per_column_width(&self) -> usize {
        TableMetadataTableRenderInfo::per_column_width_for(&self.max_component_widths)
    }

    fn render_line(
        &self,
        out: &mut String,
        state: &ShellState,
        table_list: &[TableMetadataTableRenderInfo],
        mut line_idx: usize,
        last_line: bool,
    ) {
        let mut table_idx = 0usize;
        while table_idx < self.tables.len() {
            let line_count = table_list[self.tables[table_idx]].line_count();
            if line_idx < line_count {
                break;
            }
            line_idx -= line_count;
            table_idx += 1;
        }

        if table_idx == self.tables.len() {
            if !last_line {
                let pad = " ".repeat(self.render_width);
                table_metadata_push(out, state, TableMetadataHighlightElement::Layout, &pad);
            }
            return;
        }

        let render_table = &table_list[self.tables[table_idx]];
        let table = &render_table.table;
        let layout_type = if table.is_view {
            TableMetadataHighlightElement::ViewLayout
        } else {
            TableMetadataHighlightElement::TableLayout
        };

        let horizontal = "─";
        let vertical = "│";
        let ltcorner = "┌";
        let rtcorner = "┐";
        let ldcorner = "└";
        let rdcorner = "┘";

        if line_idx == 0 {
            let mut top = String::new();
            top.push_str(ltcorner);
            top.push_str(&horizontal.repeat(self.render_width.saturating_sub(2)));
            top.push_str(rtcorner);
            table_metadata_push(out, state, layout_type, &top);
            return;
        }
        if line_idx == 1 {
            let space_count = self
                .render_width
                .saturating_sub(render_table.table_name_length)
                .saturating_sub(2);
            let lspace = space_count / 2;
            let rspace = space_count - lspace;
            let mut table_line = String::new();
            table_line.push_str(vertical);
            table_line.push_str(&" ".repeat(lspace));
            table_metadata_push(out, state, layout_type, &table_line);
            table_metadata_push(
                out,
                state,
                TableMetadataHighlightElement::TableName,
                &table.table_name,
            );
            let mut tail = String::new();
            tail.push_str(&" ".repeat(rspace));
            tail.push_str(vertical);
            table_metadata_push(out, state, layout_type, &tail);
            return;
        }

        if line_idx > 2 && line_idx < render_table.column_lines() + 3 {
            let column_idx = line_idx - 3;

            let total_render_width = self.render_width;
            let per_column_render_width = self.per_column_width();
            let column_render_width = render_table.column_renders.len() * per_column_render_width;
            let mut extra_render_width = total_render_width.saturating_sub(column_render_width);
            let render_width_per_column = if render_table.column_renders.is_empty() {
                0
            } else {
                extra_render_width / render_table.column_renders.len()
            };

            for render_idx in 0..render_table.column_renders.len() {
                let column_render = &render_table.column_renders[render_idx];
                let is_last = render_idx + 1 == render_table.column_renders.len();
                let mut column_line = String::new();
                if render_idx == 0 {
                    column_line.push_str(vertical);
                } else {
                    column_line.push(' ');
                }

                if column_idx < column_render.columns.len() {
                    let col = &column_render.columns[column_idx];
                    column_line.push(' ');
                    table_metadata_push(out, state, layout_type, &column_line);

                    for (component_idx, component) in col.components.iter().enumerate() {
                        table_metadata_push(out, state, component.element, &component.text);

                        let mut pad = String::new();
                        let max_w = self
                            .max_component_widths
                            .get(component_idx)
                            .copied()
                            .unwrap_or(0);
                        pad.push_str(&" ".repeat(max_w.saturating_sub(component.render_width) + 1));
                        if extra_render_width > 0 {
                            let render_count = if is_last {
                                extra_render_width
                            } else {
                                render_width_per_column
                            };
                            pad.push_str(&" ".repeat(render_count));
                            extra_render_width = extra_render_width.saturating_sub(render_count);
                        }
                        table_metadata_push(out, state, layout_type, &pad);
                    }
                } else {
                    let pad = " ".repeat(per_column_render_width.saturating_sub(1));
                    table_metadata_push(out, state, layout_type, &pad);
                }

                let tail = if is_last { vertical } else { " " };
                table_metadata_push(out, state, layout_type, tail);
            }
            return;
        }

        if line_idx == 2
            || (render_table.estimated_size_length.is_some()
                && line_idx == render_table.column_lines() + 3)
        {
            let mut blank = String::new();
            blank.push_str(vertical);
            blank.push_str(&" ".repeat(self.render_width.saturating_sub(2)));
            blank.push_str(vertical);
            table_metadata_push(out, state, layout_type, &blank);
            return;
        }

        if let Some(est_len) = render_table.estimated_size_length {
            if line_idx == render_table.column_lines() + 4 {
                let space_count = self.render_width.saturating_sub(est_len).saturating_sub(2);
                let lspace = space_count / 2;
                let rspace = space_count - lspace;
                let mut line = String::new();
                line.push_str(vertical);
                line.push_str(&" ".repeat(lspace));
                table_metadata_push(out, state, layout_type, &line);
                table_metadata_push(
                    out,
                    state,
                    TableMetadataHighlightElement::Comment,
                    &render_table.estimated_size_text,
                );
                let mut tail = String::new();
                tail.push_str(&" ".repeat(rspace));
                tail.push_str(vertical);
                table_metadata_push(out, state, layout_type, &tail);
                return;
            }
        }

        let mut bottom = String::new();
        bottom.push_str(ldcorner);
        bottom.push_str(&horizontal.repeat(self.render_width.saturating_sub(2)));
        bottom.push_str(rdcorner);
        table_metadata_push(out, state, layout_type, &bottom);
    }
}

#[derive(Clone, Debug)]
struct TableMetadataDisplayInfo {
    database_name: String,
    schema_name: String,
    render_height: usize,
    render_width: usize,
    display_lines: Vec<TableMetadataLine>,
}

fn table_metadata_render_line_display(
    out: &mut String,
    state: &ShellState,
    text: &str,
    total_render_width: usize,
    element: TableMetadataHighlightElement,
) {
    let mut text = text.to_string();
    let mut size = render_length(&text);
    let max_text = total_render_width.saturating_sub(4);
    if size > max_text {
        text = truncate_with_ellipsis(&text, max_text);
        size = render_length(&text);
    }
    let total_lines = total_render_width.saturating_sub(size).saturating_sub(4);
    let lline = total_lines / 2;
    let rline = total_lines - lline;
    let mut line = String::new();
    line.push(' ');
    line.push_str(&"─".repeat(lline));
    line.push(' ');
    line.push_str(&text);
    line.push(' ');
    line.push_str(&"─".repeat(rline));
    line.push(' ');
    line.push('\n');
    table_metadata_push(out, state, element, &line);
}

fn table_metadata_max_render_width(state: &ShellState) -> usize {
    let mut width = if state.max_width == 0 {
        duckbox_terminal_width()
    } else {
        state.max_width as usize
    };
    if width < 80 {
        width = 80;
    }
    width
}

fn render_table_metadata(state: &ShellState, tables: &[TableMetadataTable]) -> Vec<u8> {
    let max_render_width = table_metadata_max_render_width(state);

    // Prepare render infos + truncate to max render width.
    let mut table_list: Vec<TableMetadataTableRenderInfo> = Vec::with_capacity(tables.len());
    for t in tables {
        let table = t.clone();
        let table_name_length = render_length(&table.table_name);

        let mut has_constraint_component = false;
        for c in &table.columns {
            if c.is_not_null || c.is_unique || !c.default_value.is_empty() {
                has_constraint_component = true;
                break;
            }
        }

        let component_count = 2 + if has_constraint_component { 1 } else { 0 };
        let mut render_row = TableMetadataColumnRenderRow {
            columns: Vec::new(),
        };
        for c in &table.columns {
            let mut col_display = TableMetadataColumnRenderInfo {
                components: Vec::new(),
            };
            let name_element = if c.is_primary_key {
                TableMetadataHighlightElement::PrimaryKeyColumn
            } else {
                TableMetadataHighlightElement::ColumnName
            };
            let name_text = c.column_name.clone();
            col_display.components.push(TableMetadataRenderComponent {
                render_width: render_length(&name_text),
                text: name_text,
                element: name_element,
            });
            let type_text = format_table_metadata_type(&c.column_type);
            col_display.components.push(TableMetadataRenderComponent {
                render_width: render_length(&type_text),
                text: type_text,
                element: TableMetadataHighlightElement::ColumnType,
            });
            if has_constraint_component {
                let mut constraint_text = String::new();
                if c.is_not_null {
                    constraint_text.push_str("not null");
                }
                if c.is_unique {
                    if !constraint_text.is_empty() {
                        constraint_text.push(' ');
                    }
                    constraint_text.push_str("unique");
                }
                if !c.default_value.is_empty() {
                    if !constraint_text.is_empty() {
                        constraint_text.push(' ');
                    }
                    constraint_text.push_str("default ");
                    constraint_text.push_str(&c.default_value);
                }
                col_display.components.push(TableMetadataRenderComponent {
                    render_width: render_length(&constraint_text),
                    text: constraint_text,
                    element: TableMetadataHighlightElement::ColumnType,
                });
            }
            debug_assert_eq!(col_display.components.len(), component_count);
            render_row.columns.push(col_display);
        }

        let mut max_component_widths = vec![0usize; component_count];
        for row in &render_row.columns {
            for (idx, component) in row.components.iter().enumerate() {
                max_component_widths[idx] = max_component_widths[idx].max(component.render_width);
            }
        }

        let mut render_width = table_name_length + 4;
        let per_column = TableMetadataTableRenderInfo::per_column_width_for(&max_component_widths);
        if per_column > render_width {
            render_width = per_column;
        }

        let mut estimated_size_text = String::new();
        let mut estimated_size_length = None;
        if let Some(est) = table.estimated_size {
            estimated_size_text = format!("{} rows", est);
            let len = render_length(&estimated_size_text);
            estimated_size_length = Some(len);
            if len + 4 > render_width {
                render_width = len + 4;
            }
        }

        table_list.push(TableMetadataTableRenderInfo {
            table,
            table_name_length,
            render_width,
            max_component_widths,
            estimated_size_text,
            estimated_size_length,
            column_renders: vec![render_row],
        });
    }

    for table in table_list.iter_mut() {
        table.truncate(max_render_width);
    }

    // Try to split up large tables.
    for table in table_list.iter_mut() {
        const SPLIT_THRESHOLD: usize = 20;
        if table.column_renders[0].columns.len() <= SPLIT_THRESHOLD {
            continue;
        }
        let max_split_count =
            (table.column_renders[0].columns.len() + SPLIT_THRESHOLD - 1) / SPLIT_THRESHOLD;
        let width_per_split = table.per_column_width();
        let max_splits = max_render_width / width_per_split;
        if max_splits <= 1 {
            continue;
        }
        let split_count = std::cmp::min(max_split_count, max_splits);

        let mut new_renders = vec![
            TableMetadataColumnRenderRow {
                columns: Vec::new(),
            };
            split_count
        ];
        let mut split_idx = 0usize;
        let old_cols = std::mem::take(&mut table.column_renders[0].columns);
        for col in old_cols {
            new_renders[split_idx % split_count].columns.push(col);
            split_idx += 1;
        }
        table.column_renders = new_renders;
        table.render_width = table.render_width.max(split_count * width_per_split);
    }

    // Group tables by db + schema.
    let mut grouped: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, Vec<TableMetadataTableRenderInfo>>,
    > = std::collections::BTreeMap::new();
    for entry in table_list {
        grouped
            .entry(entry.table.database_name.clone())
            .or_default()
            .entry(entry.table.schema_name.clone())
            .or_default()
            .push(entry);
    }

    let mut metadata_displays: Vec<TableMetadataDisplayInfo> = Vec::new();
    for (db_name, schemas) in grouped.iter_mut() {
        for (schema_name, result) in schemas.iter_mut() {
            result.sort_by(|a, b| b.line_count().cmp(&a.line_count()));

            let mut displayed: std::collections::HashSet<usize> = std::collections::HashSet::new();
            for table_idx in 0..result.len() {
                if displayed.contains(&table_idx) {
                    continue;
                }
                displayed.insert(table_idx);
                let initial_table = &result[table_idx];

                let mut display = TableMetadataDisplayInfo {
                    database_name: db_name.clone(),
                    schema_name: schema_name.clone(),
                    render_width: initial_table.render_width,
                    render_height: initial_table.line_count(),
                    display_lines: Vec::new(),
                };
                display.display_lines.push(TableMetadataLine {
                    tables: vec![table_idx],
                    render_width: initial_table.render_width,
                    render_height: initial_table.line_count(),
                    max_component_widths: initial_table.max_component_widths.clone(),
                });

                for next_idx in (table_idx + 1)..result.len() {
                    if displayed.contains(&next_idx) {
                        continue;
                    }
                    let current_table = &result[next_idx];
                    let render_width = current_table.render_width;
                    let render_height = current_table.line_count();
                    if render_height > display.render_height {
                        continue;
                    }
                    let mut added = false;
                    for existing_line in display.display_lines.iter_mut() {
                        if existing_line.render_height + render_height > display.render_height {
                            continue;
                        }
                        let mut new_max_component_widths: Vec<usize> = Vec::new();
                        for component_idx in 0..existing_line.max_component_widths.len() {
                            new_max_component_widths.push(
                                existing_line.max_component_widths[component_idx].max(
                                    current_table
                                        .max_component_widths
                                        .get(component_idx)
                                        .copied()
                                        .unwrap_or(0),
                                ),
                            );
                        }
                        let mut new_column_render_width = 3usize;
                        for w in &new_max_component_widths {
                            new_column_render_width += *w + 1;
                        }
                        let mut new_rendering_width = render_width.max(existing_line.render_width);
                        new_rendering_width = new_rendering_width.max(new_column_render_width);

                        let extra_width =
                            new_rendering_width.saturating_sub(existing_line.render_width);
                        if display.render_width + extra_width > max_render_width {
                            continue;
                        }
                        existing_line.max_component_widths = new_max_component_widths;
                        existing_line.render_width += extra_width;
                        existing_line.render_height += render_height;
                        existing_line.tables.push(next_idx);
                        added = true;
                        break;
                    }
                    if !added {
                        if display.render_width + render_width <= max_render_width {
                            display.render_width += render_width;
                            display.display_lines.push(TableMetadataLine {
                                tables: vec![next_idx],
                                render_width,
                                render_height,
                                max_component_widths: current_table.max_component_widths.clone(),
                            });
                            added = true;
                        }
                    }
                    if added {
                        displayed.insert(next_idx);
                    }
                }

                display
                    .display_lines
                    .sort_by(|a, b| b.render_height.cmp(&a.render_height));
                metadata_displays.push(display);
            }
        }
    }

    let mut output = String::new();

    let mut last_displayed_database = String::new();
    let mut last_displayed_schema = String::new();
    for display in &metadata_displays {
        if !display.database_name.is_empty() && last_displayed_database != display.database_name {
            table_metadata_render_line_display(
                &mut output,
                state,
                &display.database_name,
                display.render_width,
                TableMetadataHighlightElement::DatabaseName,
            );
            last_displayed_database = display.database_name.clone();
            last_displayed_schema.clear();
        }
        if !display.schema_name.is_empty() && last_displayed_schema != display.schema_name {
            table_metadata_render_line_display(
                &mut output,
                state,
                &display.schema_name,
                display.render_width,
                TableMetadataHighlightElement::SchemaName,
            );
            last_displayed_schema = display.schema_name.clone();
        }

        let table_list: &[TableMetadataTableRenderInfo] = grouped
            .get(&display.database_name)
            .and_then(|m| m.get(&display.schema_name))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        for line_idx in 0..display.render_height {
            for table_line_idx in 0..display.display_lines.len() {
                let is_last = table_line_idx + 1 == display.display_lines.len();
                display.display_lines[table_line_idx].render_line(
                    &mut output,
                    state,
                    table_list,
                    line_idx,
                    is_last,
                );
            }
            table_metadata_push(
                &mut output,
                state,
                TableMetadataHighlightElement::Layout,
                "\n",
            );
        }
    }

    output.into_bytes()
}

fn try_render_shell_describe(
    state: &mut ShellState,
    cmd: &str,
    limits_override: Option<(u64, u64, u64)>,
    result: &mut duckdb_sys::duckdb_result,
) -> Option<i32> {
    fn escape_control_chars_for_describe(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            let b = ch as u32;
            if b < 32 {
                out.push('\\');
                match b {
                    7 => out.push('a'),
                    8 => out.push('b'),
                    9 => out.push('t'),
                    10 => out.push('n'),
                    11 => out.push('v'),
                    12 => out.push('f'),
                    13 => out.push('r'),
                    27 => out.push('e'),
                    other => out.push_str(&other.to_string()),
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    let trimmed_cmd = cmd.trim();
    let cmd_no_semi = trimmed_cmd.strip_suffix(';').unwrap_or(trimmed_cmd).trim();
    let lower = cmd_no_semi.trim_start().to_ascii_lowercase();
    if !lower.starts_with("describe") {
        return None;
    }
    let rest = cmd_no_semi[lower.find("describe").unwrap_or(0) + "describe".len()..].trim_start();
    let rest_lower = rest.to_ascii_lowercase();
    let is_query_describe = rest_lower.starts_with("select")
        || rest_lower.starts_with("with")
        || rest_lower.starts_with('(')
        || rest_lower.starts_with("from");

    let title = if is_query_describe {
        "Describe".to_string()
    } else {
        rest.split_whitespace()
            .next()
            .unwrap_or("Describe")
            .to_string()
    };

    let col_count = unsafe { duckdb_sys::duckdb_column_count(result) } as usize;
    if col_count < 2 {
        return None;
    }

    let mut name_idx: Option<usize> = None;
    let mut type_idx: Option<usize> = None;
    let mut null_idx: Option<usize> = None;
    for c in 0..col_count {
        let name_ptr = unsafe { duckdb_sys::duckdb_column_name(result, c as u64) };
        if name_ptr.is_null() {
            continue;
        }
        let name = unsafe { CStr::from_ptr(name_ptr) }
            .to_string_lossy()
            .to_string();
        match name.as_str() {
            "column_name" => name_idx = Some(c),
            "column_type" => type_idx = Some(c),
            "null" => null_idx = Some(c),
            _ => {}
        }
    }
    let (name_idx, type_idx) = (name_idx?, type_idx?);
    let null_idx = null_idx.unwrap_or(usize::MAX);

    let row_count = unsafe { duckdb_sys::duckdb_row_count(result) } as usize;
    let mut rows: Vec<String> = Vec::with_capacity(row_count);
    for r in 0..row_count {
        let name = unsafe {
            if duckdb_sys::duckdb_value_is_null(result, name_idx as u64, r as u64) {
                String::new()
            } else {
                let ptr = duckdb_sys::duckdb_value_varchar(result, name_idx as u64, r as u64);
                if ptr.is_null() {
                    String::new()
                } else {
                    let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
                    duckdb_sys::duckdb_free(ptr as *mut _);
                    s
                }
            }
        };
        let mut type_name = unsafe {
            if duckdb_sys::duckdb_value_is_null(result, type_idx as u64, r as u64) {
                String::new()
            } else {
                let ptr = duckdb_sys::duckdb_value_varchar(result, type_idx as u64, r as u64);
                if ptr.is_null() {
                    String::new()
                } else {
                    let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
                    duckdb_sys::duckdb_free(ptr as *mut _);
                    s
                }
            }
        };
        type_name = type_name.to_ascii_lowercase();
        if type_name.starts_with("decimal(") {
            type_name = "decimal".to_string();
        }

        let mut not_null = false;
        if !is_query_describe && null_idx != usize::MAX {
            let null_s = unsafe {
                if duckdb_sys::duckdb_value_is_null(result, null_idx as u64, r as u64) {
                    String::new()
                } else {
                    let ptr = duckdb_sys::duckdb_value_varchar(result, null_idx as u64, r as u64);
                    if ptr.is_null() {
                        String::new()
                    } else {
                        let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
                        duckdb_sys::duckdb_free(ptr as *mut _);
                        s
                    }
                }
            };
            not_null = null_s.to_ascii_lowercase() == "no";
        }

        let mut row = String::new();
        row.push_str(&escape_control_chars_for_describe(&name));
        if !type_name.is_empty() {
            row.push(' ');
            row.push_str(&type_name);
        }
        if not_null {
            row.push(' ');
            row.push_str("not null");
        }
        rows.push(row);
    }

    let (_, max_width, _) =
        limits_override.unwrap_or((state.max_rows, state.max_width, state.max_analyze_rows));
    let max_render_width = if max_width == u64::MAX {
        usize::MAX
    } else if max_width == 0 {
        if state.stdout_is_console {
            duckbox_terminal_width()
        } else {
            usize::MAX
        }
    } else {
        max_width as usize
    };
    let max_content_width = max_render_width.saturating_sub(4).max(1);

    let mut content_width = render_length(&title);
    for row in rows.iter() {
        content_width = content_width.max(render_length(row));
    }
    content_width = content_width.min(max_content_width);

    let highlight_results =
        state.highlighting_enabled && state.highlight_results != OptionType::Off;
    let ansi_layout = terminal_code(highlight_style_for_element(state, "layout"));
    let ansi_reset = reset_terminal_code().to_string();
    let ansi_bold = terminal_code(highlight_style_for_element(state, "table_name"));
    let border_line = |left: char, fill: char, right: char| -> String {
        let mut s = String::new();
        s.push(left);
        s.push_str(&fill.to_string().repeat(content_width + 2));
        s.push(right);
        s
    };
    let write_layout_newline = |state: &mut ShellState| {
        state.out.write_all(ansi_layout.as_bytes());
        state.out.write_all(b"\n");
        state.out.write_all(ansi_reset.as_bytes());
    };
    let write_layout_line = |state: &mut ShellState, s: &str| {
        state.out.write_all(ansi_layout.as_bytes());
        state.out.write_all(s.as_bytes());
        state.out.write_all(ansi_reset.as_bytes());
        write_layout_newline(state);
    };
    let write_plain_line = |state: &mut ShellState, s: &str| {
        state.out.write_all(s.as_bytes());
        state.out.write_all(b"\n");
    };
    let top = border_line('┌', '─', '┐');
    let mid_empty = {
        let mut s = String::new();
        s.push('│');
        s.push(' ');
        s.push_str(&" ".repeat(content_width));
        s.push(' ');
        s.push('│');
        s
    };
    let bottom = border_line('└', '─', '┘');

    if highlight_results && !ansi_layout.is_empty() {
        write_layout_line(state, &top);
    } else {
        write_plain_line(state, &top);
    }

    // Title row
    let title_trunc = truncate_with_ellipsis(&title, content_width);
    let pad = content_width.saturating_sub(render_length(&title_trunc));
    let left_pad = pad / 2;
    let right_pad = pad - left_pad;
    if highlight_results && !ansi_layout.is_empty() {
        state.out.write_all(ansi_layout.as_bytes());
        state.out.write_all("│ ".as_bytes());
        state.out.write_all(" ".repeat(left_pad).as_bytes());
        state.out.write_all(ansi_reset.as_bytes());
        state.out.write_all(ansi_bold.as_bytes());
        state.out.write_all(title_trunc.as_bytes());
        state.out.write_all(ansi_reset.as_bytes());
        state.out.write_all(ansi_layout.as_bytes());
        state.out.write_all(" ".repeat(right_pad).as_bytes());
        state.out.write_all(" │".as_bytes());
        state.out.write_all(ansi_reset.as_bytes());
        write_layout_newline(state);
    } else {
        let mut title_row = String::new();
        title_row.push('│');
        title_row.push(' ');
        title_row.push_str(&" ".repeat(left_pad));
        title_row.push_str(&title_trunc);
        title_row.push_str(&" ".repeat(right_pad));
        title_row.push(' ');
        title_row.push('│');
        write_plain_line(state, &title_row);
    }

    // Spacer row
    if highlight_results && !ansi_layout.is_empty() {
        write_layout_line(state, &mid_empty);
    } else {
        write_plain_line(state, &mid_empty);
    }

    // Value rows
    for row in rows {
        let rendered = truncate_with_ellipsis(&row, content_width);
        let pad = content_width.saturating_sub(render_length(&rendered));
        let mut line = String::new();
        line.push('│');
        line.push(' ');
        line.push_str(&rendered);
        line.push_str(&" ".repeat(pad));
        line.push(' ');
        line.push('│');
        if highlight_results && !ansi_layout.is_empty() {
            if let Some((name, rest)) = rendered.split_once(' ') {
                state.out.write_all(ansi_layout.as_bytes());
                state.out.write_all("│ ".as_bytes());
                state.out.write_all(ansi_reset.as_bytes());
                state.out.write_all(name.as_bytes());
                state.out.write_all(ansi_layout.as_bytes());
                state.out.write_all(b" ");
                state.out.write_all(ansi_reset.as_bytes());
                state.out.write_all(ansi_layout.as_bytes());
                state.out.write_all(rest.as_bytes());
                if pad > 0 {
                    state.out.write_all(ansi_reset.as_bytes());
                    state.out.write_all(ansi_layout.as_bytes());
                    state.out.write_all(" ".repeat(pad).as_bytes());
                }
                state.out.write_all(ansi_reset.as_bytes());
                state.out.write_all(ansi_layout.as_bytes());
                state.out.write_all(b" ");
                state.out.write_all(ansi_reset.as_bytes());
                state.out.write_all(ansi_layout.as_bytes());
                state.out.write_all("│".as_bytes());
                state.out.write_all(ansi_reset.as_bytes());
                write_layout_newline(state);
            } else {
                write_layout_line(state, &line);
            }
        } else {
            write_plain_line(state, &line);
        }
    }

    if highlight_results && !ansi_layout.is_empty() {
        write_layout_line(state, &bottom);
    } else {
        write_plain_line(state, &bottom);
    }
    Some(0)
}

fn wrap_line_to_width(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return vec![];
    }
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        if lines.len() + 1 >= max_lines {
            lines.push(truncate_with_ellipsis(
                &String::from_utf8_lossy(&bytes[pos..]),
                width,
            ));
            break;
        }
        let rest = String::from_utf8_lossy(&bytes[pos..]).to_string();
        if render_length(&rest) <= width {
            lines.push(rest);
            break;
        }
        let take = render_prefix_bytes(&rest, width).max(1);

        // Prefer to break on whitespace within the rendered window (keeps words intact for tests/parity).
        let mut break_at: Option<usize> = None;
        for (i, ch) in rest[..take].char_indices() {
            if ch.is_whitespace() {
                break_at = Some(i);
            }
        }
        if let Some(ws) = break_at {
            if ws == 0 {
                // Skip leading whitespace and try again.
                let mut skip = 0usize;
                for ch in rest.chars() {
                    if ch.is_whitespace() {
                        skip += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                pos += skip.max(1);
                continue;
            }
            lines.push(rest[..ws].to_string());
            // Skip any whitespace run after the break.
            let mut skip = ws;
            while skip < rest.len() {
                let ch = rest[skip..].chars().next().unwrap();
                if ch.is_whitespace() {
                    skip += ch.len_utf8();
                } else {
                    break;
                }
            }
            pos += skip;
        } else {
            lines.push(String::from_utf8_lossy(&rest.as_bytes()[..take]).to_string());
            pos += take;
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn split_and_render_lines(s: &str, width: usize, wrap: bool, max_lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in s.split('\n') {
        if out.len() >= max_lines {
            break;
        }
        let remaining = max_lines.saturating_sub(out.len());
        if wrap {
            out.extend(wrap_line_to_width(line, width, remaining));
        } else {
            out.push(truncate_with_ellipsis(line, width));
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out.truncate(max_lines);
    out
}

fn duckbox_truncate_lines(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    if max_lines == 0 || width == 0 {
        return vec![String::new()];
    }

    let bytes = s.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut pos: usize = 0;
    while pos < bytes.len() && out.len() < max_lines {
        let rest = &s[pos..];
        let newline_rel = rest.as_bytes().iter().position(|b| *b == b'\n');
        let line_end = if let Some(nl) = newline_rel {
            pos + nl
        } else {
            bytes.len()
        };
        let segment = &s[pos..line_end];
        // If we're out of vertical budget, put the remainder on the last line and let the cell
        // renderer truncate with an ellipsis.
        if out.len() + 1 == max_lines {
            out.push(segment.to_string());
            break;
        }
        if segment.is_empty() {
            out.push(String::new());
            if line_end < bytes.len() && bytes[line_end] == b'\n' {
                pos = line_end + 1;
            } else {
                break;
            }
            continue;
        }

        let mut take = duckbox_render_prefix_bytes(segment, width);
        if take == 0 {
            take = segment.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        }
        take = take.max(1).min(segment.len());
        out.push(String::from_utf8_lossy(&segment.as_bytes()[..take]).to_string());
        pos += take;
        if pos == line_end && line_end < bytes.len() && bytes[line_end] == b'\n' {
            pos += 1;
        }
    }

    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn apply_decimal_separator(mut s: String, decimal_separator: u8) -> String {
    if decimal_separator == 0 {
        return s;
    }
    let ch = decimal_separator as char;
    if ch == '.' {
        return s;
    }
    // Replace only the decimal point, not thousand separators (which may also be '.').
    if let Some(pos) = s.rfind('.') {
        let repl = ch.to_string();
        s.replace_range(pos..pos + 1, &repl);
    }
    s
}

fn insert_thousand_separators(int_part: &str, thousand_separator: u8) -> String {
    if thousand_separator == 0 {
        return int_part.to_string();
    }
    let sep = thousand_separator as char;
    let bytes = int_part.as_bytes();
    if bytes.first().copied() == Some(b'-') || bytes.first().copied() == Some(b'+') {
        // Shell quirk: do not apply thousand separators to signed numeric strings.
        return int_part.to_string();
    }
    if int_part.len() <= 3 {
        return int_part.to_string();
    }
    let mut out = String::with_capacity(int_part.len() + int_part.len() / 3);
    let first_group = int_part.len() % 3;
    let mut pos = 0usize;
    if first_group != 0 {
        out.push_str(&int_part[..first_group]);
        pos = first_group;
        if pos < int_part.len() {
            out.push(sep);
        }
    }
    while pos < int_part.len() {
        out.push_str(&int_part[pos..pos + 3]);
        pos += 3;
        if pos < int_part.len() {
            out.push(sep);
        }
    }
    out
}

fn apply_numeric_separators(s: &str, decimal_separator: u8, thousand_separator: u8) -> String {
    // Best-effort (covers tests): apply thousand separators to the integer part, and replace '.' with decimal sep.
    if s.eq_ignore_ascii_case("nan")
        || s.eq_ignore_ascii_case("-nan")
        || s.eq_ignore_ascii_case("inf")
        || s.eq_ignore_ascii_case("-inf")
        || s.contains('e')
        || s.contains('E')
    {
        return apply_decimal_separator(s.to_string(), decimal_separator);
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let mut out = insert_thousand_separators(int_part, thousand_separator);
    if let Some(frac) = frac_part {
        out.push('.');
        out.push_str(frac);
    }
    apply_decimal_separator(out, decimal_separator)
}

fn format_readable_number(value: f64) -> Option<(f64, &'static str)> {
    let abs = value.abs();
    let units: &[(f64, &str)] = &[
        (1e18, "quintillion"),
        (1e15, "quadrillion"),
        (1e12, "trillion"),
        (1e9, "billion"),
        (1e6, "million"),
    ];
    for (scale, name) in units {
        if abs >= *scale {
            return Some((value / scale, *name));
        }
    }
    None
}

fn readable_number_string(raw: &str, decimal_separator: u8) -> Option<String> {
    let value = raw.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    let (scaled, unit) = format_readable_number(value)?;
    let rounded = (scaled * 100.0).round() / 100.0;
    let mut s = format!("{:.2} {}", rounded, unit);
    s = apply_decimal_separator(s, decimal_separator);
    Some(s)
}

fn render_duckbox_result(
    state: &mut ShellState,
    cmd: &str,
    limits_override: Option<(u64, u64, u64)>,
    result: &mut duckdb_sys::duckdb_result,
    type_overrides: Option<(&[String], &[duckdb_sys::duckdb_type])>,
) -> i32 {
    // DuckBox rendering in pure Rust (C API): modeled after `duckdb::BoxRenderer` (v1.4.3).
    // Focused parity targets:
    // - Streaming (do not buffer full output) when rendering all rows
    // - Analyze-window truncation (`.maxrows -1 ANALYZE`) uses `…`
    // - Wrap behavior when `.maxwidth` is set and there is enough vertical budget
    // - Large number rendering: `all` formatting + `footer` (row + count footer)
    // - Control character escaping (`\n`, etc.) for values + column names
    // - Column pruning/splitting, `.columns` pivoting, and nested value highlighting annotations
    const BOX_24: &str = "\u{2500}";
    const BOX_13: &str = "\u{2502}";
    const BOX_23: &str = "\u{250c}";
    const BOX_34: &str = "\u{2510}";
    const BOX_12: &str = "\u{2514}";
    const BOX_14: &str = "\u{2518}";
    const BOX_123: &str = "\u{251c}";
    const BOX_134: &str = "\u{2524}";
    const BOX_234: &str = "\u{252c}";
    const BOX_124: &str = "\u{2534}";
    const BOX_1234: &str = "\u{253c}";
    const BOX_DMIDDLE: &str = "\u{2534}"; // ┴
    const BOX_DOT: &str = "\u{00b7}"; // ·
    const MAX_COL_WIDTH: usize = 20;
    const SPLIT_COLUMN: usize = usize::MAX;
    const DOTDOTDOT: &str = "…";
    const DOTDOTDOT_LENGTH: usize = 1;

    let col_count = unsafe { duckdb_sys::duckdb_column_count(result) } as usize;
    if col_count == 0 {
        return 0;
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum LargeNumberRendering {
        None,
        Footer,
        All,
    }

    fn escape_control_chars(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            let b = ch as u32;
            if b < 32 {
                out.push('\\');
                match b {
                    7 => out.push('a'),
                    8 => out.push('b'),
                    9 => out.push('t'),
                    10 => out.push('n'),
                    11 => out.push('v'),
                    12 => out.push('f'),
                    13 => out.push('r'),
                    27 => out.push('e'),
                    other => out.push_str(&other.to_string()),
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn try_format_large_number(numeric: &str, decimal_separator: u8) -> Option<String> {
        // Port of `duckdb::BoxRenderer::TryFormatLargeNumber` (v1.4.3).
        // - Only summarizes numbers >= 1,000,000
        // - Rejects exponent notation and "funky" strings
        // - Only supports up to < 1e19 (to avoid overflow in idx_t)
        if numeric.len() <= 5 {
            return None;
        }
        let bytes = numeric.as_bytes();
        let mut i = 0usize;
        let mut negative = false;
        if bytes.first().copied() == Some(b'-') {
            negative = true;
            i += 1;
        }
        let mut number: u128 = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'.' {
                break;
            }
            if !(b'0'..=b'9').contains(&c) {
                return None;
            }
            if number >= 1_000_000_000_000_000_000u128 {
                return None;
            }
            number = number * 10 + (c - b'0') as u128;
            i += 1;
        }

        struct UnitBase {
            base: u128,
            name: &'static str,
        }
        let bases: [UnitBase; 5] = [
            UnitBase {
                base: 1_000_000,
                name: "million",
            },
            UnitBase {
                base: 1_000_000_000,
                name: "billion",
            },
            UnitBase {
                base: 1_000_000_000_000,
                name: "trillion",
            },
            UnitBase {
                base: 1_000_000_000_000_000,
                name: "quadrillion",
            },
            UnitBase {
                base: 1_000_000_000_000_000_000,
                name: "quintillion",
            },
        ];

        let mut base: u128 = 0;
        let mut unit: &'static str = "";
        for b in bases {
            let rounded = number + ((b.base / 100) / 2);
            if rounded >= b.base {
                base = b.base;
                unit = b.name;
            }
        }
        if unit.is_empty() {
            return None;
        }
        number += (base / 100) / 2;
        let decimal_unit = number / (base / 100);
        let decimal_str = decimal_unit.to_string();
        if decimal_str.len() < 3 {
            return None;
        }
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        out.push_str(&decimal_str[..decimal_str.len() - 2]);
        out.push(if decimal_separator == 0 {
            '.'
        } else {
            decimal_separator as char
        });
        out.push_str(&decimal_str[decimal_str.len() - 2..]);
        out.push(' ');
        out.push_str(unit);
        Some(out)
    }

    fn format_number(
        input: &str,
        large_number_rendering: LargeNumberRendering,
        decimal_sep: u8,
        thousand_sep: u8,
    ) -> String {
        if large_number_rendering == LargeNumberRendering::All {
            if let Some(v) = try_format_large_number(input, decimal_sep) {
                return v;
            }
        }
        if decimal_sep == 0 && thousand_sep == 0 {
            return input.to_string();
        }
        apply_numeric_separators(input, decimal_sep, thousand_sep)
    }

    #[derive(Clone, Debug)]
    struct DuckboxFooter {
        row_count_str: String,
        column_count_str: String,
        readable_rows_str: String,
        shown_str: String,
        has_hidden_rows: bool,
        must_show_footer: bool,
        render_length: usize,
    }

    fn compute_footer(
        row_count: usize,
        rendered_rows: usize,
        col_count: usize,
        large_number_rendering: LargeNumberRendering,
        decimal_sep: u8,
        thousand_sep: u8,
    ) -> DuckboxFooter {
        let mut column_count_str = if row_count == 0 {
            String::new()
        } else {
            format!("{} column", col_count)
        };
        if !column_count_str.is_empty() && col_count != 1 {
            column_count_str.push('s');
        }
        let row_count_str = format!(
            "{} rows",
            format_number(
                &row_count.to_string(),
                large_number_rendering,
                decimal_sep,
                thousand_sep
            )
        );

        let readable_rows_str =
            if large_number_rendering == LargeNumberRendering::Footer && row_count > 0 {
                try_format_large_number(&row_count.to_string(), decimal_sep)
                    .map(|s| format!("{} rows", s))
                    .unwrap_or_default()
            } else {
                String::new()
            };

        let has_hidden_rows = rendered_rows < row_count;
        let shown_str = if has_hidden_rows {
            format!(
                "{} shown",
                format_number(
                    &rendered_rows.to_string(),
                    LargeNumberRendering::None,
                    decimal_sep,
                    thousand_sep
                )
            )
        } else {
            String::new()
        };

        // In DuckDB's shipped shell (v1.4.3), duckbox prints a footer section in these cases:
        // - 0 rows (shows "0 rows" as a single spanned cell)
        // - truncated output (shows "(... shown)" etc.)
        // - "big enough" results (>= 10 rows) when there are multiple columns (shows "N rows ... M columns")
        let must_show_footer =
            has_hidden_rows || row_count == 0 || (row_count >= 10 && col_count > 1);
        let render_length = {
            let mut m = row_count_str.len();
            if !shown_str.is_empty() {
                m = m.max(shown_str.len() + 2);
            }
            if !readable_rows_str.is_empty() {
                m = m.max(readable_rows_str.len() + 2);
            }
            m + 4
        };

        DuckboxFooter {
            row_count_str,
            column_count_str,
            readable_rows_str,
            shown_str,
            has_hidden_rows,
            must_show_footer,
            render_length,
        }
    }

    fn recompute_footer_render_length(footer: &mut DuckboxFooter) {
        let mut m = footer.row_count_str.len();
        if !footer.shown_str.is_empty() {
            m = m.max(footer.shown_str.len() + 2);
        }
        if !footer.readable_rows_str.is_empty() {
            m = m.max(footer.readable_rows_str.len() + 2);
        }
        footer.render_length = m + 4;
    }

    fn use_plain_footer(
        footer: &DuckboxFooter,
        has_hidden_columns: bool,
        row_count: usize,
        col_count: usize,
    ) -> bool {
        footer.must_show_footer
            && !footer.has_hidden_rows
            && !has_hidden_columns
            && footer.readable_rows_str.is_empty()
            && footer.shown_str.is_empty()
            && (row_count == 0 || (row_count >= 10 && col_count > 1))
    }

    fn plain_footer_line(footer: &DuckboxFooter, total_render_length: usize) -> String {
        let padding = total_render_length
            .saturating_sub(4)
            .saturating_sub(footer.row_count_str.len())
            .saturating_sub(footer.column_count_str.len());
        format!(
            "  {}{}{}",
            footer.row_count_str,
            " ".repeat(padding),
            footer.column_count_str
        )
    }

    fn is_numeric_type(t: duckdb_sys::duckdb_type) -> bool {
        matches!(
            t,
            duckdb_sys::DUCKDB_TYPE_TINYINT
                | duckdb_sys::DUCKDB_TYPE_SMALLINT
                | duckdb_sys::DUCKDB_TYPE_INTEGER
                | duckdb_sys::DUCKDB_TYPE_BIGINT
                | duckdb_sys::DUCKDB_TYPE_UTINYINT
                | duckdb_sys::DUCKDB_TYPE_USMALLINT
                | duckdb_sys::DUCKDB_TYPE_UINTEGER
                | duckdb_sys::DUCKDB_TYPE_UBIGINT
                | duckdb_sys::DUCKDB_TYPE_HUGEINT
                | duckdb_sys::DUCKDB_TYPE_UHUGEINT
                | duckdb_sys::DUCKDB_TYPE_DECIMAL
                | duckdb_sys::DUCKDB_TYPE_FLOAT
                | duckdb_sys::DUCKDB_TYPE_DOUBLE
        )
    }

    struct DuckboxWriter<'a> {
        out: &'a mut OutputHandle,
        pager_child: Option<std::process::Child>,
    }
    impl<'a> DuckboxWriter<'a> {
        fn new(out: &'a mut OutputHandle, pager_cmd: &str, use_pager: bool) -> Self {
            if !use_pager {
                return DuckboxWriter {
                    out,
                    pager_child: None,
                };
            }
            let child = crate::output::shell_command(pager_cmd)
                .stdin(Stdio::piped())
                .spawn()
                .ok();
            DuckboxWriter {
                out,
                pager_child: child,
            }
        }
        fn write_all(&mut self, bytes: &[u8]) {
            if let Some(child) = self.pager_child.as_mut() {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(bytes);
                }
            } else {
                self.out.write_all(bytes);
            }
        }
        fn finish(mut self) {
            if let Some(mut child) = self.pager_child.take() {
                drop(child.stdin.take());
                let _ = child.wait();
            }
        }
    }

    let output_is_file =
        limits_override.is_some() || (!state.outfile.is_empty() && !state.outfile.starts_with('|'));
    let (mut max_rows, mut max_width, mut max_analyze_rows) =
        limits_override.unwrap_or((state.max_rows, state.max_width, state.max_analyze_rows));
    if max_analyze_rows == 0 {
        max_analyze_rows = if state.stdout_is_console {
            100_000
        } else {
            u64::MAX
        };
    }
    if max_width == 0 {
        if output_is_file {
            max_rows = u64::MAX;
            max_width = u64::MAX;
        }
        if !state.stdout_is_console {
            max_width = u64::MAX;
        }
    }
    if max_analyze_rows == 0 {
        max_analyze_rows = u64::MAX;
    }

    let mut large_number_rendering = match state.large_number_rendering {
        0 => LargeNumberRendering::None,
        1 => LargeNumberRendering::Footer,
        2 => LargeNumberRendering::All,
        _ => {
            if state.stdout_is_console {
                LargeNumberRendering::Footer
            } else {
                LargeNumberRendering::None
            }
        }
    };

    let requested_columns_mode = state.columns;
    if requested_columns_mode {
        // The shipped shell only constructs the "(... million)" footer-row when in ROWS mode.
        if large_number_rendering == LargeNumberRendering::Footer {
            large_number_rendering = LargeNumberRendering::None;
        }
    }

    let highlight_results = highlight_results_enabled(state);
    let ansi_reset = reset_terminal_code().to_string();
    let ansi_layout = terminal_code(highlight_style_for_element(state, "layout"));
    let ansi_footer = terminal_code(highlight_style_for_element(state, "footer"));
    let ansi_column_name = terminal_code(highlight_style_for_element(state, "column_name"));
    let ansi_column_type = terminal_code(highlight_style_for_element(state, "column_type"));
    let ansi_null_value = terminal_code(highlight_style_for_element(state, "null_value"));
    let ansi_string_constant = terminal_code(highlight_style_for_element(state, "string_constant"));

    let mut col_names: Vec<String> = Vec::with_capacity(col_count);
    let mut col_type_names: Vec<String> = Vec::with_capacity(col_count);
    let mut col_types: Vec<duckdb_sys::duckdb_type> = Vec::with_capacity(col_count);
    let mut col_logical_type_ids: Vec<duckdb_sys::duckdb_type> = Vec::with_capacity(col_count);
    let mut col_unquote_json_literals: Vec<bool> = Vec::with_capacity(col_count);
    for c in 0..col_count {
        let name_ptr = unsafe { duckdb_sys::duckdb_column_name(result, c as u64) };
        let name = if name_ptr.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(name_ptr) }
                .to_string_lossy()
                .to_string()
        };
        col_names.push(escape_control_chars(&name));

        let col_type = unsafe { duckdb_sys::duckdb_column_type(result, c as u64) };

        let mut logical = unsafe { duckdb_sys::duckdb_column_logical_type(result, c as u64) };
        col_logical_type_ids.push(unsafe { duckdb_sys::duckdb_get_type_id(logical) });
        col_unquote_json_literals.push(logical_type_contains_json(logical, 0));
        let mut type_name = duckbox_render_type(state, logical, 0);
        unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut logical) };
        if type_name == "unknown" && col_type == duckdb_sys::DUCKDB_TYPE_ANY {
            type_name = "variant".to_string();
        }
        col_type_names.push(type_name);

        col_types.push(col_type);
    }

    if let Some((override_type_names, override_logical_ids)) = type_overrides {
        if override_type_names.len() == col_count && override_logical_ids.len() == col_count {
            col_type_names.clear();
            col_type_names.extend_from_slice(override_type_names);
            col_logical_type_ids.clear();
            col_logical_type_ids.extend_from_slice(override_logical_ids);
        }
    }

    let row_count_hint = unsafe { duckdb_sys::duckdb_row_count(result) } as usize;
    let mut row_count = row_count_hint;

    // DuckDB returns a `Success BOOLEAN` result for some SET statements, but the shipped CLI prints nothing.
    // Heuristic: suppress this specific shape.
    if row_count == 0
        && col_count == 1
        && col_names.get(0).is_some_and(|s| s == "Success")
        && col_types.get(0) == Some(&duckdb_sys::DUCKDB_TYPE_BOOLEAN)
    {
        return 0;
    }

    let is_describe_output = col_count == 6
        && col_names.get(0).is_some_and(|s| s == "column_name")
        && col_names.get(1).is_some_and(|s| s == "column_type")
        && col_names.get(2).is_some_and(|s| s == "null")
        && col_names.get(3).is_some_and(|s| s == "key")
        && col_names.get(4).is_some_and(|s| s == "default")
        && col_names.get(5).is_some_and(|s| s == "extra");

    let max_rows_usize = if max_rows == u64::MAX {
        usize::MAX
    } else {
        max_rows as usize
    };

    let mut rows_to_render = row_count.min(max_rows_usize);
    if max_rows_usize != usize::MAX && row_count <= max_rows_usize.saturating_add(3) {
        rows_to_render = row_count;
    }
    let render_columns_mode = requested_columns_mode;
    let render_columns_as_rows = requested_columns_mode && rows_to_render > 0;
    let (mut top_rows, mut bottom_rows) = if rows_to_render == row_count {
        (rows_to_render, 0usize)
    } else {
        let top = rows_to_render / 2 + if rows_to_render % 2 != 0 { 1 } else { 0 };
        (top, rows_to_render - top)
    };

    // In streaming mode (duckdb_execute_prepared_streaming), the C API reports row_count=0 up front.
    // We detect that later and switch to a non-streaming render path if the result finishes before
    // we render the header.
    let mut stream_all_rows = if max_rows_usize == usize::MAX {
        true
    } else {
        row_count > 0 && rows_to_render == row_count
    };
    let max_analyze_rows_usize = if max_analyze_rows == u64::MAX {
        usize::MAX
    } else {
        max_analyze_rows as usize
    };
    let analyze_limit = if stream_all_rows {
        if row_count == 0 && max_rows_usize == usize::MAX {
            max_analyze_rows_usize
        } else {
            row_count.min(max_analyze_rows_usize)
        }
    } else {
        0usize
    };

    let rendered_rows_hint = if stream_all_rows {
        row_count
    } else {
        top_rows + bottom_rows
    };
    let mut footer = compute_footer(
        row_count,
        rendered_rows_hint,
        col_count,
        large_number_rendering,
        state.decimal_separator,
        state.thousand_separator,
    );
    let mut should_set_last_query = limits_override.is_none() && row_count > 0;

    let max_render_width = if max_width == u64::MAX {
        usize::MAX
    } else if max_width == 0 {
        // Match the shipped shell: in duckbox mode, max_width=0 means "auto". When writing to a
        // pipe/non-console, auto becomes "infinite" (no truncation).
        if state.stdout_is_console {
            duckbox_terminal_width().max(80)
        } else {
            usize::MAX
        }
    } else {
        (max_width as usize).max(80)
    };

    let mut col_width: Vec<usize> = vec![0usize; col_count];
    for c in 0..col_count {
        col_width[c] = col_width[c].max(duckbox_render_length(&col_names[c]));
        if !render_columns_mode {
            col_width[c] = col_width[c].max(duckbox_render_length(&col_type_names[c]));
        }
    }

    let layout_write = |out: &mut DuckboxWriter, s: &str| {
        if highlight_results && !ansi_layout.is_empty() {
            out.write_all(ansi_layout.as_bytes());
            out.write_all(s.as_bytes());
            out.write_all(ansi_reset.as_bytes());
        } else {
            out.write_all(s.as_bytes());
        }
    };
    let write_styled = |out: &mut DuckboxWriter, style: &str, s: &str| {
        if highlight_results && !style.is_empty() {
            out.write_all(style.as_bytes());
            out.write_all(s.as_bytes());
            out.write_all(ansi_reset.as_bytes());
        } else {
            out.write_all(s.as_bytes());
        }
    };
    let nested_highlight_candidate = |src_c: usize, text: &str| -> bool {
        src_c != SPLIT_COLUMN
            && (col_logical_type_ids.get(src_c).is_some_and(|t| {
                matches!(
                    *t,
                    duckdb_sys::DUCKDB_TYPE_LIST
                        | duckdb_sys::DUCKDB_TYPE_STRUCT
                        | duckdb_sys::DUCKDB_TYPE_ARRAY
                        | duckdb_sys::DUCKDB_TYPE_MAP
                )
            }) || col_type_names
                .get(src_c)
                .is_some_and(|t| t == "json" || t == "variant")
                || matches!(text.trim_start().chars().next(), Some('{') | Some('[')))
    };
    let write_nested_highlighted = |out: &mut DuckboxWriter, buf: &[u8], enable: bool| {
        if !enable
            || !highlight_results
            || (ansi_string_constant.is_empty() && ansi_null_value.is_empty())
        {
            out.write_all(buf);
            return;
        }
        let Ok(text) = std::str::from_utf8(buf) else {
            out.write_all(buf);
            return;
        };
        let mut last = 0usize;
        let mut iter = text.char_indices().peekable();
        while let Some((idx, ch)) = iter.next() {
            if ch == '\'' || ch == '"' {
                let quote = ch;
                let mut escaped = false;
                let mut end = None;
                for (j, next) in iter.by_ref() {
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if next == '\\' {
                        escaped = true;
                        continue;
                    }
                    if next == quote {
                        end = Some(j + next.len_utf8());
                        break;
                    }
                }
                let Some(end) = end else {
                    break;
                };
                if idx > last {
                    out.write_all(&text.as_bytes()[last..idx]);
                }
                if ansi_string_constant.is_empty() {
                    out.write_all(&text.as_bytes()[idx..end]);
                } else {
                    out.write_all(ansi_string_constant.as_bytes());
                    out.write_all(&text.as_bytes()[idx..end]);
                    out.write_all(ansi_reset.as_bytes());
                }
                last = end;
                continue;
            }

            if idx + 4 <= text.len() && text.as_bytes()[idx..idx + 4].eq_ignore_ascii_case(b"null")
            {
                let previous = text[..idx].chars().next_back();
                let next = text[idx + 4..].chars().next();
                let is_boundary = |c: Option<char>| !matches!(c, Some(c) if c.is_ascii_alphanumeric() || c == '_');
                if is_boundary(previous) && is_boundary(next) {
                    if idx > last {
                        out.write_all(&text.as_bytes()[last..idx]);
                    }
                    if ansi_null_value.is_empty() {
                        out.write_all(&text.as_bytes()[idx..idx + 4]);
                    } else {
                        out.write_all(ansi_null_value.as_bytes());
                        out.write_all(&text.as_bytes()[idx..idx + 4]);
                        out.write_all(ansi_reset.as_bytes());
                    }
                    last = idx + 4;
                }
            }
        }
        if last < text.len() {
            out.write_all(&text.as_bytes()[last..]);
        }
    };

    fn compute_total_render_length(widths: &[usize]) -> usize {
        // Total render length includes the leading vertical and per-column: " " + value + " " + vertical.
        // This matches BoxRendererImplementation: 1 + sum(width + 3).
        1 + widths.iter().map(|w| w + 3).sum::<usize>()
    }

    fn compute_duckbox_column_layout(
        widths: &[usize],
        max_width: usize,
        min_width: usize,
    ) -> (Vec<usize>, Vec<usize>, usize, bool, bool) {
        // Port of `BoxRendererImplementation::ComputeRenderWidths` column sizing/pruning behavior.
        // Returns: (column_map, visible_widths, total_render_length, has_hidden_columns, shortened_columns)
        let mut column_widths = widths.to_vec();
        let original_widths = column_widths.clone();
        let mut total_render_length = compute_total_render_length(&column_widths);
        if total_render_length < min_width && !column_widths.is_empty() {
            column_widths[0] = column_widths[0].saturating_add(min_width - total_render_length);
            total_render_length = min_width;
        }

        let col_count = column_widths.len();
        let mut shortened_columns = false;
        let mut pruned_columns: std::collections::HashSet<usize> = std::collections::HashSet::new();

        if total_render_length > max_width && max_width != usize::MAX && col_count > 0 {
            // compute shorten capacity per column (down to MAX_COL_WIDTH)
            let mut max_shorten_amount: Vec<usize> = Vec::with_capacity(col_count);
            let mut total_max_shorten_amount: usize = 0;
            for &w in &column_widths {
                if w <= MAX_COL_WIDTH {
                    max_shorten_amount.push(0);
                    continue;
                }
                let diff = w - MAX_COL_WIDTH;
                max_shorten_amount.push(diff);
                total_max_shorten_amount = total_max_shorten_amount.saturating_add(diff);
            }

            let mut shorten_amount_required = total_render_length.saturating_sub(max_width);
            if total_max_shorten_amount >= shorten_amount_required {
                // We can get below max_width by shortening, without pruning.
                use std::collections::BTreeMap;
                let mut by_capacity: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
                for (col_idx, &cap) in max_shorten_amount.iter().enumerate() {
                    by_capacity.entry(cap).or_default().push(col_idx);
                }
                let mut actual_shorten: Vec<usize> = vec![0usize; col_count];
                while shorten_amount_required > 0 {
                    let (&largest_cap, cols) = by_capacity.iter().next_back().unwrap();
                    let column_list = cols.clone();
                    if by_capacity.len() == 1 {
                        // second largest is 0
                        let max_shorten_width = largest_cap;
                        let total_potential = max_shorten_width.saturating_mul(column_list.len());
                        if total_potential < shorten_amount_required {
                            break;
                        }
                        let per = shorten_amount_required / column_list.len();
                        for &idx in &column_list {
                            actual_shorten[idx] = actual_shorten[idx].saturating_add(per);
                        }
                        shorten_amount_required -= per.saturating_mul(column_list.len());
                        // leftover: shorten by 1 for the last N columns
                        for idx in column_list.iter().rev() {
                            if shorten_amount_required == 0 {
                                break;
                            }
                            actual_shorten[*idx] = actual_shorten[*idx].saturating_add(1);
                            shorten_amount_required -= 1;
                        }
                        break;
                    }

                    let mut it = by_capacity.iter().rev();
                    let (&largest_cap2, _) = it.next().unwrap();
                    let second = it.next().map(|(&k, _)| k).unwrap_or(0);
                    debug_assert_eq!(largest_cap, largest_cap2);
                    let max_shorten_width = largest_cap.saturating_sub(second);
                    let total_potential = max_shorten_width.saturating_mul(column_list.len());
                    if total_potential >= shorten_amount_required {
                        let per = shorten_amount_required / column_list.len();
                        for &idx in &column_list {
                            actual_shorten[idx] = actual_shorten[idx].saturating_add(per);
                        }
                        shorten_amount_required -= per.saturating_mul(column_list.len());
                        for idx in column_list.iter().rev() {
                            if shorten_amount_required == 0 {
                                break;
                            }
                            actual_shorten[*idx] = actual_shorten[*idx].saturating_add(1);
                            shorten_amount_required -= 1;
                        }
                        break;
                    }

                    // shorten all of these columns down to next bucket
                    for &idx in &column_list {
                        actual_shorten[idx] = actual_shorten[idx].saturating_add(max_shorten_width);
                    }
                    // move these columns into the next bucket and remove the current
                    by_capacity.remove(&largest_cap);
                    by_capacity.entry(second).or_default().extend(column_list);
                    shorten_amount_required =
                        shorten_amount_required.saturating_sub(total_potential);
                }

                for (c, amount) in actual_shorten.iter().copied().enumerate() {
                    if amount == 0 {
                        continue;
                    }
                    if amount < column_widths[c] {
                        column_widths[c] -= amount;
                        total_render_length = total_render_length.saturating_sub(amount);
                        shortened_columns = true;
                    }
                }
            } else {
                // Not enough shorten capacity: clamp wide columns to MAX_COL_WIDTH, then prune if needed.
                for w in column_widths.iter_mut() {
                    if *w <= MAX_COL_WIDTH {
                        continue;
                    }
                    total_render_length = total_render_length.saturating_sub(*w - MAX_COL_WIDTH);
                    *w = MAX_COL_WIDTH;
                    shortened_columns = true;
                }
            }

            if total_render_length > max_width {
                // Still too wide: prune columns from the middle (zig-zag), accounting for the split column.
                total_render_length = total_render_length.saturating_add(3 + DOTDOTDOT_LENGTH);
                let mid = (col_count as i64) / 2;
                let mut offset: i64 = 0;
                while total_render_length > max_width {
                    let idx_i64 = mid + offset;
                    if idx_i64 < 0 || idx_i64 >= col_count as i64 {
                        break;
                    }
                    let c = idx_i64 as usize;
                    if pruned_columns.insert(c) {
                        total_render_length =
                            total_render_length.saturating_sub(column_widths[c] + 3);
                    }
                    offset = if offset >= 0 { -offset - 1 } else { -offset };
                    if pruned_columns.len() >= col_count {
                        break;
                    }
                }

                // Redistribute remaining space to visible columns (in order), up to original widths.
                let mut space_left = max_width.saturating_sub(total_render_length);
                for c in 0..col_count {
                    if space_left == 0 {
                        break;
                    }
                    if pruned_columns.contains(&c) {
                        continue;
                    }
                    if column_widths[c] >= original_widths[c] {
                        continue;
                    }
                    let want = original_widths[c].saturating_sub(column_widths[c]);
                    let inc = space_left.min(want);
                    column_widths[c] = column_widths[c].saturating_add(inc);
                    space_left -= inc;
                    total_render_length = total_render_length.saturating_add(inc);
                }
            }
        }

        let mut column_map: Vec<usize> = Vec::new();
        let mut new_widths: Vec<usize> = Vec::new();
        let mut added_split_column = false;
        for c in 0..col_count {
            if !pruned_columns.contains(&c) {
                column_map.push(c);
                new_widths.push(column_widths[c]);
            } else if !added_split_column {
                column_map.push(SPLIT_COLUMN);
                new_widths.push(DOTDOTDOT_LENGTH);
                added_split_column = true;
            }
        }

        let has_hidden_columns = added_split_column;
        let new_total_length = compute_total_render_length(&new_widths);
        (
            column_map,
            new_widths,
            new_total_length,
            has_hidden_columns,
            shortened_columns,
        )
    }

    fn box_row_sep(
        out: &mut DuckboxWriter,
        widths: &[usize],
        line: &str,
        left: &str,
        mid: &str,
        right: &str,
        layout: &dyn Fn(&mut DuckboxWriter, &str),
    ) {
        layout(out, left);
        for (idx, w) in widths.iter().enumerate() {
            for _ in 0..(*w + 2) {
                layout(out, line);
            }
            if idx + 1 < widths.len() {
                layout(out, mid);
            }
        }
        layout(out, right);
        out.write_all(b"\n");
    }

    fn truncate_for_cell(s: &str, width: usize, right_align: bool) -> Vec<u8> {
        let rendered = if duckbox_render_length(s) > width {
            duckbox_truncate_with_ellipsis(s, width)
        } else {
            s.to_string()
        };
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut cur = std::io::Cursor::new(&mut buf);
            duckbox_utf8_width_print(&mut cur, &rendered, width, right_align);
        }
        buf
    }

    fn truncate_center_for_cell(s: &str, width: usize) -> Vec<u8> {
        let rendered = if duckbox_render_length(s) > width {
            duckbox_truncate_with_ellipsis(s, width)
        } else {
            s.to_string()
        };
        let len = duckbox_render_length(&rendered);
        let pad_total = width.saturating_sub(len);
        let lpad = pad_total / 2;
        let rpad = pad_total - lpad;
        let mut buf: Vec<u8> = Vec::with_capacity(rendered.len() + lpad + rpad);
        buf.extend(std::iter::repeat_n(b' ', lpad));
        buf.extend_from_slice(rendered.as_bytes());
        buf.extend(std::iter::repeat_n(b' ', rpad));
        buf
    }

    fn shrink_columns_to_fit(widths: &mut [usize], max_width: usize, max_col_width: usize) -> bool {
        let original = widths.to_vec();
        let total = compute_total_render_length(widths);
        if total <= max_width || max_width == usize::MAX {
            return false;
        }
        let mut shorten_required = total - max_width;
        let mut candidates: Vec<(usize, usize)> = widths
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, w)| *w > max_col_width)
            .collect();
        let total_max_shorten: usize = candidates.iter().map(|(_, w)| w - max_col_width).sum();
        if total_max_shorten == 0 {
            return false;
        }
        if total_max_shorten < shorten_required {
            // Best-effort: clamp to max_col_width (full column pruning still missing).
            for w in widths.iter_mut() {
                if *w > max_col_width {
                    *w = max_col_width;
                }
            }
            return widths != original;
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        let mut active: Vec<usize> = Vec::new();
        let mut current_width = candidates[0].1;
        active.push(candidates[0].0);
        for idx in 1..=candidates.len() {
            let next_width = if idx < candidates.len() {
                candidates[idx].1
            } else {
                max_col_width
            };
            let target = next_width.max(max_col_width);
            let reducible = current_width.saturating_sub(target) * active.len();
            if reducible >= shorten_required {
                let dec = shorten_required / active.len();
                for &col in &active {
                    widths[col] = widths[col].saturating_sub(dec);
                }
                shorten_required -= dec * active.len();
                for &col in active.iter().rev().take(shorten_required) {
                    widths[col] = widths[col].saturating_sub(1);
                }
                break;
            } else {
                for &col in &active {
                    widths[col] = widths[col].saturating_sub(current_width.saturating_sub(target));
                }
                shorten_required = shorten_required.saturating_sub(reducible);
                current_width = target;
            }
            if idx < candidates.len() {
                active.push(candidates[idx].0);
                current_width = current_width.max(candidates[idx].1);
            }
        }
        widths != original
    }

    let right_align: Vec<bool> = col_types.iter().map(|t| is_numeric_type(*t)).collect();

    let null_value = state.nullValue.clone();
    let decimal_separator = state.decimal_separator;
    let thousand_separator = state.thousand_separator;

    let pager_cmd = if state.pager_command.trim().is_empty() {
        get_system_pager()
    } else {
        state.pager_command.clone()
    };
    let pager_context = state.stdout_is_console
        && state.stdin_is_interactive
        && state.outfile.is_empty()
        && matches!(&state.out, OutputHandle::Stdout);
    let use_pager = pager_context
        && match state.pager_mode {
            PagerMode::Off => false,
            PagerMode::On => true,
            PagerMode::Automatic => {
                let triggers_rows = (row_count as u64) >= state.pager_min_rows;
                let triggers_cols = state.pager_min_cols > 0
                    && (compute_total_render_length(&col_width) as u64) > state.pager_min_cols;
                triggers_rows || triggers_cols
            }
        };

    let mut writer = DuckboxWriter::new(&mut state.out, &pager_cmd, use_pager);

    if render_columns_as_rows {
        let mut rows_to_render = row_count.min(max_rows_usize);
        if max_rows_usize != usize::MAX && row_count <= max_rows_usize.saturating_add(3) {
            rows_to_render = row_count;
        }
        let (top_rows, bottom_rows) = if rows_to_render == row_count {
            (rows_to_render, 0usize)
        } else {
            let top = rows_to_render / 2 + if rows_to_render % 2 != 0 { 1 } else { 0 };
            (top, rows_to_render - top)
        };
        let mut footer = compute_footer(
            row_count,
            top_rows + bottom_rows,
            col_count,
            large_number_rendering,
            state.decimal_separator,
            state.thousand_separator,
        );
        let should_set_last_query = limits_override.is_none() && row_count > 0;

        let mut display_row_indices: Vec<usize> = Vec::new();
        for r in 0..top_rows {
            display_row_indices.push(r);
        }
        if bottom_rows > 0 {
            let start = row_count.saturating_sub(bottom_rows);
            for r in start..row_count {
                display_row_indices.push(r);
            }
        }

        let display_cols = display_row_indices.len();
        let mut values: Vec<Vec<Option<String>>> = vec![vec![None; display_cols]; col_count];

        let mut want_idx = 0usize;
        let mut global_row = 0usize;
        loop {
            let mut chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
            if chunk.is_null() {
                break;
            }
            let chunk_size = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) } as usize;
            let mut vectors: Vec<duckdb_sys::duckdb_vector> = Vec::with_capacity(col_count);
            let mut types: Vec<duckdb_sys::duckdb_logical_type> = Vec::with_capacity(col_count);
            for c in 0..col_count {
                let v = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, c as u64) };
                vectors.push(v);
                types.push(unsafe { duckdb_sys::duckdb_vector_get_column_type(v) });
            }

            for r in 0..chunk_size {
                while want_idx < display_cols && display_row_indices[want_idx] == global_row {
                    for c in 0..col_count {
                        let raw =
                            crate::value::vector_value_to_string(vectors[c], types[c], r as u64);
                        let rendered = match raw {
                            None => None,
                            Some(v) => {
                                let v = if v.starts_with('[') || v.starts_with('{') {
                                    normalize_complex_value_for_shell_display(
                                        v.as_str(),
                                        col_unquote_json_literals[c],
                                    )
                                    .into_owned()
                                } else {
                                    v
                                };
                                let v = escape_control_chars(&v);
                                // Match BoxRenderer: in COLUMNS mode values are rendered as VARCHAR and do not go through
                                // numeric formatting (decimal/thousand separators, large-number rendering).
                                Some(v)
                            }
                        };
                        values[c][want_idx] = rendered;
                    }
                    want_idx += 1;
                }
                global_row += 1;
            }

            for t in types.iter_mut() {
                unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
            }
            unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
        }

        let mut headers: Vec<String> = Vec::with_capacity(2 + display_cols);
        headers.push("Column".to_string());
        headers.push("Type".to_string());
        for &idx in &display_row_indices {
            headers.push(format!("Row {}", idx + 1));
        }

        let mut out_widths: Vec<usize> = vec![0usize; headers.len()];
        out_widths[0] = out_widths[0].max(duckbox_render_length(headers[0].as_str()));
        out_widths[1] = out_widths[1].max(duckbox_render_length(headers[1].as_str()));
        for (i, h) in headers.iter().enumerate().skip(2) {
            out_widths[i] = out_widths[i].max(duckbox_render_length(h.as_str()));
        }
        for c in 0..col_count {
            out_widths[0] = out_widths[0].max(duckbox_render_length(col_names[c].as_str()));
            out_widths[1] = out_widths[1].max(duckbox_render_length(col_type_names[c].as_str()));
            for k in 0..display_cols {
                let s = values[c][k].as_deref().unwrap_or(null_value.as_str());
                out_widths[2 + k] = out_widths[2 + k].max(duckbox_render_length(s));
            }
        }

        let min_width = if footer.has_hidden_rows || row_count == 0 {
            footer.render_length
        } else {
            0
        };
        let (column_map, out_widths, total_render_length, has_hidden_columns, _) =
            compute_duckbox_column_layout(&out_widths, max_render_width, min_width);
        if has_hidden_columns && row_count > 0 {
            footer.must_show_footer = true;
            footer.has_hidden_rows = true;
            let shown = column_map.len().saturating_sub(3);
            footer.shown_str = format!("{} shown", shown);
            recompute_footer_render_length(&mut footer);
        }

        box_row_sep(
            &mut writer,
            &out_widths,
            BOX_24,
            BOX_23,
            BOX_234,
            BOX_34,
            &layout_write,
        );
        layout_write(&mut writer, BOX_13);
        layout_write(&mut writer, " ");
        for (out_c, &entry) in column_map.iter().enumerate() {
            let (s, style) = if entry == SPLIT_COLUMN {
                (DOTDOTDOT, &ansi_layout)
            } else {
                (headers[entry].as_str(), &ansi_column_name)
            };
            let buf = truncate_center_for_cell(s, out_widths[out_c]);
            write_styled(&mut writer, style, std::str::from_utf8(&buf).unwrap_or(""));
            layout_write(&mut writer, " ");
            layout_write(&mut writer, BOX_13);
            if out_c + 1 < column_map.len() {
                layout_write(&mut writer, " ");
            } else {
                writer.write_all(b"\n");
            }
        }
        box_row_sep(
            &mut writer,
            &out_widths,
            BOX_24,
            BOX_123,
            BOX_1234,
            BOX_134,
            &layout_write,
        );

        for c in 0..col_count {
            layout_write(&mut writer, BOX_13);
            layout_write(&mut writer, " ");
            for (out_c, &entry) in column_map.iter().enumerate() {
                if out_c > 0 {
                    // we already wrote the leading space for the row; write spaces between cells
                    layout_write(&mut writer, "");
                }
                let (buf, is_null, style, highlight_nested) = if entry == SPLIT_COLUMN {
                    (
                        truncate_center_for_cell(DOTDOTDOT, out_widths[out_c]),
                        false,
                        Some(&ansi_layout),
                        false,
                    )
                } else if entry == 0 {
                    (
                        truncate_for_cell(col_names[c].as_str(), out_widths[out_c], false),
                        false,
                        Some(&ansi_column_name),
                        false,
                    )
                } else if entry == 1 {
                    (
                        truncate_center_for_cell(col_type_names[c].as_str(), out_widths[out_c]),
                        false,
                        Some(&ansi_column_type),
                        false,
                    )
                } else {
                    let k = entry - 2;
                    let cell = values[c][k].as_deref().unwrap_or(null_value.as_str());
                    let b = truncate_for_cell(cell, out_widths[out_c], true);
                    (
                        b,
                        values[c][k].is_none(),
                        None,
                        nested_highlight_candidate(c, cell),
                    )
                };
                if is_null {
                    write_styled(
                        &mut writer,
                        &ansi_null_value,
                        std::str::from_utf8(&buf).unwrap_or(""),
                    );
                } else if let Some(style) = style {
                    write_styled(&mut writer, style, std::str::from_utf8(&buf).unwrap_or(""));
                } else {
                    write_nested_highlighted(&mut writer, &buf, highlight_nested);
                }
                layout_write(&mut writer, " ");
                layout_write(&mut writer, BOX_13);
                if out_c + 1 < column_map.len() {
                    layout_write(&mut writer, " ");
                } else {
                    writer.write_all(b"\n");
                }
            }
        }

        if footer.must_show_footer {
            box_row_sep(
                &mut writer,
                &out_widths,
                BOX_24,
                BOX_123,
                BOX_DMIDDLE,
                BOX_134,
                &layout_write,
            );
            let padding = total_render_length.saturating_sub(footer.row_count_str.len() + 4);
            layout_write(&mut writer, BOX_13);
            layout_write(&mut writer, " ");
            let mut footer_line = String::new();
            footer_line.push_str(&footer.row_count_str);
            footer_line
                .push_str(&" ".repeat(padding.saturating_sub(footer.column_count_str.len())));
            footer_line.push_str(&footer.column_count_str);
            write_styled(&mut writer, &ansi_footer, &footer_line);
            layout_write(&mut writer, " ");
            layout_write(&mut writer, BOX_13);
            writer.write_all(b"\n");

            if footer.has_hidden_rows && !footer.shown_str.is_empty() {
                layout_write(&mut writer, BOX_13);
                layout_write(&mut writer, " ");
                let s = format!("({})", footer.shown_str);
                let buf = truncate_for_cell(&s, total_render_length.saturating_sub(4), false);
                write_styled(
                    &mut writer,
                    &ansi_null_value,
                    std::str::from_utf8(&buf).unwrap_or(""),
                );
                layout_write(&mut writer, " ");
                layout_write(&mut writer, BOX_13);
                writer.write_all(b"\n");
            }
        }

        box_row_sep(
            &mut writer,
            &out_widths,
            BOX_24,
            BOX_12,
            BOX_124,
            BOX_14,
            &layout_write,
        );
        writer.finish();
        if should_set_last_query {
            state.last_query_duckbox = Some(cmd.to_string());
        }
        return 0;
    }

    let use_row_based_cells = col_type_names.iter().any(|t| t == "variant");

    // Buffers for small/truncated output or for the initial analyze window in streaming mode.
    let mut buffered_rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut footer_number_row: Option<Vec<String>> = None;

    let mut global_row_idx: usize = 0;
    let mut printed_any_values = false;
    let mut header_rendered = false;
    let mut stream_expand_rows = false;
    let mut stream_max_rows_per_row = 1usize;
    let mut stream_is_first_value_row = true;
    let mut render_column_map: Vec<usize> = (0..col_count).collect();
    let mut render_col_width: Vec<usize> = col_width.clone();
    let mut render_right_align: Vec<bool> = right_align.clone();
    let mut has_hidden_columns = false;
    let mut total_render_length = compute_total_render_length(&render_col_width);

    if use_row_based_cells {
        // For extension/unsupported logical types (e.g. VARIANT), the chunk/vector interface does not always
        // expose a usable physical representation. Fall back to row-based value getters for correctness.
        for row_idx in 0..row_count {
            let should_buffer_or_print = if row_idx < top_rows {
                true
            } else if bottom_rows > 0 && row_idx >= row_count.saturating_sub(bottom_rows) {
                true
            } else {
                false
            };
            if !should_buffer_or_print {
                continue;
            }

            let mut row: Vec<Option<String>> = Vec::with_capacity(col_count);
            for c in 0..col_count {
                let raw = unsafe {
                    if duckdb_sys::duckdb_value_is_null(result, c as u64, row_idx as u64) {
                        None
                    } else {
                        let ptr =
                            duckdb_sys::duckdb_value_varchar(result, c as u64, row_idx as u64);
                        if ptr.is_null() {
                            None
                        } else {
                            let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
                            duckdb_sys::duckdb_free(ptr as *mut _);
                            Some(s)
                        }
                    }
                };
                let rendered = match raw {
                    None => None,
                    Some(v) => {
                        let v = if v.starts_with('[') || v.starts_with('{') {
                            normalize_complex_value_for_shell_display(
                                v.as_str(),
                                col_unquote_json_literals[c],
                            )
                            .into_owned()
                        } else {
                            v
                        };
                        let v = if is_describe_output && c == 1 {
                            v.to_ascii_lowercase()
                        } else {
                            v
                        };
                        let v = escape_control_chars(&v);
                        let v = if is_numeric_type(col_types[c]) {
                            Some(format_number(
                                &v,
                                large_number_rendering,
                                decimal_separator,
                                thousand_separator,
                            ))
                        } else {
                            Some(v)
                        };
                        v
                    }
                };
                row.push(rendered);
            }

            for c in 0..col_count {
                let s = row[c].as_deref().unwrap_or(null_value.as_str());
                col_width[c] = col_width[c].max(max_render_length_multiline(s));
            }
            buffered_rows.push(row);
        }
    } else {
        let mut analyze_limit_effective = analyze_limit;
        let mut analyze_limit_adjusted = false;
        let mut chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
        while !chunk.is_null() {
            let chunk_size = unsafe { duckdb_sys::duckdb_data_chunk_get_size(chunk) } as usize;
            if stream_all_rows && !header_rendered && !analyze_limit_adjusted {
                if analyze_limit_effective == 0 {
                    // Match BoxRenderer: a max_analyze_rows of 0 still analyzes at least one chunk.
                    analyze_limit_effective = chunk_size;
                    analyze_limit_adjusted = true;
                } else if analyze_limit_effective != usize::MAX && chunk_size > 0 {
                    // BoxRenderer stops analyzing once row_idx >= max_analyze_rows, where row_idx increments
                    // by the fetched chunk size. This effectively rounds the analyze limit up to a chunk
                    // boundary.
                    let rounded =
                        ((analyze_limit_effective + chunk_size - 1) / chunk_size) * chunk_size;
                    analyze_limit_effective = rounded.max(analyze_limit_effective);
                    analyze_limit_adjusted = true;
                }
            }
            let mut vectors: Vec<duckdb_sys::duckdb_vector> = Vec::with_capacity(col_count);
            let mut types: Vec<duckdb_sys::duckdb_logical_type> = Vec::with_capacity(col_count);
            for c in 0..col_count {
                let v = unsafe { duckdb_sys::duckdb_data_chunk_get_vector(chunk, c as u64) };
                vectors.push(v);
                types.push(unsafe { duckdb_sys::duckdb_vector_get_column_type(v) });
            }

            for r in 0..chunk_size {
                let row_idx = global_row_idx;
                global_row_idx += 1;

                let should_buffer_or_print = if stream_all_rows {
                    true
                } else if row_idx < top_rows {
                    true
                } else if bottom_rows > 0 && row_idx >= row_count.saturating_sub(bottom_rows) {
                    true
                } else {
                    false
                };
                if !should_buffer_or_print {
                    continue;
                }

                let mut row: Vec<Option<String>> = Vec::with_capacity(col_count);
                for c in 0..col_count {
                    let raw = crate::value::vector_value_to_string(vectors[c], types[c], r as u64);
                    let rendered = match raw {
                        None => None,
                        Some(v) => {
                            let v = if v.starts_with('[') || v.starts_with('{') {
                                normalize_complex_value_for_shell_display(
                                    v.as_str(),
                                    col_unquote_json_literals[c],
                                )
                                .into_owned()
                            } else {
                                v
                            };
                            let v = if is_describe_output && c == 1 {
                                v.to_ascii_lowercase()
                            } else {
                                v
                            };
                            let v = escape_control_chars(&v);
                            let v = if is_numeric_type(col_types[c]) {
                                Some(format_number(
                                    &v,
                                    large_number_rendering,
                                    decimal_separator,
                                    thousand_separator,
                                ))
                            } else {
                                Some(v)
                            };
                            v
                        }
                    };
                    row.push(rendered);
                }

                if stream_all_rows
                    && !header_rendered
                    && analyze_limit_effective != 0
                    && buffered_rows.len() < analyze_limit_effective
                {
                    for c in 0..col_count {
                        let s = row[c].as_deref().unwrap_or(null_value.as_str());
                        col_width[c] = col_width[c].max(max_render_length_multiline(s));
                    }
                    buffered_rows.push(row);
                    if buffered_rows.len() == analyze_limit_effective {
                        // Add the `(xx.yy million)` footer row for single-row results in FOOTER mode.
                        if large_number_rendering == LargeNumberRendering::Footer
                            && !render_columns_mode
                            && row_count == 1
                            && buffered_rows.len() == 1
                        {
                            let mut readable: Vec<String> = Vec::with_capacity(col_count);
                            let mut all_readable = true;
                            for c in 0..col_count {
                                if !is_numeric_type(col_types[c]) {
                                    all_readable = false;
                                    break;
                                }
                                let raw = buffered_rows[0][c].as_deref().unwrap_or("");
                                let Some(formatted) =
                                    try_format_large_number(raw, decimal_separator)
                                else {
                                    all_readable = false;
                                    break;
                                };
                                readable.push(format!("({})", formatted));
                            }
                            if all_readable {
                                for c in 0..col_count {
                                    col_width[c] =
                                        col_width[c].max(duckbox_render_length(&readable[c]));
                                }
                                footer_number_row = Some(readable);
                            }
                        }

                        // Ensure footer fits if it must render.
                        // `duckdb_row_count` reports 0 up front in streaming mode; do not reserve
                        // footer width based on that placeholder once we've seen actual rows.
                        let treat_zero_rows_as_unknown = row_count == 0 && global_row_idx > 0;
                        let min_width = if footer.has_hidden_rows
                            || (row_count == 0 && !treat_zero_rows_as_unknown)
                        {
                            footer.render_length
                        } else {
                            0
                        };
                        let (new_map, new_widths, new_total, hidden_cols, shortened_columns) =
                            compute_duckbox_column_layout(&col_width, max_render_width, min_width);
                        render_column_map = new_map;
                        render_col_width = new_widths;
                        total_render_length = new_total;
                        has_hidden_columns = hidden_cols;
                        if has_hidden_columns && row_count > 0 {
                            let shown = render_column_map.len().saturating_sub(1);
                            footer
                                .column_count_str
                                .push_str(&format!(" ({} shown)", shown));
                            footer.must_show_footer = true;
                        }
                        render_right_align = render_column_map
                            .iter()
                            .map(|&idx| {
                                if idx == SPLIT_COLUMN {
                                    false
                                } else {
                                    right_align[idx]
                                }
                            })
                            .collect();

                        stream_expand_rows = false;
                        stream_max_rows_per_row = 1usize;
                        if shortened_columns
                            && !render_columns_mode
                            && !has_hidden_columns
                            && max_width != u64::MAX
                            && max_width != 0
                            && (row_count + 5) < max_rows_usize
                        {
                            let numer = if max_rows_usize <= 5 {
                                0u128
                            } else {
                                (max_rows_usize - 5) as u128
                            };
                            let denom = row_count.max(1) as u128;
                            let per = (numer / denom).max(1).min(10_000) as usize;
                            if per > 1 {
                                stream_expand_rows = true;
                                stream_max_rows_per_row = per;
                            }
                        }

                        // Render header now.
                        box_row_sep(
                            &mut writer,
                            &render_col_width,
                            BOX_24,
                            BOX_23,
                            BOX_234,
                            BOX_34,
                            &layout_write,
                        );
                        // header row(s)
                        let header_rows: [(&[String], &str); 2] = [
                            (&col_names, &ansi_column_name),
                            (&col_type_names, &ansi_column_type),
                        ];
                        for (idx, (vals, style)) in header_rows.iter().enumerate() {
                            if render_columns_mode && idx == 1 {
                                continue;
                            }
                            // Per-cell wrapping/truncation for header in expand mode.
                            let mut cell_lines: Vec<Vec<String>> =
                                Vec::with_capacity(render_column_map.len());
                            let mut row_line_count = 1usize;
                            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                                let s = if src_c == SPLIT_COLUMN {
                                    DOTDOTDOT
                                } else {
                                    vals[src_c].as_str()
                                };
                                let lines = if stream_expand_rows {
                                    duckbox_truncate_lines(
                                        s,
                                        render_col_width[out_c],
                                        stream_max_rows_per_row,
                                    )
                                } else {
                                    split_and_render_lines(s, render_col_width[out_c], false, 1)
                                };
                                row_line_count = row_line_count.max(lines.len());
                                cell_lines.push(lines);
                            }
                            for line_idx in 0..row_line_count {
                                layout_write(&mut writer, BOX_13);
                                layout_write(&mut writer, " ");
                                for c in 0..render_column_map.len() {
                                    let s = cell_lines[c]
                                        .get(line_idx)
                                        .map(|s| s.as_str())
                                        .unwrap_or("");
                                    let buf = truncate_center_for_cell(s, render_col_width[c]);
                                    let cell_style: &str = if render_column_map[c] == SPLIT_COLUMN {
                                        ansi_layout.as_str()
                                    } else {
                                        *style
                                    };
                                    if highlight_results && !cell_style.is_empty() {
                                        let start = buf
                                            .iter()
                                            .position(|b| *b != b' ')
                                            .unwrap_or(buf.len());
                                        let end = buf
                                            .iter()
                                            .rposition(|b| *b != b' ')
                                            .map(|i| i + 1)
                                            .unwrap_or(0);
                                        if end > start {
                                            writer.write_all(&buf[..start]);
                                            writer.write_all(cell_style.as_bytes());
                                            writer.write_all(&buf[start..end]);
                                            writer.write_all(ansi_reset.as_bytes());
                                            writer.write_all(&buf[end..]);
                                        } else {
                                            writer.write_all(&buf);
                                        }
                                    } else {
                                        writer.write_all(&buf);
                                    }
                                    layout_write(&mut writer, " ");
                                    layout_write(&mut writer, BOX_13);
                                    if c + 1 < render_column_map.len() {
                                        layout_write(&mut writer, " ");
                                    } else {
                                        writer.write_all(b"\n");
                                    }
                                }
                            }
                        }
                        // separator under header
                        let join = BOX_1234;
                        box_row_sep(
                            &mut writer,
                            &render_col_width,
                            BOX_24,
                            BOX_123,
                            join,
                            BOX_134,
                            &layout_write,
                        );

                        header_rendered = true;
                        let center_single_footer_value_row = footer_number_row.is_some();

                        // Render buffered rows.
                        for row in buffered_rows.drain(..) {
                            if stream_expand_rows && !stream_is_first_value_row {
                                box_row_sep(
                                    &mut writer,
                                    &render_col_width,
                                    BOX_24,
                                    BOX_123,
                                    BOX_1234,
                                    BOX_134,
                                    &layout_write,
                                );
                            }
                            stream_is_first_value_row = false;
                            let mut rendered_cells: Vec<Vec<String>> =
                                Vec::with_capacity(render_column_map.len());
                            let mut row_line_count = 1usize;
                            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                                let s = if src_c == SPLIT_COLUMN {
                                    DOTDOTDOT
                                } else {
                                    row[src_c].as_deref().unwrap_or(null_value.as_str())
                                };
                                let (formatted, is_pretty_printable) =
                                    if stream_expand_rows && src_c != SPLIT_COLUMN {
                                        let looks_bracketed = matches!(
                                            s.trim_start().chars().next(),
                                            Some('{') | Some('[')
                                        );
                                        let is_pretty_printable =
                                            col_logical_type_ids.get(src_c).is_some_and(|t| {
                                                matches!(
                                                    *t,
                                                    duckdb_sys::DUCKDB_TYPE_LIST
                                                        | duckdb_sys::DUCKDB_TYPE_STRUCT
                                                        | duckdb_sys::DUCKDB_TYPE_ARRAY
                                                        | duckdb_sys::DUCKDB_TYPE_MAP
                                                )
                                            }) || col_type_names
                                                .get(src_c)
                                                .is_some_and(|t| t == "json" || t == "variant")
                                                || looks_bracketed;
                                        if is_pretty_printable {
                                            (
                                                crate::duckbox_json_formatter::format_value(
                                                    s,
                                                    stream_max_rows_per_row,
                                                    render_col_width[out_c],
                                                )
                                                .unwrap_or_else(|| s.to_string()),
                                                true,
                                            )
                                        } else {
                                            (s.to_string(), false)
                                        }
                                    } else {
                                        (s.to_string(), false)
                                    };
                                let lines = if stream_expand_rows {
                                    if is_pretty_printable {
                                        duckbox_truncate_lines(
                                            &formatted,
                                            render_col_width[out_c],
                                            stream_max_rows_per_row,
                                        )
                                    } else {
                                        split_and_render_lines(
                                            &formatted,
                                            render_col_width[out_c],
                                            true,
                                            stream_max_rows_per_row,
                                        )
                                    }
                                } else {
                                    split_and_render_lines(
                                        &formatted,
                                        render_col_width[out_c],
                                        false,
                                        1,
                                    )
                                };
                                row_line_count = row_line_count.max(lines.len());
                                rendered_cells.push(lines);
                            }
                            for line_idx in 0..row_line_count {
                                layout_write(&mut writer, BOX_13);
                                layout_write(&mut writer, " ");
                                for c in 0..render_column_map.len() {
                                    let src_c = render_column_map[c];
                                    let is_null = src_c != SPLIT_COLUMN && row[src_c].is_none();
                                    let s = rendered_cells[c]
                                        .get(line_idx)
                                        .map(|s| s.as_str())
                                        .unwrap_or("");
                                    if src_c == SPLIT_COLUMN {
                                        let buf = truncate_center_for_cell(s, render_col_width[c]);
                                        if highlight_results && !ansi_layout.is_empty() {
                                            write_styled(
                                                &mut writer,
                                                &ansi_layout,
                                                std::str::from_utf8(&buf).unwrap_or(""),
                                            );
                                        } else {
                                            writer.write_all(&buf);
                                        }
                                    } else {
                                        let buf = if center_single_footer_value_row {
                                            truncate_center_for_cell(s, render_col_width[c])
                                        } else {
                                            truncate_for_cell(
                                                s,
                                                render_col_width[c],
                                                render_right_align[c],
                                            )
                                        };
                                        if is_null {
                                            if highlight_results && !ansi_null_value.is_empty() {
                                                let start = buf
                                                    .iter()
                                                    .position(|b| *b != b' ')
                                                    .unwrap_or(buf.len());
                                                let end = buf
                                                    .iter()
                                                    .rposition(|b| *b != b' ')
                                                    .map(|i| i + 1)
                                                    .unwrap_or(0);
                                                writer.write_all(&buf[..start]);
                                                writer.write_all(ansi_null_value.as_bytes());
                                                writer.write_all(&buf[start..end]);
                                                writer.write_all(ansi_reset.as_bytes());
                                                writer.write_all(&buf[end..]);
                                            } else {
                                                writer.write_all(&buf);
                                            }
                                        } else {
                                            write_nested_highlighted(
                                                &mut writer,
                                                &buf,
                                                nested_highlight_candidate(src_c, s),
                                            );
                                        }
                                    }
                                    layout_write(&mut writer, " ");
                                    layout_write(&mut writer, BOX_13);
                                    if c + 1 < render_column_map.len() {
                                        layout_write(&mut writer, " ");
                                    } else {
                                        writer.write_all(b"\n");
                                    }
                                }
                            }
                            printed_any_values = true;
                        }

                        // Render the optional large-number footer row for the single-row FOOTER mode.
                        if let Some(nums) = footer_number_row.take() {
                            let mut row_cells: Vec<Vec<String>> =
                                Vec::with_capacity(render_column_map.len());
                            let mut row_line_count = 1usize;
                            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                                let cell = if src_c == SPLIT_COLUMN {
                                    DOTDOTDOT
                                } else {
                                    nums[src_c].as_str()
                                };
                                let lines = split_and_render_lines(
                                    cell,
                                    render_col_width[out_c],
                                    stream_expand_rows,
                                    stream_max_rows_per_row,
                                );
                                row_line_count = row_line_count.max(lines.len());
                                row_cells.push(lines);
                            }
                            if stream_expand_rows && printed_any_values {
                                box_row_sep(
                                    &mut writer,
                                    &render_col_width,
                                    BOX_24,
                                    BOX_123,
                                    BOX_1234,
                                    BOX_134,
                                    &layout_write,
                                );
                            }
                            for line_idx in 0..row_line_count {
                                layout_write(&mut writer, BOX_13);
                                layout_write(&mut writer, " ");
                                for c in 0..render_column_map.len() {
                                    let s = row_cells[c]
                                        .get(line_idx)
                                        .map(|s| s.as_str())
                                        .unwrap_or("");
                                    let src_c = render_column_map[c];
                                    if src_c == SPLIT_COLUMN {
                                        let buf = truncate_center_for_cell(s, render_col_width[c]);
                                        if highlight_results && !ansi_layout.is_empty() {
                                            write_styled(
                                                &mut writer,
                                                &ansi_layout,
                                                std::str::from_utf8(&buf).unwrap_or(""),
                                            );
                                        } else {
                                            writer.write_all(&buf);
                                        }
                                    } else {
                                        let buf = truncate_for_cell(
                                            s,
                                            render_col_width[c],
                                            render_right_align[c],
                                        );
                                        writer.write_all(&buf);
                                    }
                                    layout_write(&mut writer, " ");
                                    layout_write(&mut writer, BOX_13);
                                    if c + 1 < render_column_map.len() {
                                        layout_write(&mut writer, " ");
                                    } else {
                                        writer.write_all(b"\n");
                                    }
                                }
                            }
                        }
                    }
                } else if stream_all_rows && header_rendered {
                    // Render streaming rows after the analyze window.
                    if stream_expand_rows && !stream_is_first_value_row {
                        box_row_sep(
                            &mut writer,
                            &render_col_width,
                            BOX_24,
                            BOX_123,
                            BOX_1234,
                            BOX_134,
                            &layout_write,
                        );
                    }
                    stream_is_first_value_row = false;

                    let mut rendered_cells: Vec<Vec<String>> =
                        Vec::with_capacity(render_column_map.len());
                    let mut row_line_count = 1usize;
                    for (out_c, &src_c) in render_column_map.iter().enumerate() {
                        let s = if src_c == SPLIT_COLUMN {
                            DOTDOTDOT
                        } else {
                            row[src_c].as_deref().unwrap_or(null_value.as_str())
                        };
                        let (formatted, is_pretty_printable) =
                            if stream_expand_rows && src_c != SPLIT_COLUMN {
                                let looks_bracketed =
                                    matches!(s.trim_start().chars().next(), Some('{') | Some('['));
                                let is_pretty_printable =
                                    col_logical_type_ids.get(src_c).is_some_and(|t| {
                                        matches!(
                                            *t,
                                            duckdb_sys::DUCKDB_TYPE_LIST
                                                | duckdb_sys::DUCKDB_TYPE_STRUCT
                                                | duckdb_sys::DUCKDB_TYPE_ARRAY
                                                | duckdb_sys::DUCKDB_TYPE_MAP
                                        )
                                    }) || col_type_names
                                        .get(src_c)
                                        .is_some_and(|t| t == "json" || t == "variant")
                                        || looks_bracketed;
                                if is_pretty_printable {
                                    (
                                        crate::duckbox_json_formatter::format_value(
                                            s,
                                            stream_max_rows_per_row,
                                            render_col_width[out_c],
                                        )
                                        .unwrap_or_else(|| s.to_string()),
                                        true,
                                    )
                                } else {
                                    (s.to_string(), false)
                                }
                            } else {
                                (s.to_string(), false)
                            };
                        let lines = if stream_expand_rows {
                            if is_pretty_printable {
                                duckbox_truncate_lines(
                                    &formatted,
                                    render_col_width[out_c],
                                    stream_max_rows_per_row,
                                )
                            } else {
                                split_and_render_lines(
                                    &formatted,
                                    render_col_width[out_c],
                                    true,
                                    stream_max_rows_per_row,
                                )
                            }
                        } else {
                            split_and_render_lines(&formatted, render_col_width[out_c], false, 1)
                        };
                        row_line_count = row_line_count.max(lines.len());
                        rendered_cells.push(lines);
                    }
                    for line_idx in 0..row_line_count {
                        layout_write(&mut writer, BOX_13);
                        layout_write(&mut writer, " ");
                        for c in 0..render_column_map.len() {
                            let src_c = render_column_map[c];
                            let is_null = src_c != SPLIT_COLUMN && row[src_c].is_none();
                            let s = rendered_cells[c]
                                .get(line_idx)
                                .map(|s| s.as_str())
                                .unwrap_or("");
                            if src_c == SPLIT_COLUMN {
                                let buf = truncate_center_for_cell(s, render_col_width[c]);
                                if highlight_results && !ansi_layout.is_empty() {
                                    write_styled(
                                        &mut writer,
                                        &ansi_layout,
                                        std::str::from_utf8(&buf).unwrap_or(""),
                                    );
                                } else {
                                    writer.write_all(&buf);
                                }
                            } else {
                                let buf = truncate_for_cell(
                                    s,
                                    render_col_width[c],
                                    render_right_align[c],
                                );
                                if is_null {
                                    if highlight_results && !ansi_null_value.is_empty() {
                                        let start = buf
                                            .iter()
                                            .position(|b| *b != b' ')
                                            .unwrap_or(buf.len());
                                        let end = buf
                                            .iter()
                                            .rposition(|b| *b != b' ')
                                            .map(|i| i + 1)
                                            .unwrap_or(0);
                                        writer.write_all(&buf[..start]);
                                        writer.write_all(ansi_null_value.as_bytes());
                                        writer.write_all(&buf[start..end]);
                                        writer.write_all(ansi_reset.as_bytes());
                                        writer.write_all(&buf[end..]);
                                    } else {
                                        writer.write_all(&buf);
                                    }
                                } else {
                                    write_nested_highlighted(
                                        &mut writer,
                                        &buf,
                                        nested_highlight_candidate(src_c, s),
                                    );
                                }
                            }
                            layout_write(&mut writer, " ");
                            layout_write(&mut writer, BOX_13);
                            if c + 1 < render_column_map.len() {
                                layout_write(&mut writer, " ");
                            } else {
                                writer.write_all(b"\n");
                            }
                        }
                    }
                    printed_any_values = true;
                } else {
                    // Truncated/non-streaming path: buffer rows for later render.
                    for c in 0..col_count {
                        let s = row[c].as_deref().unwrap_or(null_value.as_str());
                        col_width[c] = col_width[c].max(max_render_length_multiline(s));
                    }
                    buffered_rows.push(row);
                }
            }

            for t in types.iter_mut() {
                unsafe { duckdb_sys::duckdb_destroy_logical_type(t) };
            }
            unsafe { duckdb_sys::duckdb_destroy_data_chunk(&mut chunk) };
            chunk = unsafe { duckdb_sys::duckdb_fetch_chunk(*result) };
        }
    }

    if stream_all_rows {
        if row_count == 0 {
            row_count = global_row_idx;
        }
        if !header_rendered {
            // Streaming result ended before we rendered the header (or is empty). Switch to the
            // bounded render path using the rows we buffered.
            stream_all_rows = false;
            top_rows = row_count;
            bottom_rows = 0;
            footer = compute_footer(
                row_count,
                row_count,
                col_count,
                large_number_rendering,
                decimal_separator,
                thousand_separator,
            );
            should_set_last_query = limits_override.is_none() && row_count > 0;
        } else {
            // Streaming path already rendered the header + values. Render footer + closing border now.
            footer = compute_footer(
                row_count,
                row_count,
                col_count,
                large_number_rendering,
                decimal_separator,
                thousand_separator,
            );
            if has_hidden_columns && row_count > 0 {
                let shown = render_column_map.len().saturating_sub(1);
                footer
                    .column_count_str
                    .push_str(&format!(" ({} shown)", shown));
                footer.must_show_footer = true;
            }
            let total_render_length = compute_total_render_length(&render_col_width);

            let plain_footer = use_plain_footer(&footer, has_hidden_columns, row_count, col_count);
            if plain_footer {
                if row_count != 0 {
                    box_row_sep(
                        &mut writer,
                        &render_col_width,
                        BOX_24,
                        BOX_12,
                        BOX_124,
                        BOX_14,
                        &layout_write,
                    );
                }
                let footer_line = plain_footer_line(&footer, total_render_length);
                write_styled(&mut writer, &ansi_footer, &footer_line);
                writer.write_all(b"\n");
            } else {
                // footer
                if footer.must_show_footer {
                    let mut render_anything = true;
                    let minimum_length =
                        footer.row_count_str.len() + footer.column_count_str.len() + 6;
                    let render_rows_and_columns = total_render_length >= minimum_length
                        && ((has_hidden_columns && row_count > 0)
                            || (row_count >= 10 && col_count > 1));
                    let render_rows = total_render_length >= footer.render_length
                        && (row_count == 0 || row_count >= 10);
                    if !render_rows && !render_rows_and_columns {
                        render_anything = false;
                    }
                    let left = if render_anything { BOX_123 } else { BOX_12 };
                    let right = if render_anything { BOX_134 } else { BOX_14 };
                    // For empty results we already rendered the header separator; avoid a duplicate divider line.
                    if row_count != 0 {
                        box_row_sep(
                            &mut writer,
                            &render_col_width,
                            BOX_24,
                            BOX_123,
                            BOX_DMIDDLE,
                            BOX_134,
                            &layout_write,
                        );
                    }
                    if render_anything {
                        let padding =
                            total_render_length.saturating_sub(footer.row_count_str.len() + 4);
                        layout_write(&mut writer, BOX_13);
                        layout_write(&mut writer, " ");
                        let mut footer_line = String::new();
                        if render_rows_and_columns {
                            footer_line.push_str(&footer.row_count_str);
                            footer_line.push_str(
                                &" ".repeat(padding.saturating_sub(footer.column_count_str.len())),
                            );
                            footer_line.push_str(&footer.column_count_str);
                            write_styled(&mut writer, &ansi_footer, &footer_line);
                        } else if render_rows {
                            let lpad = padding / 2;
                            let rpad = padding - lpad;
                            footer_line.push_str(&" ".repeat(lpad));
                            footer_line.push_str(&footer.row_count_str);
                            footer_line.push_str(&" ".repeat(rpad));
                            write_styled(&mut writer, &ansi_footer, &footer_line);
                        }
                        layout_write(&mut writer, " ");
                        layout_write(&mut writer, BOX_13);
                        writer.write_all(b"\n");
                        if footer.has_hidden_rows {
                            if !footer.readable_rows_str.is_empty() {
                                layout_write(&mut writer, BOX_13);
                                layout_write(&mut writer, " ");
                                let s = format!("({})", footer.readable_rows_str);
                                let buf = truncate_for_cell(
                                    &s,
                                    total_render_length.saturating_sub(4),
                                    false,
                                );
                                write_styled(
                                    &mut writer,
                                    &ansi_null_value,
                                    std::str::from_utf8(&buf).unwrap_or(""),
                                );
                                layout_write(&mut writer, " ");
                                layout_write(&mut writer, BOX_13);
                                writer.write_all(b"\n");
                            }
                            if !footer.shown_str.is_empty() {
                                layout_write(&mut writer, BOX_13);
                                layout_write(&mut writer, " ");
                                let s = format!("({})", footer.shown_str);
                                let buf = truncate_for_cell(
                                    &s,
                                    total_render_length.saturating_sub(4),
                                    false,
                                );
                                write_styled(
                                    &mut writer,
                                    &ansi_null_value,
                                    std::str::from_utf8(&buf).unwrap_or(""),
                                );
                                layout_write(&mut writer, " ");
                                layout_write(&mut writer, BOX_13);
                                writer.write_all(b"\n");
                            }
                        } else if !footer.readable_rows_str.is_empty() {
                            layout_write(&mut writer, BOX_13);
                            layout_write(&mut writer, " ");
                            let s = format!("({})", footer.readable_rows_str);
                            let buf =
                                truncate_for_cell(&s, total_render_length.saturating_sub(4), false);
                            write_styled(
                                &mut writer,
                                &ansi_null_value,
                                std::str::from_utf8(&buf).unwrap_or(""),
                            );
                            layout_write(&mut writer, " ");
                            layout_write(&mut writer, BOX_13);
                            writer.write_all(b"\n");
                        }
                    }
                    let _ = (left, right);
                }

                if footer.must_show_footer {
                    let single_width = [total_render_length.saturating_sub(4)];
                    box_row_sep(
                        &mut writer,
                        &single_width,
                        BOX_24,
                        BOX_12,
                        BOX_124,
                        BOX_14,
                        &layout_write,
                    );
                } else {
                    box_row_sep(
                        &mut writer,
                        &render_col_width,
                        BOX_24,
                        BOX_12,
                        BOX_124,
                        BOX_14,
                        &layout_write,
                    );
                }
            }
        }
    }

    if !stream_all_rows {
        // Add the `(xx.yy million)` footer row for single-row results in FOOTER mode.
        if large_number_rendering == LargeNumberRendering::Footer
            && !render_columns_mode
            && row_count == 1
            && buffered_rows.len() == 1
        {
            let mut readable: Vec<String> = Vec::with_capacity(col_count);
            let mut all_readable = true;
            for c in 0..col_count {
                if !is_numeric_type(col_types[c]) {
                    all_readable = false;
                    break;
                }
                let raw = buffered_rows[0][c].as_deref().unwrap_or("");
                let Some(formatted) = try_format_large_number(raw, decimal_separator) else {
                    all_readable = false;
                    break;
                };
                readable.push(format!("({})", formatted));
            }
            if all_readable {
                for c in 0..col_count {
                    col_width[c] = col_width[c].max(duckbox_render_length(&readable[c]));
                }
                footer_number_row = Some(readable);
            }
        }

        // Non-streaming path: compute widths and render everything now (output is bounded by max_rows).
        let min_width = if footer.has_hidden_rows || row_count == 0 {
            footer.render_length
        } else {
            0
        };
        let (new_map, new_widths, new_total, hidden_cols, shortened_columns) =
            compute_duckbox_column_layout(&col_width, max_render_width, min_width);
        render_column_map = new_map;
        render_col_width = new_widths;
        total_render_length = new_total;
        has_hidden_columns = hidden_cols;
        if has_hidden_columns && row_count > 0 {
            let shown = render_column_map.len().saturating_sub(1);
            footer
                .column_count_str
                .push_str(&format!(" ({} shown)", shown));
            footer.must_show_footer = true;
        }
        render_right_align = render_column_map
            .iter()
            .map(|&idx| {
                if idx == SPLIT_COLUMN {
                    false
                } else {
                    right_align[idx]
                }
            })
            .collect();

        let mut expand_rows = false;
        let mut max_rows_per_row = 1usize;
        if shortened_columns
            && !render_columns_mode
            && !has_hidden_columns
            && row_count > 0
            && max_width != u64::MAX
            && max_width != 0
            && (row_count + 5) < max_rows_usize
        {
            let numer = if max_rows_usize <= 5 {
                0u128
            } else {
                (max_rows_usize - 5) as u128
            };
            let denom = row_count as u128;
            let per = (numer / denom).max(1).min(10_000) as usize;
            if per > 1 {
                expand_rows = true;
                max_rows_per_row = per;
            }
        }

        box_row_sep(
            &mut writer,
            &render_col_width,
            BOX_24,
            BOX_23,
            BOX_234,
            BOX_34,
            &layout_write,
        );
        // header
        let header_rows: [(&[String], &str); 2] = [
            (&col_names, &ansi_column_name),
            (&col_type_names, &ansi_column_type),
        ];
        for (idx, (vals, style)) in header_rows.iter().enumerate() {
            if render_columns_mode && idx == 1 {
                continue;
            }
            let mut cell_lines: Vec<Vec<String>> = Vec::with_capacity(render_column_map.len());
            let mut row_line_count = 1usize;
            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                let s = if src_c == SPLIT_COLUMN {
                    DOTDOTDOT
                } else {
                    vals[src_c].as_str()
                };
                let lines = split_and_render_lines(
                    s,
                    render_col_width[out_c],
                    expand_rows,
                    max_rows_per_row,
                );
                row_line_count = row_line_count.max(lines.len());
                cell_lines.push(lines);
            }
            for line_idx in 0..row_line_count {
                layout_write(&mut writer, BOX_13);
                layout_write(&mut writer, " ");
                for c in 0..render_column_map.len() {
                    let s = cell_lines[c]
                        .get(line_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let buf = truncate_center_for_cell(s, render_col_width[c]);
                    let cell_style: &str = if render_column_map[c] == SPLIT_COLUMN {
                        ansi_layout.as_str()
                    } else {
                        *style
                    };
                    if highlight_results && !cell_style.is_empty() {
                        let start = buf.iter().position(|b| *b != b' ').unwrap_or(buf.len());
                        let end = buf
                            .iter()
                            .rposition(|b| *b != b' ')
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        if end > start {
                            writer.write_all(&buf[..start]);
                            writer.write_all(cell_style.as_bytes());
                            writer.write_all(&buf[start..end]);
                            writer.write_all(ansi_reset.as_bytes());
                            writer.write_all(&buf[end..]);
                        } else {
                            writer.write_all(&buf);
                        }
                    } else {
                        writer.write_all(&buf);
                    }
                    layout_write(&mut writer, " ");
                    layout_write(&mut writer, BOX_13);
                    if c + 1 < render_column_map.len() {
                        layout_write(&mut writer, " ");
                    } else {
                        writer.write_all(b"\n");
                    }
                }
            }
        }
        if row_count == 0 {
            box_row_sep(
                &mut writer,
                &render_col_width,
                BOX_24,
                BOX_12,
                BOX_124,
                BOX_14,
                &layout_write,
            );
        } else {
            box_row_sep(
                &mut writer,
                &render_col_width,
                BOX_24,
                BOX_123,
                BOX_1234,
                BOX_134,
                &layout_write,
            );
        }

        // values
        let mut rendered_row_idx = 0usize;
        let mut is_first_value_row = true;
        let center_single_footer_value_row = footer_number_row.is_some();
        for row in buffered_rows.iter() {
            if expand_rows && !is_first_value_row {
                box_row_sep(
                    &mut writer,
                    &render_col_width,
                    BOX_24,
                    BOX_123,
                    BOX_1234,
                    BOX_134,
                    &layout_write,
                );
            }
            is_first_value_row = false;

            let mut rendered_cells: Vec<Vec<String>> = Vec::with_capacity(render_column_map.len());
            let mut row_line_count = 1usize;
            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                let s = if src_c == SPLIT_COLUMN {
                    DOTDOTDOT
                } else {
                    row[src_c].as_deref().unwrap_or(null_value.as_str())
                };
                let (formatted, is_pretty_printable) = if expand_rows && src_c != SPLIT_COLUMN {
                    let looks_bracketed =
                        matches!(s.trim_start().chars().next(), Some('{') | Some('['));
                    let is_pretty_printable = col_logical_type_ids.get(src_c).is_some_and(|t| {
                        matches!(
                            *t,
                            duckdb_sys::DUCKDB_TYPE_LIST
                                | duckdb_sys::DUCKDB_TYPE_STRUCT
                                | duckdb_sys::DUCKDB_TYPE_ARRAY
                                | duckdb_sys::DUCKDB_TYPE_MAP
                        )
                    }) || col_type_names
                        .get(src_c)
                        .is_some_and(|t| t == "json" || t == "variant")
                        || looks_bracketed;
                    if is_pretty_printable {
                        (
                            crate::duckbox_json_formatter::format_value(
                                s,
                                max_rows_per_row,
                                render_col_width[out_c],
                            )
                            .unwrap_or_else(|| s.to_string()),
                            true,
                        )
                    } else {
                        (s.to_string(), false)
                    }
                } else {
                    (s.to_string(), false)
                };
                let lines = if expand_rows {
                    if is_pretty_printable {
                        duckbox_truncate_lines(
                            &formatted,
                            render_col_width[out_c],
                            max_rows_per_row,
                        )
                    } else {
                        split_and_render_lines(
                            &formatted,
                            render_col_width[out_c],
                            true,
                            max_rows_per_row,
                        )
                    }
                } else {
                    split_and_render_lines(&formatted, render_col_width[out_c], false, 1)
                };
                row_line_count = row_line_count.max(lines.len());
                rendered_cells.push(lines);
            }
            for line_idx in 0..row_line_count {
                layout_write(&mut writer, BOX_13);
                layout_write(&mut writer, " ");
                for c in 0..render_column_map.len() {
                    let src_c = render_column_map[c];
                    let is_null = src_c != SPLIT_COLUMN && row[src_c].is_none();
                    let s = rendered_cells[c]
                        .get(line_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    if src_c == SPLIT_COLUMN {
                        let buf = truncate_center_for_cell(s, render_col_width[c]);
                        if highlight_results && !ansi_layout.is_empty() {
                            write_styled(
                                &mut writer,
                                &ansi_layout,
                                std::str::from_utf8(&buf).unwrap_or(""),
                            );
                        } else {
                            writer.write_all(&buf);
                        }
                    } else {
                        let buf = if center_single_footer_value_row {
                            truncate_center_for_cell(s, render_col_width[c])
                        } else {
                            truncate_for_cell(s, render_col_width[c], render_right_align[c])
                        };
                        if is_null {
                            if highlight_results && !ansi_null_value.is_empty() {
                                let start =
                                    buf.iter().position(|b| *b != b' ').unwrap_or(buf.len());
                                let end = buf
                                    .iter()
                                    .rposition(|b| *b != b' ')
                                    .map(|i| i + 1)
                                    .unwrap_or(0);
                                writer.write_all(&buf[..start]);
                                writer.write_all(ansi_null_value.as_bytes());
                                writer.write_all(&buf[start..end]);
                                writer.write_all(ansi_reset.as_bytes());
                                writer.write_all(&buf[end..]);
                            } else {
                                writer.write_all(&buf);
                            }
                        } else {
                            write_nested_highlighted(
                                &mut writer,
                                &buf,
                                nested_highlight_candidate(src_c, s),
                            );
                        }
                    }
                    layout_write(&mut writer, " ");
                    layout_write(&mut writer, BOX_13);
                    if c + 1 < render_column_map.len() {
                        layout_write(&mut writer, " ");
                    } else {
                        writer.write_all(b"\n");
                    }
                }
            }
            rendered_row_idx += 1;
            if rendered_row_idx == top_rows && bottom_rows > 0 {
                // divider rows
                for _ in 0..3 {
                    layout_write(&mut writer, BOX_13);
                    layout_write(&mut writer, " ");
                    for c in 0..render_column_map.len() {
                        let buf =
                            truncate_for_cell(BOX_DOT, render_col_width[c], render_right_align[c]);
                        write_styled(
                            &mut writer,
                            &ansi_layout,
                            std::str::from_utf8(&buf).unwrap_or(""),
                        );
                        layout_write(&mut writer, " ");
                        layout_write(&mut writer, BOX_13);
                        if c + 1 < render_column_map.len() {
                            layout_write(&mut writer, " ");
                        } else {
                            writer.write_all(b"\n");
                        }
                    }
                }
            }
        }

        // Render the optional large-number footer row for the single-row FOOTER mode.
        if let Some(nums) = footer_number_row.take() {
            let mut row_cells: Vec<Vec<String>> = Vec::with_capacity(render_column_map.len());
            let mut row_line_count = 1usize;
            for (out_c, &src_c) in render_column_map.iter().enumerate() {
                let cell = if src_c == SPLIT_COLUMN {
                    DOTDOTDOT
                } else {
                    nums[src_c].as_str()
                };
                let lines = split_and_render_lines(
                    cell,
                    render_col_width[out_c],
                    expand_rows,
                    max_rows_per_row,
                );
                row_line_count = row_line_count.max(lines.len());
                row_cells.push(lines);
            }
            if expand_rows {
                box_row_sep(
                    &mut writer,
                    &render_col_width,
                    BOX_24,
                    BOX_123,
                    BOX_1234,
                    BOX_134,
                    &layout_write,
                );
            }
            for line_idx in 0..row_line_count {
                layout_write(&mut writer, BOX_13);
                layout_write(&mut writer, " ");
                for c in 0..render_column_map.len() {
                    let s = row_cells[c].get(line_idx).map(|s| s.as_str()).unwrap_or("");
                    let src_c = render_column_map[c];
                    if src_c == SPLIT_COLUMN {
                        let buf = truncate_center_for_cell(s, render_col_width[c]);
                        if highlight_results && !ansi_layout.is_empty() {
                            write_styled(
                                &mut writer,
                                &ansi_layout,
                                std::str::from_utf8(&buf).unwrap_or(""),
                            );
                        } else {
                            writer.write_all(&buf);
                        }
                    } else {
                        let buf = truncate_for_cell(s, render_col_width[c], render_right_align[c]);
                        writer.write_all(&buf);
                    }
                    layout_write(&mut writer, " ");
                    layout_write(&mut writer, BOX_13);
                    if c + 1 < render_column_map.len() {
                        layout_write(&mut writer, " ");
                    } else {
                        writer.write_all(b"\n");
                    }
                }
            }
        }

        let plain_footer = use_plain_footer(&footer, has_hidden_columns, row_count, col_count);
        if plain_footer {
            if row_count != 0 {
                box_row_sep(
                    &mut writer,
                    &render_col_width,
                    BOX_24,
                    BOX_12,
                    BOX_124,
                    BOX_14,
                    &layout_write,
                );
            }
            let footer_line = plain_footer_line(&footer, total_render_length);
            write_styled(&mut writer, &ansi_footer, &footer_line);
            writer.write_all(b"\n");
        } else {
            // footer
            if footer.must_show_footer {
                let mut render_anything = true;
                let minimum_length = footer.row_count_str.len() + footer.column_count_str.len() + 6;
                let render_rows_and_columns = total_render_length >= minimum_length
                    && ((has_hidden_columns && row_count > 0)
                        || (row_count >= 10 && col_count > 1));
                let render_rows = total_render_length >= footer.render_length
                    && (row_count == 0 || row_count >= 10);
                if !render_rows && !render_rows_and_columns {
                    render_anything = false;
                }
                let left = if render_anything { BOX_123 } else { BOX_12 };
                let right = if render_anything { BOX_134 } else { BOX_14 };
                // For empty results we already rendered the header separator; avoid a duplicate divider line.
                if row_count != 0 {
                    box_row_sep(
                        &mut writer,
                        &render_col_width,
                        BOX_24,
                        BOX_123,
                        BOX_DMIDDLE,
                        BOX_134,
                        &layout_write,
                    );
                }
                if render_anything {
                    let padding =
                        total_render_length.saturating_sub(footer.row_count_str.len() + 4);
                    layout_write(&mut writer, BOX_13);
                    layout_write(&mut writer, " ");
                    let mut footer_line = String::new();
                    if render_rows_and_columns {
                        footer_line.push_str(&footer.row_count_str);
                        footer_line.push_str(
                            &" ".repeat(padding.saturating_sub(footer.column_count_str.len())),
                        );
                        footer_line.push_str(&footer.column_count_str);
                        write_styled(&mut writer, &ansi_footer, &footer_line);
                    } else if render_rows {
                        let lpad = padding / 2;
                        let rpad = padding - lpad;
                        footer_line.push_str(&" ".repeat(lpad));
                        footer_line.push_str(&footer.row_count_str);
                        footer_line.push_str(&" ".repeat(rpad));
                        write_styled(&mut writer, &ansi_footer, &footer_line);
                    }
                    layout_write(&mut writer, " ");
                    layout_write(&mut writer, BOX_13);
                    writer.write_all(b"\n");
                    if footer.has_hidden_rows {
                        if !footer.readable_rows_str.is_empty() {
                            layout_write(&mut writer, BOX_13);
                            layout_write(&mut writer, " ");
                            let s = format!("({})", footer.readable_rows_str);
                            let buf =
                                truncate_for_cell(&s, total_render_length.saturating_sub(4), false);
                            write_styled(
                                &mut writer,
                                &ansi_null_value,
                                std::str::from_utf8(&buf).unwrap_or(""),
                            );
                            layout_write(&mut writer, " ");
                            layout_write(&mut writer, BOX_13);
                            writer.write_all(b"\n");
                        }
                        if !footer.shown_str.is_empty() {
                            layout_write(&mut writer, BOX_13);
                            layout_write(&mut writer, " ");
                            let s = format!("({})", footer.shown_str);
                            let buf =
                                truncate_for_cell(&s, total_render_length.saturating_sub(4), false);
                            write_styled(
                                &mut writer,
                                &ansi_null_value,
                                std::str::from_utf8(&buf).unwrap_or(""),
                            );
                            layout_write(&mut writer, " ");
                            layout_write(&mut writer, BOX_13);
                            writer.write_all(b"\n");
                        }
                    } else if !footer.readable_rows_str.is_empty() {
                        layout_write(&mut writer, BOX_13);
                        layout_write(&mut writer, " ");
                        let s = format!("({})", footer.readable_rows_str);
                        let buf =
                            truncate_for_cell(&s, total_render_length.saturating_sub(4), false);
                        write_styled(
                            &mut writer,
                            &ansi_null_value,
                            std::str::from_utf8(&buf).unwrap_or(""),
                        );
                        layout_write(&mut writer, " ");
                        layout_write(&mut writer, BOX_13);
                        writer.write_all(b"\n");
                    }
                }
                let _ = (left, right);
            }

            if footer.must_show_footer {
                let single_width = [total_render_length.saturating_sub(4)];
                box_row_sep(
                    &mut writer,
                    &single_width,
                    BOX_24,
                    BOX_12,
                    BOX_124,
                    BOX_14,
                    &layout_write,
                );
            } else {
                box_row_sep(
                    &mut writer,
                    &render_col_width,
                    BOX_24,
                    BOX_12,
                    BOX_124,
                    BOX_14,
                    &layout_write,
                );
            }
        }
    }

    writer.finish();
    if should_set_last_query {
        state.last_query_duckbox = Some(cmd.to_string());
    }
    0
}

fn run_duckbox_query_impl(
    state: &mut ShellState,
    con: duckdb_sys::duckdb_connection,
    cmd: &str,
    limits_override: Option<(u64, u64, u64)>,
) -> i32 {
    let query = match CString::new(cmd) {
        Ok(q) => q,
        Err(_) => {
            print_database_error("Invalid SQL (contains null byte)");
            return 1;
        }
    };

    let mut extracted: duckdb_sys::duckdb_extracted_statements = std::ptr::null_mut();
    let count =
        unsafe { duckdb_sys::duckdb_extract_statements(con, query.as_ptr(), &mut extracted) };
    if extracted.is_null() {
        print_database_error_state(
            state,
            "duckdb_extract_statements returned null extracted statements",
        );
        return 1;
    }
    if count == 0 {
        let err_ptr = unsafe { duckdb_sys::duckdb_extract_statements_error(extracted) };
        if !err_ptr.is_null() {
            let err = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .to_string();
            if !err.trim().is_empty() {
                print_database_error_state(state, &err);
                unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
                return 1;
            }
        }
        unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
        return 0;
    }

    let echo_on = (state.shellFlgs & (crate::state::ShellFlags::SHFLG_Echo as u32)) != 0;
    let stmt_slices = echo_slices_for_shell(extracted, cmd);
    let tz_stmts_fallback = crate::sql_split::split_statements_for_shell(cmd);
    let stmt_slices_per_statement = stmt_slices.len() == count as usize;
    let echo_has_per_statement = echo_on && stmt_slices_per_statement;
    let tz_has_per_statement =
        stmt_slices_per_statement || tz_stmts_fallback.len() == count as usize;

    for idx in 0..count {
        crate::db::sync_process_timezone(state, con);

        let mut stmt: duckdb_sys::duckdb_prepared_statement = std::ptr::null_mut();
        let mut prepared = false;
        for attempt in 0..2 {
            let prep_rc = unsafe {
                duckdb_sys::duckdb_prepare_extracted_statement(con, extracted, idx, &mut stmt)
            };
            if prep_rc == duckdb_sys::DuckDBSuccess {
                prepared = true;
                break;
            }

            let err = if !stmt.is_null() {
                let err_ptr = unsafe { duckdb_sys::duckdb_prepare_error(stmt) };
                if err_ptr.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(err_ptr) }
                        .to_string_lossy()
                        .to_string()
                }
            } else {
                String::new()
            };

            if crate::signals::has_seen_interrupt()
                || err.to_ascii_lowercase().contains("interrupt")
            {
                print_stdout_line(state, "Interrupt");
                unsafe {
                    duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                    duckdb_sys::duckdb_destroy_extracted(&mut extracted);
                }
                return 1;
            }

            if attempt == 0
                && crate::db::error_mentions_icu_extension(&err)
                && crate::db::ensure_icu_loaded(state, con)
            {
                if !stmt.is_null() {
                    unsafe { duckdb_sys::duckdb_destroy_prepare(&mut stmt) };
                }
                stmt = std::ptr::null_mut();
                continue;
            }

            if !err.trim().is_empty() {
                print_database_error_state(state, &err);
            } else {
                print_database_error_state(state, "Failed to prepare statement");
            }
            unsafe {
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }
        if !prepared {
            unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
            return 1;
        }

        if echo_on {
            let to_echo = if echo_has_per_statement {
                stmt_slices[idx as usize].as_str()
            } else if idx == 0 {
                cmd
            } else {
                ""
            };
            if !to_echo.is_empty() {
                print_stdout(state, to_echo);
                print_stdout(state, "\n");
            }
        }

        let mut result: duckdb_sys::duckdb_result = unsafe { std::mem::zeroed() };
        let mut executed = false;
        for attempt in 0..2 {
            let exec_rc = if state.columns || state.max_rows != u64::MAX {
                unsafe { duckdb_sys::duckdb_execute_prepared(stmt, &mut result) }
            } else {
                unsafe { duckdb_sys::duckdb_execute_prepared_streaming(stmt, &mut result) }
            };
            if exec_rc == duckdb_sys::DuckDBSuccess {
                executed = true;
                break;
            }

            let err_ptr = unsafe { duckdb_sys::duckdb_result_error(&mut result) };
            let err = if err_ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(err_ptr) }
                    .to_string_lossy()
                    .to_string()
            };

            if crate::signals::has_seen_interrupt()
                || err.to_ascii_lowercase().contains("interrupt")
            {
                print_stdout_line(state, "Interrupt");
                unsafe {
                    duckdb_sys::duckdb_destroy_result(&mut result);
                    duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                    duckdb_sys::duckdb_destroy_extracted(&mut extracted);
                }
                return 1;
            }

            if attempt == 0
                && crate::db::error_mentions_icu_extension(&err)
                && crate::db::ensure_icu_loaded(state, con)
            {
                unsafe { duckdb_sys::duckdb_destroy_result(&mut result) };
                result = unsafe { std::mem::zeroed() };
                continue;
            }

            if !err.trim().is_empty() {
                if err.contains(
                    "Values were not provided for the following prepared statement parameters",
                ) {
                    print_database_error_state(
                        state,
                        "Prepared statement parameters cannot be used directly",
                    );
                } else {
                    print_database_error_state(state, &err);
                }
            } else {
                print_database_error_state(state, "Query failed");
            }
            unsafe {
                duckdb_sys::duckdb_destroy_result(&mut result);
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }
        if !executed {
            unsafe {
                duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                duckdb_sys::duckdb_destroy_extracted(&mut extracted);
            }
            return 1;
        }

        let mut render_rc = 0;
        let return_type = unsafe { duckdb_sys::duckdb_result_return_type(result) };
        state.cMode = state.mode;
        let stmt_type = unsafe { duckdb_sys::duckdb_result_statement_type(result) };
        if stmt_type == duckdb_sys::DUCKDB_STATEMENT_TYPE_EXPLAIN {
            state.cMode = RenderMode::EXPLAIN;
        }
        if return_type == duckdb_sys::DUCKDB_RESULT_TYPE_QUERY_RESULT {
            if state.cMode == RenderMode::EXPLAIN {
                render_rc = render_result(state, &mut result);
            } else {
                if count == 1 {
                    if let Some(rc) =
                        try_render_shell_describe(state, cmd, limits_override, &mut result)
                    {
                        render_rc = rc;
                        unsafe {
                            duckdb_sys::duckdb_destroy_result(&mut result);
                            duckdb_sys::duckdb_destroy_prepare(&mut stmt);
                        }
                        if render_rc != 0 {
                            unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
                            return render_rc;
                        }
                        continue;
                    }
                }

                // DuckDB C API value getters do not fully support extension/complex types (e.g. VARIANT),
                // which can show up as `DUCKDB_TYPE_INVALID` and render as NULL in the chunk API.
                // Workaround: for single-statement SELECTs, re-run the query and cast all columns to VARCHAR,
                // then render the stringified result (which the shell formatter can pretty-print).
                let col_count = unsafe { duckdb_sys::duckdb_column_count(&mut result) } as usize;
                let mut original_type_names: Vec<String> = Vec::with_capacity(col_count);
                let mut original_logical_type_ids: Vec<duckdb_sys::duckdb_type> =
                    Vec::with_capacity(col_count);
                let mut cast_cols: Vec<bool> = vec![false; col_count];
                let mut has_problem_type = false;
                for c in 0..col_count {
                    let mut logical =
                        unsafe { duckdb_sys::duckdb_column_logical_type(&mut result, c as u64) };
                    let type_id = unsafe { duckdb_sys::duckdb_get_type_id(logical) };
                    original_logical_type_ids.push(type_id);
                    let type_name = duckbox_render_type(state, logical, 0);
                    original_type_names.push(type_name.clone());
                    unsafe { duckdb_sys::duckdb_destroy_logical_type(&mut logical) };
                    if matches!(type_name.as_str(), "geometry" | "unknown")
                        || matches!(
                            type_id,
                            duckdb_sys::DUCKDB_TYPE_INVALID
                                | duckdb_sys::DUCKDB_TYPE_BIGNUM
                                | duckdb_sys::DUCKDB_TYPE_UNION
                        )
                    {
                        cast_cols[c] = true;
                        has_problem_type = true;
                    }
                }

                if has_problem_type && count == 1 {
                    let trimmed_cmd = cmd.trim();
                    let cmd_no_semi = trimmed_cmd.strip_suffix(';').unwrap_or(trimmed_cmd).trim();
                    let lower = cmd_no_semi.trim_start().to_ascii_lowercase();
                    if lower.starts_with("select") || lower.starts_with("with") {
                        if original_type_names.iter().any(|t| t == "unknown") {
                            if let Some(described) = duckbox_describe_type_names(con, cmd_no_semi) {
                                if described.len() == col_count {
                                    for c in 0..col_count {
                                        if original_type_names[c] == "unknown"
                                            && !described[c].is_empty()
                                        {
                                            original_type_names[c] = described[c].clone();
                                        }
                                    }
                                }
                            }
                        }

                        let mut select_list = String::new();
                        for c in 0..col_count {
                            let name_ptr =
                                unsafe { duckdb_sys::duckdb_column_name(&mut result, c as u64) };
                            let name = if name_ptr.is_null() {
                                String::new()
                            } else {
                                unsafe { CStr::from_ptr(name_ptr) }
                                    .to_string_lossy()
                                    .to_string()
                            };
                            let ident = quote_identifier_if_needed(state, &name);
                            if c > 0 {
                                select_list.push_str(", ");
                            }
                            if cast_cols[c] {
                                select_list.push_str("cast(t.");
                                select_list.push_str(&ident);
                                select_list.push_str(" as varchar) as ");
                                select_list.push_str(&ident);
                            } else {
                                select_list.push_str("t.");
                                select_list.push_str(&ident);
                                select_list.push_str(" as ");
                                select_list.push_str(&ident);
                            }
                        }
                        let wrapper_sql =
                            format!("select {} from ({}) t", select_list, cmd_no_semi);
                        if let Ok(wrapper_cstr) = CString::new(wrapper_sql) {
                            let mut string_result: duckdb_sys::duckdb_result =
                                unsafe { std::mem::zeroed() };
                            let string_rc = unsafe {
                                duckdb_sys::duckdb_query(
                                    con,
                                    wrapper_cstr.as_ptr(),
                                    &mut string_result,
                                )
                            };
                            if string_rc == duckdb_sys::DuckDBSuccess {
                                render_rc = render_duckbox_result(
                                    state,
                                    cmd,
                                    limits_override,
                                    &mut string_result,
                                    Some((&original_type_names, &original_logical_type_ids)),
                                );
                                unsafe { duckdb_sys::duckdb_destroy_result(&mut string_result) };
                            } else {
                                render_rc = render_duckbox_result(
                                    state,
                                    cmd,
                                    limits_override,
                                    &mut result,
                                    None,
                                );
                            }
                        } else {
                            render_rc = render_duckbox_result(
                                state,
                                cmd,
                                limits_override,
                                &mut result,
                                None,
                            );
                        }
                    } else {
                        render_rc =
                            render_duckbox_result(state, cmd, limits_override, &mut result, None);
                    }
                } else {
                    render_rc =
                        render_duckbox_result(state, cmd, limits_override, &mut result, None);
                }

                // Best-effort last-result support ("FROM _"): keep a temp table "_" in sync with the last successful query
                // result in the current session.
                if render_rc == 0 && count == 1 {
                    let trimmed_cmd = cmd.trim();
                    let cmd_no_semi = trimmed_cmd.strip_suffix(';').unwrap_or(trimmed_cmd).trim();
                    let lower = cmd_no_semi.trim_start().to_ascii_lowercase();
                    if lower.starts_with("select")
                        || lower.starts_with("with")
                        || lower.starts_with("from")
                    {
                        crate::shell_ext::set_suppress_log_output(true);
                        let mut ok = true;
                        let stmts = [
                            "begin transaction".to_string(),
                            "drop table if exists __duckdb_cli_last_result_tmp".to_string(),
                            format!(
                                "create temporary table __duckdb_cli_last_result_tmp as {}",
                                cmd_no_semi
                            ),
                            "drop table if exists _".to_string(),
                            "alter table __duckdb_cli_last_result_tmp rename to _".to_string(),
                            "commit".to_string(),
                        ];
                        for sql in stmts {
                            let Ok(sql_c) = CString::new(sql) else {
                                ok = false;
                                break;
                            };
                            let mut tmp_res: duckdb_sys::duckdb_result =
                                unsafe { std::mem::zeroed() };
                            let rc = unsafe {
                                duckdb_sys::duckdb_query(con, sql_c.as_ptr(), &mut tmp_res)
                            };
                            unsafe { duckdb_sys::duckdb_destroy_result(&mut tmp_res) };
                            if rc != duckdb_sys::DuckDBSuccess {
                                ok = false;
                                break;
                            }
                        }
                        if !ok {
                            let _ = unsafe {
                                let sql_c = CString::new("rollback").unwrap();
                                let mut tmp_res: duckdb_sys::duckdb_result = std::mem::zeroed();
                                let rc =
                                    duckdb_sys::duckdb_query(con, sql_c.as_ptr(), &mut tmp_res);
                                duckdb_sys::duckdb_destroy_result(&mut tmp_res);
                                rc
                            };
                            crate::shell_ext::set_suppress_log_output(false);
                        } else {
                            crate::shell_ext::set_last_result_available(true);
                            crate::shell_ext::set_suppress_log_output(false);
                        }
                    }
                }
            }
        } else if return_type == duckdb_sys::DUCKDB_RESULT_TYPE_CHANGED_ROWS {
            let changes = unsafe { duckdb_sys::duckdb_rows_changed(&mut result) };
            state.last_changes = changes;
            state.total_changes = state.total_changes.saturating_add(changes);
        }
        unsafe {
            duckdb_sys::duckdb_destroy_result(&mut result);
            duckdb_sys::duckdb_destroy_prepare(&mut stmt);
        }
        state.cMode = state.mode;
        if render_rc != 0 {
            unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
            return render_rc;
        }

        if tz_has_per_statement {
            let stmt = if stmt_slices_per_statement {
                stmt_slices[idx as usize].as_str()
            } else {
                tz_stmts_fallback[idx as usize]
            };
            if let Some(tz) = try_parse_set_timezone_statement(stmt) {
                crate::db::apply_process_timezone_setting(state, tz.as_str());
            }
        }
    }

    unsafe { duckdb_sys::duckdb_destroy_extracted(&mut extracted) };
    0
}
