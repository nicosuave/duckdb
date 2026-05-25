fn render_length(s: &str) -> usize {
    duckdb_render_width::compute_render_width_duckbox(s.as_bytes())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComponentType {
    BracketOpen,
    BracketClose,
    Literal,
    Colon,
    Comma,
    NullValue,
}

#[derive(Clone, Debug)]
struct Component {
    typ: ComponentType,
    text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParseState {
    Regular,
    InQuote,
    Escape,
}

fn is_whitespace_escape(c: u8) -> bool {
    c == b'n' || c == b't'
}

fn separator_is_matching(open: u8, close: u8) -> bool {
    (open == b'{' && close == b'}') || (open == b'[' && close == b']')
}

fn process_components(value: &str) -> Option<Vec<Component>> {
    let bytes = value.as_bytes();
    let mut components: Vec<Component> = Vec::new();

    let mut state = ParseState::Regular;
    let mut quote_char: u8 = b'"';
    let mut separators: Vec<u8> = Vec::new();
    let mut can_parse_value = false;

    fn add_literal_char(components: &mut Vec<Component>, ch: u8) {
        let need_new = components
            .last()
            .map(|c| c.typ != ComponentType::Literal)
            .unwrap_or(true);
        if need_new {
            components.push(Component {
                typ: ComponentType::Literal,
                text: String::new(),
            });
        }
        if let Some(last) = components.last_mut() {
            last.text.push(ch as char);
        }
    }

    let mut pos: usize = 0;
    while pos < bytes.len() {
        let c = bytes[pos];
        match state {
            ParseState::Regular => {
                if can_parse_value && pos + 4 < bytes.len() {
                    let token = &bytes[pos..pos + 4];
                    if token.eq_ignore_ascii_case(b"null") {
                        components.push(Component {
                            typ: ComponentType::NullValue,
                            text: "null".to_string(),
                        });
                        pos += 4;
                        continue;
                    }
                }

                match c {
                    b'[' | b'{' => {
                        separators.push(c);
                        components.push(Component {
                            typ: ComponentType::BracketOpen,
                            text: (c as char).to_string(),
                        });
                        can_parse_value = c == b'[';
                        pos += 1;
                    }
                    b']' | b'}' => {
                        let open = separators.pop()?;
                        if !separator_is_matching(open, c) {
                            return None;
                        }
                        components.push(Component {
                            typ: ComponentType::BracketClose,
                            text: (c as char).to_string(),
                        });
                        pos += 1;
                    }
                    b'"' | b'\'' => {
                        quote_char = c;
                        state = ParseState::InQuote;
                        add_literal_char(&mut components, c);
                        pos += 1;
                    }
                    b',' => {
                        components.push(Component {
                            typ: ComponentType::Comma,
                            text: ",".to_string(),
                        });
                        pos += 1;
                    }
                    b':' => {
                        components.push(Component {
                            typ: ComponentType::Colon,
                            text: ":".to_string(),
                        });
                        can_parse_value = true;
                        pos += 1;
                    }
                    b'\\' => {
                        if pos + 1 < bytes.len() && is_whitespace_escape(bytes[pos + 1]) {
                            pos += 2;
                        } else {
                            add_literal_char(&mut components, c);
                            pos += 1;
                        }
                    }
                    b' ' | b'\t' | b'\n' => {
                        pos += 1;
                    }
                    _ => {
                        add_literal_char(&mut components, c);
                        pos += 1;
                    }
                }
            }
            ParseState::InQuote => {
                if c == quote_char {
                    state = ParseState::Regular;
                    add_literal_char(&mut components, c);
                    pos += 1;
                } else if c == b'\\' {
                    state = ParseState::Escape;
                    add_literal_char(&mut components, c);
                    pos += 1;
                } else {
                    add_literal_char(&mut components, c);
                    pos += 1;
                }
            }
            ParseState::Escape => {
                state = ParseState::InQuote;
                add_literal_char(&mut components, c);
                pos += 1;
            }
        }
    }

    if !separators.is_empty() {
        return None;
    }
    Some(components)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FormattingMode {
    Standard,
    CompactVertical,
    CompactHorizontal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FormattingResult {
    Success,
    TooManyRows,
    TooWide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineMode {
    Standard,
    InlinedSingleLine,
    InlinedMultiLine,
}

struct FormatState<'a> {
    mode: FormattingMode,
    result: String,
    component_idx: usize,
    row_count: usize,
    line_length: usize,
    depth: usize,
    max_rows: usize,
    max_width: usize,
    indentation_size: usize,
    format_result: FormattingResult,
    components: &'a [Component],
}

impl<'a> FormatState<'a> {
    fn literal_fits_width(&self, render_width: usize) -> bool {
        self.line_length + render_width <= self.max_width
    }

    fn literal_fits(&self, text: &str) -> bool {
        self.literal_fits_width(render_length(text))
    }

    fn add_newline(&mut self) {
        self.result.push('\n');
        self.result.push_str(&" ".repeat(self.depth));
        self.row_count += 1;
        if self.row_count > self.max_rows {
            self.format_result = FormattingResult::TooManyRows;
            return;
        }
        self.line_length = self.depth;
        if self.line_length > self.max_width {
            self.format_result = FormattingResult::TooWide;
        }
    }

    fn add_literal(&mut self, text: &str, skip_if_does_not_fit: bool) {
        let w = render_length(text);
        if !self.literal_fits_width(w) {
            if skip_if_does_not_fit {
                return;
            }
            self.add_newline();
            if self.format_result != FormattingResult::Success {
                return;
            }
        }
        self.result.push_str(text);
        self.line_length = self.line_length.saturating_add(w);
        if self.line_length > self.max_width {
            self.format_result = FormattingResult::TooWide;
        }
    }

    fn add_space(&mut self) {
        self.add_literal(" ", true);
    }

    fn format_component(&mut self, inline_mode: InlineMode) {
        if self.component_idx >= self.components.len() {
            return;
        }
        let component = &self.components[self.component_idx];
        match component.typ {
            ComponentType::BracketOpen => {
                let before_line_length = self.line_length;
                let prev_byte = self.result.as_bytes().last().copied();
                if component.text == "{" {
                    self.depth = self.depth.saturating_add(self.indentation_size);
                } else {
                    self.depth = self.depth.saturating_add(1);
                }
                self.add_literal(&component.text, false);
                if inline_mode == InlineMode::Standard {
                    let mut peek_depth: usize = 0;
                    let mut render_size: usize = self.line_length;
                    let mut peek_idx: usize = self.component_idx + 1;
                    let mut inline_child_mode = InlineMode::Standard;
                    while peek_idx < self.components.len() && render_size <= self.max_width {
                        let peek = &self.components[peek_idx];
                        if peek.typ == ComponentType::BracketOpen {
                            peek_depth += 1;
                        } else if peek.typ == ComponentType::BracketClose {
                            if peek_depth == 0 {
                                if render_size + 1 < self.max_width {
                                    inline_child_mode = InlineMode::InlinedSingleLine;
                                }
                                break;
                            }
                            peek_depth = peek_depth.saturating_sub(1);
                        }
                        render_size = render_size.saturating_add(render_length(&peek.text));
                        if matches!(peek.typ, ComponentType::Comma | ComponentType::Colon) {
                            render_size = render_size.saturating_add(1);
                        }
                        peek_idx += 1;
                    }
                    if component.text == "[" {
                        // For arrays, always inline unless there are nested objects inside.
                        for scan_idx in (self.component_idx + 1)..self.components.len() {
                            let peek = &self.components[scan_idx];
                            let mut peek_depth: usize = 0;
                            if peek.typ == ComponentType::BracketOpen {
                                if peek.text == "{" {
                                    peek_idx = scan_idx;
                                    break;
                                }
                                peek_depth += 1;
                            }
                            if peek.typ == ComponentType::BracketClose {
                                if peek_depth == 0 {
                                    inline_child_mode = InlineMode::InlinedMultiLine;
                                    peek_idx = scan_idx;
                                    break;
                                }
                            }
                        }
                    }

                    // For arrays of objects that appear mid-line, do not inline the entire array onto a single line.
                    // This matches the shipped duckbox JSON formatter behavior.
                    let next_is_object = self.component_idx + 1 < self.components.len()
                        && self.components[self.component_idx + 1].typ == ComponentType::BracketOpen
                        && self.components[self.component_idx + 1].text == "{";
                    if component.text == "["
                        && next_is_object
                        && before_line_length > 0
                        && inline_child_mode == InlineMode::InlinedSingleLine
                    {
                        inline_child_mode = InlineMode::Standard;
                    }

                    if component.text == "{"
                        && prev_byte == Some(b'[')
                        && inline_child_mode == InlineMode::InlinedSingleLine
                    {
                        inline_child_mode = InlineMode::Standard;
                    }

                    if inline_child_mode != InlineMode::Standard {
                        for inline_idx in (self.component_idx + 1)..=peek_idx {
                            if self.format_result != FormattingResult::Success {
                                return;
                            }
                            if inline_child_mode == InlineMode::InlinedMultiLine && inline_idx + 1 <= peek_idx {
                                let next = &self.components[inline_idx + 1];
                                if matches!(next.typ, ComponentType::Comma | ComponentType::BracketClose) {
                                    let cur = &self.components[inline_idx];
                                    let mut combined = String::with_capacity(
                                        cur.text.len().saturating_add(next.text.len()),
                                    );
                                    combined.push_str(&cur.text);
                                    combined.push_str(&next.text);
                                    if !self.literal_fits(&combined) {
                                        self.add_newline();
                                    }
                                }
                            }
                            self.component_idx = inline_idx;
                            self.format_component(inline_child_mode);
                        }
                        self.component_idx = peek_idx;
                        return;
                    }

                    if self.mode == FormattingMode::CompactVertical {
                        if self.component_idx + 1 < self.components.len()
                            && self.components[self.component_idx + 1].typ == ComponentType::BracketOpen
                        {
                            return;
                        }
                    }

                    if self.mode == FormattingMode::Standard && component.text == "[" && next_is_object
                        && before_line_length > 0
                    {
                        return;
                    }
                    if self.mode == FormattingMode::Standard
                        && component.text == "{"
                        && prev_byte == Some(b'[')
                        && before_line_length > 0
                    {
                        return;
                    }
                    self.add_newline();
                }
            }
            ComponentType::BracketClose => {
                let depth_diff = if component.text == "}" {
                    self.indentation_size
                } else {
                    1
                };
                self.depth = self.depth.saturating_sub(depth_diff);
                if inline_mode == InlineMode::Standard {
                    self.add_newline();
                }
                self.add_literal(&component.text, false);
            }
            ComponentType::Comma | ComponentType::Colon => {
                self.add_literal(&component.text, false);
                let always_inline = if self.mode == FormattingMode::CompactHorizontal {
                    false
                } else {
                    component.typ == ComponentType::Colon
                };
                if inline_mode != InlineMode::Standard || always_inline {
                    self.add_space();
                } else {
                    if self.mode != FormattingMode::Standard {
                        let mut peek_depth: usize = 0;
                        let mut render_size: usize = self.line_length.saturating_add(1);
                        let mut peek_idx: usize = self.component_idx + 1;
                        let mut inline_comma = false;
                        while peek_idx < self.components.len() && render_size <= self.max_width {
                            let peek = &self.components[peek_idx];
                            if peek.typ == ComponentType::BracketOpen {
                                peek_depth += 1;
                            } else if peek.typ == ComponentType::BracketClose {
                                if peek_depth == 0 {
                                    inline_comma = render_size + 1 < self.max_width;
                                    break;
                                }
                                peek_depth = peek_depth.saturating_sub(1);
                            }
                            if peek_depth == 0 && peek.typ == ComponentType::Comma {
                                inline_comma = render_size + 2 <= self.max_width;
                                break;
                            }
                            render_size = render_size.saturating_add(render_length(&peek.text));
                            if matches!(peek.typ, ComponentType::Comma | ComponentType::Colon) {
                                render_size = render_size.saturating_add(1);
                            }
                            peek_idx += 1;
                        }
                        if inline_comma {
                            self.add_space();
                            for inline_idx in (self.component_idx + 1)..peek_idx {
                                self.component_idx = inline_idx;
                                self.format_component(InlineMode::InlinedSingleLine);
                                if self.format_result != FormattingResult::Success {
                                    return;
                                }
                            }
                            self.component_idx = peek_idx.saturating_sub(1);
                            return;
                        }
                    }
                    self.add_newline();
                }
            }
            ComponentType::NullValue | ComponentType::Literal => {
                self.add_literal(&component.text, false);
            }
        }
    }
}

fn try_format(
    components: &[Component],
    mode: FormattingMode,
    max_rows: usize,
    max_width: usize,
    indentation_size: usize,
) -> (FormattingResult, String) {
    let mut state = FormatState {
        mode,
        result: String::new(),
        component_idx: 0,
        row_count: 0,
        line_length: 0,
        depth: 0,
        max_rows,
        max_width,
        indentation_size,
        format_result: FormattingResult::Success,
        components,
    };
    while state.component_idx < state.components.len()
        && state.format_result == FormattingResult::Success
    {
        state.format_component(InlineMode::Standard);
        state.component_idx += 1;
    }
    (state.format_result, state.result)
}

pub fn format_value(value: &str, max_rows: usize, max_width: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let components = process_components(trimmed)?;
    if components
        .first()
        .is_none_or(|c| c.typ != ComponentType::BracketOpen)
    {
        return None;
    }

    let mut indentation_size: usize = 2;
    let (res, out) = try_format(
        &components,
        FormattingMode::Standard,
        max_rows,
        max_width,
        indentation_size,
    );
    if res == FormattingResult::Success {
        return Some(out);
    }

    let mode = if res == FormattingResult::TooWide {
        indentation_size = 1;
        FormattingMode::CompactHorizontal
    } else {
        FormattingMode::CompactVertical
    };
    let (res2, out2) = try_format(&components, mode, max_rows, max_width, indentation_size);
    if res2 == FormattingResult::Success {
        return Some(out2);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_nested_object_with_null_value() {
        let formatted = format_value(r#"{"a":1,"b":[null,{"c":"x"}]}"#, 20, 80).unwrap();
        assert!(formatted.contains("\"a\": 1"));
        assert!(formatted.contains("null"));
        assert!(formatted.contains("\"c\": \"x\""));
    }

    #[test]
    fn returns_none_for_invalid_or_scalar_values() {
        assert!(format_value("not json", 20, 80).is_none());
        assert!(format_value(r#""string""#, 20, 80).is_none());
        assert!(format_value(r#"{"unterminated": [1, 2]"#, 20, 80).is_none());
    }

    #[test]
    fn respects_row_limit() {
        let value = r#"{"a":[{"b":1},{"c":2},{"d":3}]}"#;
        assert!(format_value(value, 1, 12).is_none());
    }
}
