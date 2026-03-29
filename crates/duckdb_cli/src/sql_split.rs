#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuoteMode {
    Single,
    Double,
    Backtick,
    Dollar,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

pub fn split_statements_for_shell(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut out: Vec<&str> = Vec::new();

    let mut i = 0usize;
    let mut stmt_start: Option<usize> = None;

    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut quote: Option<QuoteMode> = None;
    let mut dollar_delim: Option<Vec<u8>> = None;

    while i < bytes.len() {
        let b = bytes[i];

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if let Some(mode) = quote {
            match mode {
                QuoteMode::Single => {
                    if b == b'\'' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                        } else {
                            quote = None;
                            i += 1;
                        }
                    } else if b == b'\\' && i + 1 < bytes.len() {
                        // Best-effort: consume escaped char.
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                QuoteMode::Double => {
                    if b == b'"' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                            i += 2;
                        } else {
                            quote = None;
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                    continue;
                }
                QuoteMode::Backtick => {
                    if b == b'`' {
                        quote = None;
                    }
                    i += 1;
                    continue;
                }
                QuoteMode::Dollar => {
                    let Some(delim) = dollar_delim.as_deref() else {
                        // should not happen; treat as unquoted.
                        quote = None;
                        continue;
                    };
                    let delim_len = delim.len();
                    if b == b'$'
                        && i + delim_len <= bytes.len()
                        && &bytes[i..i + delim_len] == delim
                    {
                        quote = None;
                        dollar_delim = None;
                        i += delim_len;
                        continue;
                    }
                    i += 1;
                    continue;
                }
            }
        }

        // Not in comment/quote: handle whitespace/comments/quotes/statement terminators.
        if stmt_start.is_none() {
            if is_space(b) {
                i += 1;
                continue;
            }
            if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
            stmt_start = Some(i);
        }

        // statement already started
        match b {
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                in_line_comment = true;
                i += 2;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                in_block_comment = true;
                i += 2;
            }
            b'\'' => {
                quote = Some(QuoteMode::Single);
                i += 1;
            }
            b'"' => {
                quote = Some(QuoteMode::Double);
                i += 1;
            }
            b'`' => {
                quote = Some(QuoteMode::Backtick);
                i += 1;
            }
            b'$' => {
                // Dollar-quoted string: $tag$...$tag$ or $$...$$
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'$' {
                    let c = bytes[j];
                    // Keep delimiter conservative: ASCII letters/digits/underscore only.
                    if !(c.is_ascii_alphanumeric() || c == b'_') {
                        break;
                    }
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'$' {
                    let delim = bytes[i..=j].to_vec();
                    dollar_delim = Some(delim);
                    quote = Some(QuoteMode::Dollar);
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            b';' => {
                let start = stmt_start.take().unwrap_or(0);
                out.push(&sql[start..i + 1]);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if let Some(start) = stmt_start {
        out.push(&sql[start..]);
    }

    // Drop entries that are only whitespace/comments after trimming.
    out.into_iter()
        .filter_map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .collect()
}

pub fn normalize_statement_for_echo(stmt: &str) -> &str {
    // Match shell behavior best-effort: trim leading whitespace, then drop trailing semicolons and whitespace.
    let mut s = stmt.trim_start_matches(|c: char| c.is_whitespace());
    loop {
        let trimmed = s.trim_end_matches(|c: char| c.is_whitespace());
        if let Some(no_semi) = trimmed.strip_suffix(';') {
            s = no_semi;
            continue;
        }
        return trimmed;
    }
}

pub fn echo_tails_for_shell(sql: &str) -> Vec<&str> {
    // Shell echo behavior uses DuckDB's ExtractStatements metadata, which (in v1.4.3)
    // effectively prints the remaining query tail starting at each statement start.
    //
    // We approximate this by locating top-level semicolon statement boundaries and then
    // returning slices from each statement start to the end of the input.
    let bytes = sql.as_bytes();
    let mut starts: Vec<usize> = Vec::new();

    let mut i = 0usize;

    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut quote: Option<QuoteMode> = None;
    let mut dollar_delim: Option<Vec<u8>> = None;

    fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                return i + 1;
            }
            i += 1;
        }
        i
    }

    fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        i
    }

    fn find_statement_start(bytes: &[u8], mut i: usize, saw_statement: bool) -> Option<usize> {
        loop {
            i = skip_ws(bytes, i);
            if i >= bytes.len() {
                return None;
            }
            if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                // A line comment between statements is treated as trailing for the previous
                // statement (it does not start the next statement).
                if saw_statement {
                    i = skip_line_comment(bytes, i + 2);
                    continue;
                }
                // Leading line comments are handled outside statement execution (echoed separately).
                i = skip_line_comment(bytes, i + 2);
                continue;
            }
            // Block comments can begin a statement tail in the shipped shell behavior.
            return Some(i);
        }
    }

    if let Some(start) = find_statement_start(bytes, 0, false) {
        starts.push(start);
    } else {
        return Vec::new();
    }

    while i < bytes.len() {
        let b = bytes[i];

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if let Some(mode) = quote {
            match mode {
                QuoteMode::Single => {
                    if b == b'\'' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                        } else {
                            quote = None;
                            i += 1;
                        }
                    } else if b == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                QuoteMode::Double => {
                    if b == b'"' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                            i += 2;
                        } else {
                            quote = None;
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                    continue;
                }
                QuoteMode::Backtick => {
                    if b == b'`' {
                        quote = None;
                    }
                    i += 1;
                    continue;
                }
                QuoteMode::Dollar => {
                    let Some(delim) = dollar_delim.as_deref() else {
                        quote = None;
                        continue;
                    };
                    let delim_len = delim.len();
                    if b == b'$'
                        && i + delim_len <= bytes.len()
                        && &bytes[i..i + delim_len] == delim
                    {
                        quote = None;
                        dollar_delim = None;
                        i += delim_len;
                        continue;
                    }
                    i += 1;
                    continue;
                }
            }
        }

        match b {
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                in_line_comment = true;
                i += 2;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                in_block_comment = true;
                i += 2;
            }
            b'\'' => {
                quote = Some(QuoteMode::Single);
                i += 1;
            }
            b'"' => {
                quote = Some(QuoteMode::Double);
                i += 1;
            }
            b'`' => {
                quote = Some(QuoteMode::Backtick);
                i += 1;
            }
            b'$' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'$' {
                    let c = bytes[j];
                    if !(c.is_ascii_alphanumeric() || c == b'_') {
                        break;
                    }
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'$' {
                    let delim = bytes[i..=j].to_vec();
                    dollar_delim = Some(delim);
                    quote = Some(QuoteMode::Dollar);
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            b';' => {
                let next = i + 1;
                if let Some(start) = find_statement_start(bytes, next, true) {
                    starts.push(start);
                    i = start;
                } else {
                    break;
                }
            }
            _ => i += 1,
        }
    }

    starts
        .into_iter()
        .filter_map(|start| {
            if start >= sql.len() {
                return None;
            }
            let tail = &sql[start..];
            let trimmed = tail.trim_end_matches(|c: char| c.is_whitespace());
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect()
}
