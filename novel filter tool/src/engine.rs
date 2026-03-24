use crate::matcher::build_groups;
use crate::model::{AppConfig, LatestScanState, ProgressSnapshot, ScanReport, ScanSummary};
use crate::scanner::scan_files;
use anyhow::{Result, anyhow};
use chrono::Local;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

#[derive(Clone)]
pub struct Engine {
    config: Arc<RwLock<AppConfig>>,
    report: Arc<RwLock<Option<ScanReport>>>,
    latest_scan_state: Arc<RwLock<Option<LatestScanState>>>,
    progress: Arc<Mutex<ProgressSnapshot>>,
    cancel_flag: Arc<AtomicBool>,
    background_scan_running: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct ScanRuntime {
    progress: Arc<Mutex<ProgressSnapshot>>,
    cancel_flag: Arc<AtomicBool>,
}

impl Engine {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            report: Arc::new(RwLock::new(None)),
            latest_scan_state: Arc::new(RwLock::new(None)),
            progress: Arc::new(Mutex::new(ProgressSnapshot::default())),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            background_scan_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config.read().expect("config lock poisoned").clone()
    }

    pub fn set_config(&self, config: AppConfig) {
        *self.config.write().expect("config lock poisoned") = config;
    }

    pub fn progress(&self) -> ProgressSnapshot {
        self.progress
            .lock()
            .expect("progress lock poisoned")
            .clone()
    }

    pub fn report(&self) -> Option<ScanReport> {
        self.report.read().expect("report lock poisoned").clone()
    }

    pub fn latest_scan_state(&self) -> Option<LatestScanState> {
        self.latest_scan_state
            .read()
            .expect("latest scan state lock poisoned")
            .clone()
    }

    #[cfg(test)]
    pub fn store_completed_scan_for_tests(&self, report: ScanReport) {
        self.store_completed_scan(report);
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.cancelled = true;
        progress.message = "正在取消扫描...".to_string();
    }

    pub async fn start_scan_background(&self, config: AppConfig) -> Result<()> {
        if self
            .background_scan_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(anyhow!("已有扫描任务正在运行"));
        }

        if let Err(err) = self.prepare_background_scan_state(config.clone()) {
            self.background_scan_running.store(false, Ordering::Release);
            return Err(err);
        }

        let engine = self.clone();
        tokio::spawn(async move {
            let engine_for_job = engine.clone();
            let outcome =
                tokio::task::spawn_blocking(move || engine_for_job.execute_scan(config)).await;
            match outcome {
                Ok(Ok(report)) => engine.finish_background_success(report),
                Ok(Err(err)) => engine.finish_background_error(err.to_string()),
                Err(err) => engine.finish_background_error(format!("后台任务失败: {err}")),
            }
        });

