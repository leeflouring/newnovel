use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Exact,
    Similar,
    Review,
}

impl MatchType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "完全重名",
            Self::Similar => "高相似",
            Self::Review => "待复核",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub scan_dirs: Vec<PathBuf>,
    pub extensions: Vec<String>,
    pub recursive: bool,
    pub include_hidden: bool,
    pub similarity_threshold: f32,
    pub review_threshold: f32,
    pub stopwords: Vec<String>,
    pub max_doc_freq_ratio: f32,
    pub max_bucket_size: usize,
    pub top_rare_grams: usize,
    pub min_shared_grams: usize,
    pub extension_priority: Vec<String>,
    pub ui_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scan_dirs: Vec::new(),
            extensions: vec!["txt", "doc", "docx", "epub"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            recursive: true,
            include_hidden: false,
            similarity_threshold: 0.80,
            review_threshold: 0.64,
            stopwords: vec![
                "完结",
                "精校",
                "精校版",
                "校对",
                "全本",
                "全集",
                "番外",
                "修订版",
                "插图版",
                "完整版",
                "文字版",
                "典藏版",
                "精编版",
                "精排版",
                "未删节",
                "最新修订",
                "作者",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            max_doc_freq_ratio: 0.08,
            max_bucket_size: 96,
            top_rare_grams: 8,
            min_shared_grams: 2,
            extension_priority: vec!["epub", "txt", "docx", "doc"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            ui_port: 8765,
        }
    }
}

impl AppConfig {
    pub fn normalized_extensions(&self) -> Vec<String> {
        let mut items = self
            .extensions
            .iter()
            .filter_map(|ext| {
                let cleaned = ext.trim().trim_start_matches('.').to_ascii_lowercase();
                (!cleaned.is_empty()).then_some(cleaned)
            })
            .collect::<Vec<_>>();
        items.sort();
        items.dedup();
        items
    }

    pub fn extension_rank(&self, extension: &str) -> usize {
        self.extension_priority
            .iter()
            .position(|item| item.eq_ignore_ascii_case(extension))
            .unwrap_or(self.extension_priority.len())
    }

    pub fn sanitized_stopwords(&self) -> Vec<String> {
        let mut items = self
            .stopwords
            .iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        items.sort();
        items.dedup();
        items
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: usize,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub file_name: String,
    pub stem: String,
    pub extension: String,
    pub size: u64,
    pub modified_ms: i64,
    pub normalized_name: String,
    pub compact_name: String,
    pub tokens: Vec<String>,
    pub grams: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchEdge {
    pub left_id: usize,
    pub right_id: usize,
    pub score: f32,
    pub result_type: MatchType,
    pub shared_tokens: Vec<String>,
    pub shared_grams: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub file_id: usize,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub file_name: String,
    pub extension: String,
    pub size: u64,
    pub modified_ms: i64,
    pub normalized_name: String,
    pub keep_recommended: bool,
    pub recommendation_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchGroup {
    pub group_id: usize,
    pub result_type: MatchType,
    pub summary_reason: String,
    pub max_score: f32,
    pub recommended_keep_id: usize,
    pub members: Vec<GroupMember>,
    pub evidence: Vec<MatchEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanSummary {
    pub scanned_files: usize,
    pub candidate_pairs: usize,
    pub compared_pairs: usize,
    pub exact_groups: usize,
    pub near_groups: usize,
    pub review_groups: usize,
    pub matched_files: usize,
    pub warnings: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub config: AppConfig,
    pub groups: Vec<MatchGroup>,
    pub summary: ScanSummary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LatestScanState {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub all_members: std::collections::HashSet<PathBuf>,
    pub deletable: std::collections::HashSet<PathBuf>,
    pub keep_recommended: std::collections::HashSet<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressSnapshot {
    pub running: bool,
    pub cancelled: bool,
    pub stage: String,
    pub message: String,
    pub scanned_files: usize,
    pub candidate_pairs: usize,
    pub compared_pairs: usize,
    pub groups: usize,
    pub finished_report_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteResult {
    pub requested_count: usize,
    pub deleted_count: usize,
    pub deleted_paths: Vec<PathBuf>,
    pub log_path: Option<PathBuf>,
    pub log_write_warning: Option<String>,
}

impl DeleteResult {
    pub fn failed_count(&self) -> usize {
        self.requested_count.saturating_sub(self.deleted_count)
    }

    pub fn is_partial(&self) -> bool {
        self.failed_count() > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub scan_dirs: Option<Vec<String>>,
    pub extensions: Option<Vec<String>>,
    pub similarity_threshold: Option<f32>,
    pub review_threshold: Option<f32>,
    pub stopwords: Option<Vec<String>>,
    pub recursive: Option<bool>,
    pub include_hidden: Option<bool>,
}

impl ScanRequest {
    pub fn apply_to(self, mut config: AppConfig) -> AppConfig {
        if let Some(values) = self.scan_dirs {
            config.scan_dirs = values.into_iter().map(PathBuf::from).collect();
        }
        if let Some(values) = self.extensions {
            config.extensions = values;
        }
        if let Some(value) = self.similarity_threshold {
            config.similarity_threshold = value;
        }
        if let Some(value) = self.review_threshold {
            config.review_threshold = value;
        }
        if let Some(values) = self.stopwords {
            config.stopwords = values;
        }
        if let Some(value) = self.recursive {
            config.recursive = value;
        }
        if let Some(value) = self.include_hidden {
            config.include_hidden = value;
        }
        config
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRequest {
    pub paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn base_config() -> AppConfig {
        AppConfig {
            scan_dirs: vec![PathBuf::from("C:/books")],
            extensions: vec!["txt".into(), "epub".into()],
            stopwords: vec!["完结".into()],
            recursive: true,
            include_hidden: false,
            similarity_threshold: 0.80,
            review_threshold: 0.64,
            ..AppConfig::default()
        }
    }

    #[test]
    fn keeps_existing_lists_when_request_fields_are_none() {
        let merged = ScanRequest {
            scan_dirs: None,
            extensions: None,
            similarity_threshold: None,
            review_threshold: None,
            stopwords: None,
            recursive: None,
            include_hidden: None,
        }
        .apply_to(base_config());

        assert_eq!(merged.scan_dirs, vec![PathBuf::from("C:/books")]);
        assert_eq!(merged.extensions, vec!["txt", "epub"]);
        assert_eq!(merged.stopwords, vec!["完结"]);
    }

    #[test]
    fn clears_lists_when_request_fields_are_present_but_empty() {
        let merged = ScanRequest {
            scan_dirs: Some(vec![]),
            extensions: Some(vec![]),
            similarity_threshold: None,
            review_threshold: None,
            stopwords: Some(vec![]),
            recursive: None,
            include_hidden: None,
        }
        .apply_to(base_config());

        assert!(merged.scan_dirs.is_empty());
        assert!(merged.extensions.is_empty());
        assert!(merged.stopwords.is_empty());
    }

    #[test]
    fn delete_request_deserializes_from_paths_only() {
        let request: DeleteRequest = serde_json::from_str(r#"{"paths":["C:/books/a.txt"]}"#)
            .expect("deserialize delete request");
        assert_eq!(request.paths, vec!["C:/books/a.txt"]);
    }
}
