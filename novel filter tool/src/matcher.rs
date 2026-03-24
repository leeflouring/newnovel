use crate::model::{AppConfig, FileRecord, GroupMember, MatchEdge, MatchGroup, MatchType};
use crate::normalize::{
    common_prefix_len, contains_relation, dice_score, file_length_hint, jaccard_score,
    normalized_levenshtein, shared_sorted,
};
use crate::recommend::choose_keep;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn build_groups(records: &[FileRecord], config: &AppConfig) -> (Vec<MatchGroup>, usize, usize) {
    if records.len() < 2 {
        return (Vec::new(), 0, 0);
    }

    let lengths = records.iter().map(file_length_hint).collect::<Vec<_>>();
    let mut doc_freq = HashMap::<String, usize>::new();
    for record in records {
        let mut seen = HashSet::new();
        for gram in &record.grams {
            if seen.insert(gram) {
                *doc_freq.entry(gram.clone()).or_default() += 1;
            }
        }
    }

    let max_doc_freq = ((records.len() as f32 * config.max_doc_freq_ratio).ceil() as usize).max(2);
    let selected_grams = records
        .iter()
        .map(|record| select_rare_grams(record, &doc_freq, max_doc_freq, config.top_rare_grams))
        .collect::<Vec<_>>();

    let mut buckets = HashMap::<String, Vec<usize>>::new();
    for (idx, grams) in selected_grams.iter().enumerate() {
        for gram in grams {
            buckets.entry(gram.clone()).or_default().push(idx);
        }
    }

    let mut candidate_hits = HashMap::<(usize, usize), u16>::new();
    for ids in buckets.values() {
        if ids.len() > config.max_bucket_size {
            continue;
        }
        for left in 0..ids.len() {
            for right in (left + 1)..ids.len() {
                let a = ids[left];
                let b = ids[right];
                if too_far_apart(lengths[a], lengths[b]) {
                    continue;
                }
                let pair = if a < b { (a, b) } else { (b, a) };
                *candidate_hits.entry(pair).or_default() += 1;
            }
        }
    }

    let candidate_pairs = candidate_hits.len();
    let pairs = candidate_hits
        .into_iter()
        .filter(|(_, hits)| *hits as usize >= config.min_shared_grams.saturating_sub(1).max(1))
        .map(|(pair, _)| pair)
        .collect::<Vec<_>>();
    let compared_pairs = pairs.len();

    let edges = pairs
        .par_iter()
        .filter_map(|(left, right)| compare_pair(&records[*left], &records[*right], config))
        .collect::<Vec<_>>();

    if edges.is_empty() {
        return (Vec::new(), candidate_pairs, compared_pairs);
    }

    let mut dsu = DisjointSet::new(records.len());
    for edge in &edges {
        dsu.union(edge.left_id, edge.right_id);
    }

    let mut groups_by_root = BTreeMap::<usize, Vec<usize>>::new();
    for record in records {
        let root = dsu.find(record.id);
        groups_by_root.entry(root).or_default().push(record.id);
    }

    let mut evidence_by_root = HashMap::<usize, Vec<MatchEdge>>::new();
    for edge in edges {
        let root = dsu.find(edge.left_id);
        evidence_by_root.entry(root).or_default().push(edge);
    }

    let mut groups = Vec::new();
    for member_ids in groups_by_root.into_values() {
        if member_ids.len() < 2 {
            continue;
        }
        let refs = member_ids
            .iter()
            .map(|id| &records[*id])
            .collect::<Vec<_>>();
        let evidence = evidence_by_root
            .remove(&dsu.find(member_ids[0]))
            .unwrap_or_default();
        let (keep, reason) = choose_keep(&refs, config);
        let mut members = refs
            .iter()
            .map(|record| GroupMember {
                file_id: record.id,
                path: record.path.clone(),
                relative_path: record.relative_path.clone(),
                file_name: record.file_name.clone(),
                extension: record.extension.clone(),
                size: record.size,
                modified_ms: record.modified_ms,
                normalized_name: record.normalized_name.clone(),
                keep_recommended: record.id == keep.id,
                recommendation_reason: if record.id == keep.id {
                    reason.clone()
                } else {
                    format!("建议清理：组内优先保留 {}", keep.file_name)
                },
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|member| {
            (
                !member.keep_recommended,
                std::cmp::Reverse(member.size),
                std::cmp::Reverse(member.modified_ms),
                member.file_name.clone(),
            )
        });

        let result_type = classify_group(&evidence);
        let summary_reason = build_group_reason(&evidence);
        let max_score = evidence.iter().map(|edge| edge.score).fold(0.0, f32::max);
        groups.push(MatchGroup {
            group_id: groups.len() + 1,
            result_type,
            summary_reason,
            max_score,
            recommended_keep_id: keep.id,
            members,
            evidence,
        });
    }

    groups.sort_by(|a, b| {
        b.max_score
            .partial_cmp(&a.max_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.group_id.cmp(&b.group_id))
    });
    for (index, group) in groups.iter_mut().enumerate() {
        group.group_id = index + 1;
    }

    (groups, candidate_pairs, compared_pairs)
}

fn select_rare_grams(
    record: &FileRecord,
    doc_freq: &HashMap<String, usize>,
    max_doc_freq: usize,
    top_k: usize,
) -> Vec<String> {
    let mut grams = record
        .grams
        .iter()
        .map(|gram| {
            (
                doc_freq.get(gram).copied().unwrap_or(usize::MAX),
                gram.clone(),
            )
        })
        .filter(|(freq, _)| *freq <= max_doc_freq)
        .collect::<Vec<_>>();
    grams.sort_by_key(|(freq, gram)| (*freq, gram.len(), gram.clone()));
    let mut selected = grams
        .into_iter()
        .take(top_k.max(1))
        .map(|(_, gram)| gram)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        selected = record.grams.iter().take(top_k.max(1)).cloned().collect();
    }
    selected
}