        Ok(())
    }

    pub fn run_scan(&self, config: AppConfig) -> Result<ScanReport> {
        self.prepare_scan_state(config.clone());

        match self.execute_scan(config) {
            Ok(report) => {
                self.finish_success(report.clone());
                Ok(report)
            }
            Err(err) => {
                self.finish_error(err.to_string());
                Err(err)
            }
        }
    }

    fn execute_scan(&self, config: AppConfig) -> Result<ScanReport> {
        let started = Instant::now();
        let runtime = ScanRuntime {
            progress: Arc::clone(&self.progress),
            cancel_flag: Arc::clone(&self.cancel_flag),
        };

        runtime.update_stage("scan", "正在扫描目录与收集文件名...");
        let (roots, records, warnings) = scan_files(&config, Some(&runtime))?;

        runtime.update_stage("match", "正在构建候选索引与相似分组...");
        let (groups, candidate_pairs, compared_pairs) = build_groups(&records, &config);
        runtime.update_matching(candidate_pairs, compared_pairs, groups.len());

        let mut summary = ScanSummary {
            scanned_files: records.len(),
            candidate_pairs,
            compared_pairs,
            matched_files: groups.iter().map(|group| group.members.len()).sum(),
            warnings: warnings.len(),
            duration_ms: started.elapsed().as_millis() as u64,
            ..ScanSummary::default()
        };
        for group in &groups {
            match group.result_type {
                crate::model::MatchType::Exact => summary.exact_groups += 1,
                crate::model::MatchType::Similar => summary.near_groups += 1,
                crate::model::MatchType::Review => summary.review_groups += 1,
            }
        }

        Ok(ScanReport {
            generated_at: Local::now().to_rfc3339(),
            roots,
            config,
            groups,
            summary,
            warnings,
        })
    }

    fn prepare_background_scan_state(&self, config: AppConfig) -> Result<()> {
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        if progress.running {
            return Err(anyhow!("已有扫描任务正在运行"));
        }

        self.set_config(config);
        self.cancel_flag.store(false, Ordering::Relaxed);
        *progress = ProgressSnapshot {
            running: true,
            cancelled: false,
            stage: "prepare".to_string(),
            message: "准备开始扫描...".to_string(),
            ..ProgressSnapshot::default()
        };
        drop(progress);
        self.clear_completed_scan_state();
        Ok(())
    }

    fn prepare_scan_state(&self, config: AppConfig) {
        self.set_config(config);
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.reset_progress();
        self.clear_completed_scan_state();
    }

    pub fn clear_completed_scan_state(&self) {
        *self.report.write().expect("report lock poisoned") = None;
        *self
            .latest_scan_state
            .write()
            .expect("latest scan state lock poisoned") = None;
        self.progress
            .lock()
            .expect("progress lock poisoned")
            .finished_report_available = false;
    }

    fn store_completed_scan(&self, report: ScanReport) {
        let latest_scan_state = build_latest_scan_state(&report);
        *self.report.write().expect("report lock poisoned") = Some(report);
        *self
            .latest_scan_state
            .write()
            .expect("latest scan state lock poisoned") = Some(latest_scan_state);
    }

    fn reset_progress(&self) {
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        *progress = ProgressSnapshot {
            running: true,
            cancelled: false,
            stage: "prepare".to_string(),
            message: "准备开始扫描...".to_string(),
            ..ProgressSnapshot::default()
        };
    }

    fn finish_success(&self, report: ScanReport) {
        let groups = report.groups.len();
        let summary = report.summary.clone();
        self.store_completed_scan(report);
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.running = false;
        progress.cancelled = false;
        progress.stage = "done".to_string();
        progress.message = format!(
            "扫描完成：{} 个分组，{} 个文件",
            groups, summary.scanned_files
        );
        progress.scanned_files = summary.scanned_files;
        progress.candidate_pairs = summary.candidate_pairs;
        progress.compared_pairs = summary.compared_pairs;
        progress.groups = groups;
        progress.finished_report_available = true;
    }

    fn finish_background_success(&self, report: ScanReport) {
        self.finish_success(report);
        self.background_scan_running.store(false, Ordering::Release);
    }

    fn finish_error(&self, message: String) {
        let cancelled = self.cancel_flag.load(Ordering::Relaxed);
        self.clear_completed_scan_state();
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.running = false;
        progress.cancelled = cancelled;
        progress.stage = if cancelled { "cancelled" } else { "error" }.to_string();
        progress.message = if cancelled {
            "扫描已取消".to_string()
        } else {
            sanitize_scan_error_message(&message)
        };
        progress.finished_report_available = false;
    }

    fn finish_background_error(&self, message: String) {
        self.finish_error(message);
        self.background_scan_running.store(false, Ordering::Release);
    }
}

fn sanitize_scan_error_message(message: &str) -> String {
    if message.starts_with("扫描目录不存在或无法访问:")
        || message.starts_with("后台任务失败: 扫描目录不存在或无法访问:")
    {
        "扫描失败，请检查扫描目录后重试".to_string()
    } else if message.starts_with("请至少提供一个扫描目录") {
        "请至少选择一个扫描目录".to_string()
    } else if message.starts_with("扫描已取消") {
        "扫描已取消".to_string()
    } else {
        "扫描失败，请重试".to_string()
    }
}

fn build_latest_scan_state(report: &ScanReport) -> LatestScanState {
    let mut state = LatestScanState {
        generated_at: report.generated_at.clone(),
        roots: report.roots.clone(),
        ..LatestScanState::default()
    };

    for group in &report.groups {
        for member in &group.members {
            let path = member.path.clone();
            state.all_members.insert(path.clone());
            if member.keep_recommended {
                state.keep_recommended.insert(path);
            } else {
                state.deletable.insert(path);
            }
        }
    }

    state
}

