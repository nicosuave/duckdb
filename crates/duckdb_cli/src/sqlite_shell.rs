pub fn strglob(pattern: &str, text: &str) -> bool {
    fn class_match(class: &str, ch: char) -> (bool, usize) {
        // Returns (matched, consumed_chars_from_class_including_closing_bracket_if_present).
        let mut chars = class.chars();
        if chars.next() != Some('[') {
            return (false, 0);
        }
        let mut consumed = 1usize;
        let mut negate = false;
        let mut matched = false;
        if let Some(next) = chars.clone().next() {
            if next == '^' || next == '!' {
                negate = true;
                chars.next();
                consumed += 1;
            }
        }

        let mut prev: Option<char> = None;
        while let Some(c) = chars.next() {
            consumed += c.len_utf8();
            if c == ']' {
                return (if negate { !matched } else { matched }, consumed);
            }
            if c == '-' {
                if let (Some(start), Some(end)) = (prev, chars.clone().next()) {
                    // range start-end
                    let _ = chars.next();
                    consumed += end.len_utf8();
                    if start <= ch && ch <= end {
                        matched = true;
                    }
                    prev = Some(end);
                    continue;
                }
            }
            if c == ch {
                matched = true;
            }
            prev = Some(c);
        }
        // No closing bracket: treat '[' as literal.
        (false, 1)
    }

    fn glob_match(pat: &str, text: &str) -> bool {
        fn advance_one(s: &str) -> Option<&str> {
            let mut it = s.chars();
            it.next()?;
            Some(it.as_str())
        }

        let mut p = pat;
        let mut t = text;
        let mut star_pat: Option<&str> = None;
        let mut star_text: Option<&str> = None;

        loop {
            if p.is_empty() {
                return t.is_empty()
                    || (star_pat.is_some() && {
                        // Retry consuming one more char under the last '*'
                        if let Some(st) = star_text {
                            let mut it = st.chars();
                            let _ = it.next();
                            star_text = Some(it.as_str());
                        }
                        star_text.is_some_and(|st| glob_match(star_pat.unwrap(), st))
                    });
            }
            let pch = p.chars().next().unwrap();
            match pch {
                '*' => {
                    // collapse consecutive '*'
                    while p.starts_with('*') {
                        p = &p[1..];
                    }
                    if p.is_empty() {
                        return true;
                    }
                    star_pat = Some(p);
                    star_text = Some(t);
                    continue;
                }
                '?' => {
                    if t.is_empty() {
                        if let (Some(sp), Some(st)) = (star_pat, star_text) {
                            star_text = advance_one(st);
                            return star_text.is_some_and(|st2| glob_match(sp, st2));
                        }
                        return false;
                    }
                    t = advance_one(t).unwrap_or("");
                    p = &p[1..];
                }
                '[' => {
                    if t.is_empty() {
                        if let (Some(sp), Some(st)) = (star_pat, star_text) {
                            star_text = advance_one(st);
                            return star_text.is_some_and(|st2| glob_match(sp, st2));
                        }
                        return false;
                    }
                    let (m, consumed) = class_match(p, t.chars().next().unwrap());
                    if consumed == 1 && !m {
                        // literal '['
                        if t.chars().next().unwrap() != '[' {
                            if let (Some(sp), Some(st)) = (star_pat, star_text) {
                                star_text = advance_one(st);
                                return star_text.is_some_and(|st2| glob_match(sp, st2));
                            }
                            return false;
                        }
                        p = &p[1..];
                        t = advance_one(t).unwrap_or("");
                        continue;
                    }
                    if !m {
                        if let (Some(sp), Some(st)) = (star_pat, star_text) {
                            star_text = advance_one(st);
                            return star_text.is_some_and(|st2| glob_match(sp, st2));
                        }
                        return false;
                    }
                    p = &p[consumed..];
                    t = advance_one(t).unwrap_or("");
                }
                other => {
                    if t.chars().next() != Some(other) {
                        if let (Some(sp), Some(st)) = (star_pat, star_text) {
                            star_text = advance_one(st);
                            return star_text.is_some_and(|st2| glob_match(sp, st2));
                        }
                        return false;
                    }
                    p = &p[other.len_utf8()..];
                    t = advance_one(t).unwrap_or("");
                }
            }
        }
    }

    glob_match(pattern, text)
}

pub fn strlike(pattern: &str, text: &str) -> bool {
    fn advance_one(s: &str) -> Option<&str> {
        let mut it = s.chars();
        it.next()?;
        Some(it.as_str())
    }

    fn fold_ascii(c: char) -> char {
        if c.is_ascii_uppercase() {
            c.to_ascii_lowercase()
        } else {
            c
        }
    }

    // SQLite LIKE semantics (best-effort): '%' matches any sequence, '_' matches any single char.
    // In SQLite, LIKE is case-insensitive for ASCII by default; emulate that here.
    let mut p = pattern;
    let mut t = text;
    let mut percent_pat: Option<&str> = None;
    let mut percent_text: Option<&str> = None;

    loop {
        if p.is_empty() {
            return t.is_empty()
                || percent_pat.is_some_and(|pp| {
                    if let Some(st) = percent_text {
                        let mut it = st.chars();
                        let _ = it.next();
                        percent_text = Some(it.as_str());
                    }
                    percent_text.is_some_and(|st2| strlike(pp, st2))
                });
        }
        let pch = p.chars().next().unwrap();
        match pch {
            '%' => {
                while p.starts_with('%') {
                    p = &p[1..];
                }
                if p.is_empty() {
                    return true;
                }
                percent_pat = Some(p);
                percent_text = Some(t);
                continue;
            }
            '_' => {
                if t.is_empty() {
                    if let (Some(pp), Some(st)) = (percent_pat, percent_text) {
                        percent_text = advance_one(st);
                        return percent_text.is_some_and(|st2| strlike(pp, st2));
                    }
                    return false;
                }
                t = advance_one(t).unwrap_or("");
                p = &p[1..];
            }
            other => {
                let Some(tch) = t.chars().next() else {
                    if let (Some(pp), Some(st)) = (percent_pat, percent_text) {
                        percent_text = advance_one(st);
                        return percent_text.is_some_and(|st2| strlike(pp, st2));
                    }
                    return false;
                };
                if fold_ascii(other) != fold_ascii(tch) {
                    if let (Some(pp), Some(st)) = (percent_pat, percent_text) {
                        percent_text = advance_one(st);
                        return percent_text.is_some_and(|st2| strlike(pp, st2));
                    }
                    return false;
                }
                p = &p[other.len_utf8()..];
                t = advance_one(t).unwrap_or("");
            }
        }
    }
}
