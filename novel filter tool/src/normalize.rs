use crate::model::FileRecord;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

static SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

#[derive(Debug, Clone)]
pub struct NormalizedName {
    pub normalized: String,
    pub compact: String,
    pub tokens: Vec<String>,
    pub grams: Vec<String>,
}

pub fn normalize_filename(name: &str, stopwords: &[String]) -> NormalizedName {
    let nfkc = name.nfkc().collect::<String>().to_lowercase();
    let stripped = nfkc
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>();
    let collapsed = SPACE_RE.replace_all(&stripped, " ").trim().to_string();
    let mut cleaned = collapsed;
    let mut normalized_stopwords = stopwords
        .iter()
        .map(|word| word.nfkc().collect::<String>().to_lowercase())
        .filter(|candidate| !candidate.is_empty())
        .collect::<Vec<_>>();
    normalized_stopwords.sort_by(|left, right| {
        right
            .chars()
            .count()
            .cmp(&left.chars().count())
            .then_with(|| left.cmp(right))
    });
    normalized_stopwords.dedup();
    for candidate in normalized_stopwords {
        cleaned = cleaned.replace(&candidate, " ");
    }
    let normalized = SPACE_RE.replace_all(&cleaned, " ").trim().to_string();
    let compact = normalized.replace(' ', "");
    let tokens = unique_sorted(
        normalized
            .split_whitespace()
            .filter(|item| !item.is_empty())
            .map(str::to_string),
    );
    let grams = generate_ngrams(&compact);
    NormalizedName {
        normalized,
        compact,
        tokens,
        grams,
    }
}

pub fn generate_ngrams(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() == 1 {
        return vec![text.to_string()];
    }
    let mut grams = BTreeSet::new();
    for size in [2usize, 3usize] {
        if chars.len() >= size {
            for window in chars.windows(size) {
                grams.insert(window.iter().collect::<String>());
            }
        }
    }
    if grams.is_empty() {
        grams.insert(text.to_string());
    }
    grams.into_iter().collect()
}

pub fn shared_sorted(a: &[String], b: &[String], limit: usize) -> (usize, Vec<String>) {
    let mut i = 0usize;
    let mut j = 0usize;
    let mut count = 0usize;
    let mut sample = Vec::new();
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                count += 1;
                if sample.len() < limit {
                    sample.push(a[i].clone());
                }
                i += 1;
                j += 1;
            }
        }
    }
    (count, sample)
}

pub fn dice_score(a: &[String], b: &[String]) -> (f32, Vec<String>) {
    if a.is_empty() || b.is_empty() {
        return (0.0, Vec::new());
    }
    let (shared, sample) = shared_sorted(a, b, 6);
    let score = (2.0 * shared as f32) / (a.len() + b.len()) as f32;
    (score, sample)
}

pub fn jaccard_score(a: &[String], b: &[String]) -> (f32, Vec<String>) {
    if a.is_empty() || b.is_empty() {
        return (0.0, Vec::new());
    }
    let (shared, sample) = shared_sorted(a, b, 6);
    let union = a.len() + b.len() - shared;
    let score = if union == 0 {
        0.0
    } else {
        shared as f32 / union as f32
    };
    (score, sample)
}

pub fn normalized_levenshtein(a: &str, b: &str) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a_chars = a.chars().collect::<Vec<_>>();
    let b_chars = b.chars().collect::<Vec<_>>();
    if a_chars.is_empty() || b_chars.is_empty() {
        return 0.0;
    }
    let mut prev = (0..=b_chars.len()).collect::<Vec<_>>();
    let mut curr = vec![0usize; b_chars.len() + 1];
    for (i, a_ch) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = usize::from(a_ch != b_ch);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    let distance = prev[b_chars.len()];
    1.0 - distance as f32 / a_chars.len().max(b_chars.len()) as f32
}

pub fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

pub fn contains_relation(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a.contains(b) || b.contains(a))
}

fn unique_sorted(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values = items.into_iter().collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn file_length_hint(record: &FileRecord) -> usize {
    record.compact_name.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppConfig;

    #[test]
    fn default_stopwords_remove_full_jingjiao_version_marker() {
        let normalized =
            normalize_filename("遮天 精校版", &AppConfig::default().sanitized_stopwords());

        assert_eq!(normalized.normalized, "遮天");
        assert_eq!(normalized.compact, "遮天");
        assert_eq!(normalized.tokens, vec!["遮天"]);
    }
}
