fn ascii_to_lower(byte: u8) -> u8 {
    if (b'A'..=b'Z').contains(&byte) {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}

fn ascii_to_upper(byte: u8) -> u8 {
    if (b'a'..=b'z').contains(&byte) {
        byte - (b'a' - b'A')
    } else {
        byte
    }
}

fn lower_ascii(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .iter()
        .copied()
        .map(ascii_to_lower)
        .collect()
}

fn ci_less_than(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut u1: u8 = 0;
    let mut u2: u8 = 0;

    let mut length = a_bytes.len().min(b_bytes.len());
    if a_bytes.len() != b_bytes.len() {
        length += 1;
    }

    for i in 0..length {
        u1 = *a_bytes.get(i).unwrap_or(&0);
        u2 = *b_bytes.get(i).unwrap_or(&0);
        if ascii_to_upper(u1) != ascii_to_upper(u2) {
            break;
        }
    }

    ascii_to_upper(u1) < ascii_to_upper(u2)
}

fn levenshtein_distance(s1: &str, s2: &str, not_equal_penalty: usize) -> usize {
    let s1 = lower_ascii(s1);
    let s2 = lower_ascii(s2);
    let len1 = s1.len();
    let len2 = s2.len();
    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let width = len1 + 1;
    let height = len2 + 1;
    let mut dist = vec![0usize; width * height];

    let idx = |i: usize, j: usize| j * width + i;
    dist[idx(0, 0)] = 0;
    for i in 0..=len1 {
        dist[idx(i, 0)] = i;
    }
    for j in 0..=len2 {
        dist[idx(0, j)] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let equal = if s1[i - 1] == s2[j - 1] {
                0
            } else {
                not_equal_penalty
            };
            let adjacent_score1 = dist[idx(i - 1, j)] + 1;
            let adjacent_score2 = dist[idx(i, j - 1)] + 1;
            let adjacent_score3 = dist[idx(i - 1, j - 1)] + equal;
            dist[idx(i, j)] = adjacent_score1.min(adjacent_score2).min(adjacent_score3);
        }
    }

    dist[idx(len1, len2)]
}

fn similarity_score(s1: &str, s2: &str) -> usize {
    levenshtein_distance(s1, s2, 3)
}

fn normalize_score(score: usize, max_score: usize) -> f64 {
    1.0 - score as f64 / max_score as f64
}

fn top_n_strings(scores: &mut [(String, f64)], n: usize, threshold: f64) -> Vec<String> {
    if scores.is_empty() {
        return Vec::new();
    }
    scores.sort_by(
        |(a_str, a_score), (b_str, b_score)| match b_score.partial_cmp(a_score) {
            Some(std::cmp::Ordering::Equal) | None => {
                if ci_less_than(a_str, b_str) {
                    std::cmp::Ordering::Less
                } else if ci_less_than(b_str, a_str) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            Some(other) => other,
        },
    );

    let mut result = Vec::new();
    result.push(scores[0].0.clone());
    let limit = scores.len().min(n);
    for i in 1..limit {
        if scores[i].1 < threshold {
            break;
        }
        result.push(scores[i].0.clone());
    }
    result
}

fn top_n_strings_levenshtein(
    scores: &[(String, usize)],
    n: usize,
    threshold: usize,
) -> Vec<String> {
    let mut max_score = threshold;
    for (_, score) in scores {
        if *score > max_score {
            max_score = *score;
        }
    }
    let mut normalized: Vec<(String, f64)> = scores
        .iter()
        .map(|(s, score)| (s.clone(), normalize_score(*score, max_score)))
        .collect();
    let normalized_threshold = normalize_score(threshold, max_score);
    top_n_strings(&mut normalized, n, normalized_threshold)
}

pub fn top_n_levenshtein(
    strings: &[String],
    target: &str,
    n: usize,
    threshold: usize,
) -> Vec<String> {
    let mut scores: Vec<(String, usize)> = Vec::with_capacity(strings.len());
    for s in strings {
        if target.len() < s.len() {
            scores.push((s.clone(), similarity_score(&s[..target.len()], target)));
        } else {
            scores.push((s.clone(), similarity_score(s, target)));
        }
    }
    top_n_strings_levenshtein(&scores, n, threshold)
}

pub fn candidates_message(candidates: &[String], prefix: &str) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    result.push('\n');
    result.push_str(prefix);
    result.push_str(": ");
    for (idx, cand) in candidates.iter().enumerate() {
        if idx > 0 {
            result.push_str(", ");
        }
        result.push('"');
        result.push_str(cand);
        result.push('"');
    }
    result
}

pub fn candidates_error_message(strings: &[String], target: &str, prefix: &str) -> String {
    let closest = top_n_levenshtein(strings, target, 5, 5);
    candidates_message(&closest, prefix)
}