impl ScanRuntime {
    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::Relaxed)
    }

    pub fn update_stage(&self, stage: &str, message: &str) {
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.stage = stage.to_string();
        progress.message = message.to_string();
    }

    pub fn update_scan(&self, scanned_files: usize, warnings: usize) {
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.scanned_files = scanned_files;
        progress.message = format!("已扫描 {} 个文件，警告 {} 条", scanned_files, warnings);
    }

    pub fn update_matching(&self, candidate_pairs: usize, compared_pairs: usize, groups: usize) {
        let mut progress = self.progress.lock().expect("progress lock poisoned");
        progress.stage = "group".to_string();
        progress.candidate_pairs = candidate_pairs;
        progress.compared_pairs = compared_pairs;
        progress.groups = groups;
        progress.message = format!(
            "已生成 {} 个候选对，实际比较 {} 对，形成 {} 个分组",
            candidate_pairs, compared_pairs, groups
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AppConfig, GroupMember, MatchEdge, MatchGroup, MatchType, ScanReport, ScanSummary,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::sync::Barrier;
    use tokio::task::yield_now;
    use tokio::time::timeout;

    fn report_with_keep_and_candidate() -> ScanReport {
        ScanReport {
            generated_at: "2026-03-24T10:00:00+08:00".into(),
            roots: vec![PathBuf::from("C:/books")],
            config: AppConfig::default(),
            groups: vec![MatchGroup {
                group_id: 1,
                result_type: MatchType::Similar,
                summary_reason: "same title".into(),
                max_score: 0.95,
                recommended_keep_id: 1,
                members: vec![
                    GroupMember {
                        file_id: 1,
                        path: PathBuf::from("C:/books/keep.txt"),
                        relative_path: PathBuf::from("keep.txt"),
                        file_name: "keep.txt".into(),
                        extension: "txt".into(),
                        size: 1,
                        modified_ms: 1,
                        normalized_name: "keep".into(),
                        keep_recommended: true,
                        recommendation_reason: "largest".into(),
                    },
                    GroupMember {
                        file_id: 2,
                        path: PathBuf::from("C:/books/delete.txt"),
                        relative_path: PathBuf::from("delete.txt"),
                        file_name: "delete.txt".into(),
                        extension: "txt".into(),
                        size: 1,
                        modified_ms: 2,
                        normalized_name: "delete".into(),
                        keep_recommended: false,
                        recommendation_reason: "duplicate".into(),
                    },
                ],
                evidence: Vec::<MatchEdge>::new(),
            }],
            summary: ScanSummary::default(),
            warnings: Vec::new(),
        }
    }

    fn scan_config_for(root: &std::path::Path) -> AppConfig {
        AppConfig {
            scan_dirs: vec![root.to_path_buf()],
            extensions: vec!["txt".into()],
            ..AppConfig::default()
        }
    }

    #[test]
    fn builds_latest_scan_state_from_report() {
        let state = build_latest_scan_state(&report_with_keep_and_candidate());
        assert!(
            state
                .all_members
                .contains(&PathBuf::from("C:/books/keep.txt"))
        );
        assert!(
            state
                .all_members
                .contains(&PathBuf::from("C:/books/delete.txt"))
        );
        assert!(
            state
                .keep_recommended
                .contains(&PathBuf::from("C:/books/keep.txt"))
        );
        assert!(
            state
                .deletable
                .contains(&PathBuf::from("C:/books/delete.txt"))
        );
        assert!(
            !state
                .deletable
                .contains(&PathBuf::from("C:/books/keep.txt"))
        );
    }

    #[test]
    fn store_completed_scan_updates_report_and_latest_scan_state() {
        let engine = Engine::new(AppConfig::default());
        let report = report_with_keep_and_candidate();

        engine.store_completed_scan(report.clone());

        let stored_report = engine.report().expect("stored report");
        let stored_state = engine
            .latest_scan_state()
            .expect("stored latest scan state");
        assert_eq!(stored_report.generated_at, report.generated_at);
        assert_eq!(stored_state.generated_at, report.generated_at);
        assert_eq!(stored_state.roots, report.roots);
        assert!(
            stored_state
                .keep_recommended
                .contains(&PathBuf::from("C:/books/keep.txt"))
        );
        assert!(
            stored_state
                .deletable
                .contains(&PathBuf::from("C:/books/delete.txt"))
        );
    }

    #[test]
    fn prepare_scan_state_clears_previous_completed_state() {
        let engine = Engine::new(AppConfig::default());
        engine.store_completed_scan(report_with_keep_and_candidate());

        engine.prepare_scan_state(AppConfig::default());

        assert!(engine.report().is_none());
        assert!(engine.latest_scan_state().is_none());
        let progress = engine.progress();
        assert!(progress.running);
        assert!(!progress.finished_report_available);
    }

    #[test]
    fn finish_error_clears_previous_completed_state() {
        let engine = Engine::new(AppConfig::default());
        engine.store_completed_scan(report_with_keep_and_candidate());

        engine.finish_error("boom".into());

        assert!(engine.report().is_none());
        assert!(engine.latest_scan_state().is_none());
        let progress = engine.progress();
        assert_eq!(progress.stage, "error");
        assert!(!progress.finished_report_available);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_background_starts_allow_only_one_scan() {
        let engine = Engine::new(AppConfig::default());
        let root = tempdir().expect("tempdir");
        fs::write(root.path().join("novel.txt"), b"sample novel").expect("write sample file");
        let config = scan_config_for(root.path());
        let barrier = Arc::new(Barrier::new(3));

        let first_engine = engine.clone();
        let first_config = config.clone();
        let first_barrier = Arc::clone(&barrier);
        let first = tokio::spawn(async move {
            first_barrier.wait().await;
            first_engine.start_scan_background(first_config).await
        });

        let second_engine = engine.clone();
        let second_barrier = Arc::clone(&barrier);
        let second = tokio::spawn(async move {
            second_barrier.wait().await;
            second_engine.start_scan_background(config).await
        });

        barrier.wait().await;
        let first = first.await.expect("first start join");
        let second = second.await.expect("second start join");

        assert!(first.is_ok() ^ second.is_ok());
        let error_message = first
            .as_ref()
            .err()
            .or_else(|| second.as_ref().err())
            .map(ToString::to_string);
        assert_eq!(error_message.as_deref(), Some("已有扫描任务正在运行"));

        timeout(Duration::from_secs(5), async {
            loop {
                if !engine.progress().running {
                    break;
                }
                yield_now().await;
            }
        })
        .await
        .expect("background scan should finish");

        let progress = engine.progress();
        assert!(!progress.running);
        assert!(progress.finished_report_available);
        assert!(engine.report().is_some());
        assert!(engine.latest_scan_state().is_some());
    }
}