fn too_far_apart(a: usize, b: usize) -> bool {
    let max_len = a.max(b);
    let min_len = a.min(b);
    max_len > min_len + (max_len / 2).max(3)
}

fn compare_pair(left: &FileRecord, right: &FileRecord, config: &AppConfig) -> Option<MatchEdge> {
    if left.compact_name.is_empty() || right.compact_name.is_empty() {
        return None;
    }

    if left.compact_name == right.compact_name {
        return Some(MatchEdge {
            left_id: left.id,
            right_id: right.id,
            score: 1.0,
            result_type: MatchType::Exact,
            shared_tokens: left.tokens.iter().take(6).cloned().collect(),
            shared_grams: left.grams.iter().take(6).cloned().collect(),
            reasons: vec!["规范化后文件名完全一致".to_string()],
        });
    }

    let (gram_score, shared_grams) = dice_score(&left.grams, &right.grams);
    let (token_score, shared_tokens) = jaccard_score(&left.tokens, &right.tokens);
    let edit_score = normalized_levenshtein(&left.compact_name, &right.compact_name);
    let shared_gram_count = shared_sorted(&left.grams, &right.grams, 6).0;
    let shared_token_count = shared_sorted(&left.tokens, &right.tokens, 6).0;
    if shared_gram_count < config.min_shared_grams && shared_token_count == 0 {
        return None;
    }

    let mut score = gram_score * 0.55 + token_score * 0.25 + edit_score * 0.20;
    let prefix = common_prefix_len(&left.compact_name, &right.compact_name);
    let contains = contains_relation(&left.compact_name, &right.compact_name);
    if prefix >= 4 {
        score += 0.03;
    }
    if contains {
        score += 0.05;
    }
    if left.extension == right.extension {
        score += 0.01;
    }
    score = score.clamp(0.0, 1.0);

    let result_type = if score >= config.similarity_threshold {
        MatchType::Similar
    } else if score >= config.review_threshold {
        MatchType::Review
    } else {
        return None;
    };

    let mut reasons = vec![format!(
        "综合得分 {:.3}（n-gram {:.3} / token {:.3} / 编辑距离 {:.3}）",
        score, gram_score, token_score, edit_score
    )];
    if !shared_grams.is_empty() {
        reasons.push(format!("共享片段：{}", shared_grams.join(" / ")));
    }
    if !shared_tokens.is_empty() {
        reasons.push(format!("共享词：{}", shared_tokens.join(" / ")));
    }
    if prefix >= 4 {
        reasons.push(format!("公共前缀长度 {}", prefix));
    }
    if contains_relation(&left.compact_name, &right.compact_name) {
        reasons.push("一个名称包含另一个名称主体".to_string());
    }

    Some(MatchEdge {
        left_id: left.id,
        right_id: right.id,
        score,
        result_type,
        shared_tokens,
        shared_grams,
        reasons,
    })
}

fn classify_group(edges: &[MatchEdge]) -> MatchType {
    if edges
        .iter()
        .all(|edge| edge.result_type == MatchType::Exact)
    {
        MatchType::Exact
    } else if edges
        .iter()
        .any(|edge| edge.result_type == MatchType::Similar)
    {
        MatchType::Similar
    } else {
        MatchType::Review
    }
}

fn build_group_reason(edges: &[MatchEdge]) -> String {
    if edges.is_empty() {
        return "候选组".to_string();
    }
    let best = edges
        .iter()
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("non-empty edges");
    best.reasons
        .first()
        .cloned()
        .unwrap_or_else(|| best.result_type.label().to_string())
}

#[derive(Debug)]
struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, node: usize) -> usize {
        if self.parent[node] != node {
            let root = self.find(self.parent[node]);
            self.parent[node] = root;
        }
        self.parent[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left_root = self.find(left);
        let right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.rank[left_root] < self.rank[right_root] {
            self.parent[left_root] = right_root;
        } else if self.rank[left_root] > self.rank[right_root] {
            self.parent[right_root] = left_root;
        } else {
            self.parent[right_root] = left_root;
            self.rank[left_root] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize_filename;

    fn record(id: usize, name: &str) -> FileRecord {
        let stopwords = AppConfig::default().sanitized_stopwords();
        let normalized = normalize_filename(name, &stopwords);
        FileRecord {
            id,
            path: format!("C:/tmp/{name}.txt").into(),
            relative_path: format!("{name}.txt").into(),
            file_name: format!("{name}.txt"),
            stem: name.to_string(),
            extension: "txt".to_string(),
            size: 1,
            modified_ms: id as i64,
            normalized_name: normalized.normalized,
            compact_name: normalized.compact,
            tokens: normalized.tokens,
            grams: normalized.grams,
        }
    }

    #[test]
    fn groups_similar_books() {
        let config = AppConfig::default();
        let records = vec![
            record(0, "遮天 精校版"),
            record(1, "遮天"),
            record(2, "凡人修仙传"),
        ];
        let (groups, candidates, compared) = build_groups(&records, &config);
        assert!(candidates >= 1);
        assert!(compared >= 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }
}
