# Novel Tool Correctness + Localhost Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the approved correctness bugs and localhost API trust-boundary issues without expanding the product scope.

**Architecture:** Keep the current single-binary Rust CLI + local Web UI architecture. Represent web scan overrides with explicit optional request fields, store a server-owned latest-scan authorization snapshot beside the latest `ScanReport`, and route delete/export/config-save decisions through that server state instead of trusting browser-supplied roots or paths. Prefer small pure helpers and inline module tests so behavior can be verified without a live browser.

**Tech Stack:** Rust 2024, axum 0.8, tokio, walkdir, serde/serde_json, tempfile, trash

---

## File Structure

- Modify: `src/model.rs:208-257`
  - Change `ScanRequest` list fields to `Option<Vec<String>>`
  - Remove client-supplied delete roots from `DeleteRequest`
  - Add latest-scan authorization data types used only by the server
  - Add request-merge tests
- Modify: `src/engine.rs:10-177`
  - Persist latest `ScanReport` together with latest authorization snapshot
  - Expose accessors/helpers for the web layer and tests
  - Clear both report + authorization state when a new scan starts or fails
- Modify: `src/scanner.rs:10-166`
  - Prune hidden directories before descent when `include_hidden = false`
  - Add traversal tests with temp directories
- Modify: `src/safety.rs:1-100`
  - Keep the existing root-based delete flow for CLI use
  - Add a separate latest-scan-authorized delete path for web requests
  - Canonicalize + authorize every requested delete path before deleting anything
  - Downgrade delete-log write failure to a warning on an otherwise successful delete
  - Add authorization/prevalidation tests
- Modify: `src/export.rs:1-86`
  - Add safe export-directory / file-name helpers under app-controlled storage
  - Keep report serialization code reusable
- Modify: `src/config.rs:6-33`
  - Reuse `default_config_path()` from tests/helpers where needed
- Modify: `src/web.rs:16-197, 373-382, 421-455, 575-638`
  - Add resolved config path + safe export dir to `WebState`
  - Harden `/api/scan`, `/api/delete`, `/api/export`, `/api/config`
  - Simplify path-sensitive error messages
  - Update embedded frontend JS to send the new delete/export payloads
  - Add async handler tests in the same module
- Modify: `src/main.rs:109-213`
  - Thread the resolved config path into `web::serve` / `WebState`
  - Remove `scan` config persistence side effect
  - Add CLI behavior tests for `scan` vs `save-config`

> If the local Git Bash/MSYS linker issue still blocks `cargo test`, run the same test filters through PowerShell after the Windows SDK is repaired. The commands below remain the source of truth.

### Task 1: Fix request merge semantics

**Files:**
- Modify: `src/model.rs:208-257`
- Test: `src/model.rs`

- [ ] **Step 1: Write the failing request-merge tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn base_config() -> AppConfig {
        AppConfig {
            scan_dirs: vec![PathBuf::from("C:/books")],
            extensions: vec!["txt".into(), "epub".into()],
            stopwords: vec!["完结".into()],
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test model::tests -- --nocapture`
Expected: FAIL with compile errors because `ScanRequest` still uses `Vec<String>` fields and `DeleteRequest` still requires `roots`.

- [ ] **Step 3: Write the minimal implementation**

```rust
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
        if let Some(values) = self.stopwords {
            config.stopwords = values;
        }
        if let Some(value) = self.similarity_threshold {
            config.similarity_threshold = value;
        }
        if let Some(value) = self.review_threshold {
            config.review_threshold = value;
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
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test model::tests -- --nocapture`
Expected: PASS with the three new request-model tests green.

- [ ] **Step 5: Commit**

```bash
git add src/model.rs
git commit -m "fix: honor explicit empty scan request fields"
```

### Task 2: Prune hidden directories during scan

**Files:**
- Modify: `src/scanner.rs:10-166`
- Test: `src/scanner.rs`

- [ ] **Step 1: Write the failing traversal tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn config_for(root: &Path, include_hidden: bool) -> AppConfig {
        AppConfig {
            scan_dirs: vec![root.to_path_buf()],
            extensions: vec!["txt".into()],
            include_hidden,
            ..AppConfig::default()
        }
    }

    #[test]
    fn skips_hidden_directories_when_include_hidden_is_false() {
        let root = tempdir().expect("tempdir");
        fs::create_dir(root.path().join(".hidden")).expect("hidden dir");
        fs::write(root.path().join(".hidden/inside.txt"), b"x").expect("hidden file");
        fs::write(root.path().join("visible.txt"), b"x").expect("visible file");

        let (_, records, warnings) = scan_files(&config_for(root.path(), false), None).unwrap();

        assert!(warnings.is_empty());
        assert_eq!(records.iter().map(|item| item.file_name.as_str()).collect::<Vec<_>>(), vec!["visible.txt"]);
    }

    #[test]
    fn keeps_hidden_directories_when_include_hidden_is_true() {
        let root = tempdir().expect("tempdir");
        fs::create_dir(root.path().join(".hidden")).expect("hidden dir");
        fs::write(root.path().join(".hidden/inside.txt"), b"x").expect("hidden file");
        fs::write(root.path().join("visible.txt"), b"x").expect("visible file");

        let (_, records, _) = scan_files(&config_for(root.path(), true), None).unwrap();

        assert_eq!(records.len(), 2);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test scanner::tests -- --nocapture`
Expected: FAIL because the current `continue` logic skips the hidden directory entry itself but still descends into `.hidden`.

- [ ] **Step 3: Write the minimal implementation**

```rust
let walker = WalkDir::new(&canonical_root).follow_links(false);
let walker = if config.recursive {
    walker
} else {
    walker.max_depth(1)
};

for entry in walker.into_iter().filter_entry(|entry| {
    entry.depth() == 0 || config.include_hidden || !is_hidden_entry(entry)
}) {
    // existing scan loop
}
```

Keep the later `is_hidden_entry` guard for hidden files, but make directory pruning happen before descent.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test scanner::tests -- --nocapture`
Expected: PASS with one visible file in the hidden-off case and two files in the hidden-on case.

- [ ] **Step 5: Commit**

```bash
git add src/scanner.rs
git commit -m "fix: prune hidden directories during scan"
```

### Task 3: Store latest scan authorization state in the engine

**Files:**
- Modify: `src/model.rs:185-257`
- Modify: `src/engine.rs:10-177`
- Test: `src/engine.rs`

- [ ] **Step 1: Write the failing engine-state tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AppConfig, GroupMember, MatchEdge, MatchGroup, MatchType, ScanReport, ScanSummary,
    };
    use std::path::PathBuf;

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

    #[test]
    fn builds_latest_scan_state_from_report() {
        let state = build_latest_scan_state(&report_with_keep_and_candidate());
        assert!(state.all_members.contains(&PathBuf::from("C:/books/keep.txt")));
        assert!(state.all_members.contains(&PathBuf::from("C:/books/delete.txt")));
        assert!(state.keep_recommended.contains(&PathBuf::from("C:/books/keep.txt")));
        assert!(state.deletable.contains(&PathBuf::from("C:/books/delete.txt")));
        assert!(!state.deletable.contains(&PathBuf::from("C:/books/keep.txt")));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test engine::tests -- --nocapture`
Expected: FAIL because there is no latest authorization state type or builder yet.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Debug, Clone, Default)]
pub struct LatestScanState {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub all_members: std::collections::HashSet<PathBuf>,
    pub deletable: std::collections::HashSet<PathBuf>,
    pub keep_recommended: std::collections::HashSet<PathBuf>,
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
```

Update `Engine` to store `latest_scan_state: Arc<RwLock<Option<LatestScanState>>>`, clear it alongside `report` when a scan starts/fails, and set it whenever a scan succeeds. Add a small helper like `store_completed_scan(report: ScanReport)` so both runtime code and tests use the same state-write path.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test engine::tests -- --nocapture`
Expected: PASS with the authorization snapshot populated from the report.

- [ ] **Step 5: Commit**

```bash
git add src/model.rs src/engine.rs
git commit -m "refactor: persist latest scan authorization state"
```

### Task 4: Prevalidate deletes and harden `/api/delete`

**Files:**
- Modify: `src/model.rs:208-257`
- Modify: `src/safety.rs:1-100`
- Modify: `src/web.rs:109-129, 622-633`
- Modify: `src/engine.rs:24-177`
- Modify: `src/main.rs:169-176`
- Test: `src/safety.rs`
- Test: `src/web.rs`

- [ ] **Step 1: Write the failing delete tests**

```rust
#[test]
fn rejects_paths_that_are_not_in_the_deletable_set() {
    let root = tempdir().expect("tempdir");
    let allowed = root.path().join("delete.txt");
    let blocked = root.path().join("keep.txt");
    fs::write(&allowed, b"x").unwrap();
    fs::write(&blocked, b"x").unwrap();

    let mut deletable = std::collections::HashSet::new();
    deletable.insert(allowed.canonicalize().unwrap());

    let result = prepare_delete_paths(&[blocked], &deletable);
    assert!(result.is_err());
}

#[test]
fn mixed_valid_and_invalid_paths_do_not_start_deletion() {
    let root = tempdir().expect("tempdir");
    let allowed = root.path().join("delete.txt");
    let blocked = root.path().join("keep.txt");
    fs::write(&allowed, b"x").unwrap();
    fs::write(&blocked, b"x").unwrap();

    let mut deletable = std::collections::HashSet::new();
    deletable.insert(allowed.canonicalize().unwrap());

    let result = prepare_delete_paths(&[allowed.clone(), blocked], &deletable);

    assert!(result.is_err());
    assert!(allowed.exists());
}

#[test]
fn duplicate_requested_paths_are_rejected_before_deletion() {
    let root = tempdir().expect("tempdir");
    let allowed = root.path().join("delete.txt");
    fs::write(&allowed, b"x").unwrap();

    let mut deletable = std::collections::HashSet::new();
    deletable.insert(allowed.canonicalize().unwrap());

    let result = prepare_delete_paths(&[allowed.clone(), allowed.clone()], &deletable);

    assert!(result.is_err());
    assert!(allowed.exists());
}
```

```rust
fn report_with_keep_and_candidate(keep: &Path, delete: &Path) -> ScanReport {
    ScanReport {
        generated_at: "2026-03-24T10:00:00+08:00".into(),
        roots: vec![keep.parent().unwrap().to_path_buf()],
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
                    path: keep.to_path_buf(),
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
                    path: delete.to_path_buf(),
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
            evidence: Vec::new(),
        }],
        summary: ScanSummary::default(),
        warnings: Vec::new(),
    }
}

fn web_state_for_delete_tests(engine: Engine) -> WebState {
    WebState { engine }
}

#[tokio::test]
async fn delete_handler_only_accepts_current_scan_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let keep = temp.path().join("keep.txt");
    let delete = temp.path().join("delete.txt");
    fs::write(&keep, b"x").unwrap();
    fs::write(&delete, b"x").unwrap();

    let engine = Engine::new(AppConfig::default());
    engine.store_completed_scan(report_with_keep_and_candidate(&keep, &delete));
    let state = web_state_for_delete_tests(engine);

    let response = delete_handler(
        State(state),
        Json(DeleteRequest {
            paths: vec![keep.canonicalize().unwrap().display().to_string()],
        }),
    )
    .await
    .unwrap_err()
    .into_response();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("不可删除") || text.contains("当前扫描结果"));
    assert!(!text.contains(&keep.canonicalize().unwrap().display().to_string()));
    assert!(!text.contains("deleted_paths"));
    assert!(!text.contains("log_path"));
}

#[test]
fn delete_log_failure_becomes_sanitized_warning() {
    let result = finalize_delete_result(
        vec![PathBuf::from("C:/books/delete.txt")],
        Err(anyhow::anyhow!("写入删除日志失败: C:/secret/delete-log.json")),
    )
    .unwrap();

    assert_eq!(result.deleted_count, 1);
    assert!(result.log_path.is_none());
    assert_eq!(
        result.log_write_warning.as_deref(),
        Some("删除已完成，但删除日志写入失败"),
    );
    assert!(!result.log_write_warning.unwrap().contains("C:/secret"));
}

#[test]
fn delete_success_payload_omits_internal_path_fields() {
    let payload = build_delete_response(&DeleteResult {
        requested_count: 1,
        deleted_count: 1,
        deleted_paths: vec![PathBuf::from("C:/books/delete.txt")],
        log_path: Some(PathBuf::from("C:/logs/delete-log.json")),
        log_write_warning: Some("删除已完成，但删除日志写入失败".into()),
    });
    let object = payload.as_object().unwrap();
    assert!(!object.contains_key("deleted_paths"));
    assert!(!object.contains_key("log_path"));
    assert!(!payload.to_string().contains("C:/books"));
    assert!(!payload.to_string().contains("C:/logs"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test safety::tests -- --nocapture && cargo test web::tests -- --nocapture`
Expected: FAIL because delete authorization still trusts caller roots, mixed valid/invalid and duplicate input is not fully prevalidated yet, delete-log failures still surface as fatal/raw errors, and the success payload helper does not exist yet.

- [ ] **Step 3: Write the minimal implementation**

```rust
pub fn prepare_delete_paths(
    paths: &[PathBuf],
    deletable: &std::collections::HashSet<PathBuf>,
) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(anyhow!("未提供待删除文件"));
    }

    let mut canonical_paths = Vec::with_capacity(paths.len());
    let mut seen = std::collections::HashSet::with_capacity(paths.len());
    for path in paths {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("目标文件不存在或无法访问: {}", path.display()))?;
        if !deletable.contains(&canonical) {
            return Err(anyhow!("所选文件不属于当前扫描结果或不可删除"));
        }
        if !seen.insert(canonical.clone()) {
            return Err(anyhow!("所选文件包含重复项"));
        }
        canonical_paths.push(canonical);
    }
    Ok(canonical_paths)
}

fn finalize_delete_result(
    canonical_paths: Vec<PathBuf>,
    log_result: Result<PathBuf>,
) -> Result<DeleteResult> {
    match log_result {
        Ok(log_path) => Ok(DeleteResult {
            requested_count: canonical_paths.len(),
            deleted_count: canonical_paths.len(),
            deleted_paths: canonical_paths,
            log_path: Some(log_path),
            log_write_warning: None,
        }),
        Err(_) => Ok(DeleteResult {
            requested_count: canonical_paths.len(),
            deleted_count: canonical_paths.len(),
            deleted_paths: canonical_paths,
            log_path: None,
            log_write_warning: Some("删除已完成，但删除日志写入失败".to_string()),
        }),
    }
}

fn delete_canonical_paths(canonical_paths: &[PathBuf]) -> Result<DeleteResult> {
    for path in canonical_paths {
        trash::delete(path).with_context(|| format!("移入回收站失败: {}", path.display()))?;
    }

    finalize_delete_result(
        canonical_paths.to_vec(),
        write_delete_log(canonical_paths),
    )
}

pub fn delete_latest_scan_candidates(
    paths: &[PathBuf],
    deletable: &std::collections::HashSet<PathBuf>,
) -> Result<DeleteResult> {
    let canonical_paths = prepare_delete_paths(paths, deletable)?;
    delete_canonical_paths(&canonical_paths)
}

pub fn delete_to_trash(paths: &[PathBuf], roots: &[PathBuf]) -> Result<DeleteResult> {
    let canonical_roots = canonicalize_roots(roots)?;
    let canonical_paths = paths
        .iter()
        .map(|path| ensure_within_roots(path, &canonical_roots))
        .collect::<Result<Vec<_>>>()?;
    delete_canonical_paths(&canonical_paths)
}
```

Update `/api/delete` to:
- read `state.engine.latest_scan_state()`
- reject missing state with a clean `404/400`
- accept only `DeleteRequest { paths }`
- return counts and an optional warning, never `deleted_paths` / `log_path`
- keep `DeleteResult.log_path` available for CLI output while introducing a sanitized web response helper like `build_delete_response(result: &DeleteResult) -> serde_json::Value`
- update `run_delete_command(...)` to print the log path only when `result.log_path` is `Some(...)`, otherwise print the sanitized warning line instead of crashing or inventing a path

Example response payload:

```rust
fn build_delete_response(result: &DeleteResult) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "message": format!("已移入回收站 {} 个文件", result.deleted_count),
        "requested_count": result.requested_count,
        "deleted_count": result.deleted_count,
        "log_write_warning": result.log_write_warning,
    })
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test safety::tests -- --nocapture && cargo test web::tests -- --nocapture`
Expected: PASS with delete requests blocked unless they are current server-authorized candidates.

- [ ] **Step 5: Commit**

```bash
git add src/safety.rs src/web.rs src/engine.rs src/model.rs
git commit -m "fix: authorize deletes from latest scan state only"
```

### Task 5: Harden export/config endpoints and sanitize web-facing errors

**Files:**
- Modify: `src/export.rs:1-86`
- Modify: `src/config.rs:6-33`
- Modify: `src/web.rs:16-107, 90-107, 136-197, 373-382, 421-455, 575-638`
- Modify: `src/main.rs:109-213`
- Test: `src/web.rs`
- Test: `src/main.rs`

- [ ] **Step 1: Write the failing web handler tests**

```rust
fn report_with_keep_and_candidate(keep: &Path, delete: &Path) -> ScanReport {
    ScanReport {
        generated_at: "2026-03-24T10:00:00+08:00".into(),
        roots: vec![keep.parent().unwrap().to_path_buf()],
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
                    path: keep.to_path_buf(),
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
                    path: delete.to_path_buf(),
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
            evidence: Vec::new(),
        }],
        summary: ScanSummary::default(),
        warnings: Vec::new(),
    }
}

#[tokio::test]
async fn save_config_handler_writes_to_the_resolved_config_path() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("custom-config.json");
    let state = WebState {
        engine: Engine::new(AppConfig::default()),
        resolved_config_path: config_path.clone(),
        export_dir: temp.path().join("exports"),
    };
    let config = AppConfig {
        scan_dirs: vec![PathBuf::from("C:/books")],
        ..AppConfig::default()
    };

    save_config_handler(State(state), Json(config)).await.unwrap();

    assert!(config_path.exists());
}

#[test]
fn save_config_target_path_prefers_explicit_path() {
    let explicit = PathBuf::from("C:/tmp/custom-config.json");
    assert_eq!(save_config_target_path(Some(explicit.as_path())), explicit);
}

#[tokio::test]
async fn export_handler_requires_current_report() {
    let temp = tempfile::tempdir().unwrap();
    let state = WebState {
        engine: Engine::new(AppConfig::default()),
        resolved_config_path: temp.path().join("config.json"),
        export_dir: temp.path().join("exports"),
    };

    let response = export_handler(
        State(state),
        Json(ExportRequest {
            format: "json".into(),
            file_name_prefix: None,
        }),
    )
    .await
    .unwrap_err()
    .into_response();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("暂无扫描结果"));
}

#[tokio::test]
async fn start_scan_rejects_empty_effective_scan_dirs() {
    let state = WebState {
        engine: Engine::new(AppConfig {
            scan_dirs: vec![PathBuf::from("C:/books")],
            ..AppConfig::default()
        }),
        resolved_config_path: PathBuf::from("C:/tmp/config.json"),
        export_dir: PathBuf::from("C:/tmp/exports"),
    };

    let response = start_scan(
        State(state),
        Json(ScanRequest {
            scan_dirs: Some(vec![]),
            extensions: None,
            similarity_threshold: None,
            review_threshold: None,
            stopwords: None,
            recursive: None,
            include_hidden: None,
        }),
    )
    .await
    .unwrap_err()
    .into_response();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("请先设置扫描目录"));
}

#[tokio::test]
async fn export_handler_returns_safe_file_name_and_writes_under_safe_export_dir() {
    let temp = tempfile::tempdir().unwrap();
    let export_dir = temp.path().join("exports");
    let keep = temp.path().join("keep.txt");
    let delete = temp.path().join("delete.txt");
    std::fs::write(&keep, b"x").unwrap();
    std::fs::write(&delete, b"x").unwrap();

    let engine = Engine::new(AppConfig::default());
    engine.store_completed_scan(report_with_keep_and_candidate(&keep, &delete));
    let state = WebState {
        engine,
        resolved_config_path: temp.path().join("config.json"),
        export_dir: export_dir.clone(),
    };

    let response = export_handler(
        State(state),
        Json(ExportRequest {
            format: "json".into(),
            file_name_prefix: Some("../../outside".into()),
        }),
    )
    .await
    .unwrap();

    let body = response.0;
    assert!(body.message.contains("导出成功"));
    assert!(body.file_name.ends_with(".json"));
    assert!(!body.file_name.contains(".."));

    let mut names = std::fs::read_dir(&export_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 1);
    assert_eq!(names.pop().unwrap(), body.file_name);

    let written = export_dir.join(&body.file_name).canonicalize().unwrap();
    assert!(written.starts_with(export_dir.canonicalize().unwrap()));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test web::tests -- --nocapture && cargo test save_config_target_path_prefers_explicit_path -- --nocapture`
Expected: FAIL because `/api/config` always saves to `None`, `web::serve` still has no resolved config path to thread through, `/api/export` still trusts a raw path, and the no-report / empty-scan validation paths are not implemented yet.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
    pub resolved_config_path: PathBuf,
    pub export_dir: PathBuf,
}

#[derive(serde::Deserialize)]
struct ExportRequest {
    format: String,
    file_name_prefix: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiExportResponse {
    ok: bool,
    message: String,
    file_name: String,
}
```

```rust
pub fn default_export_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("novel-filter-tool")
        .join("exports")
}

pub fn build_safe_export_path(base: &Path, format: ExportFormat, prefix: Option<&str>, stamp: &str) -> PathBuf {
    let prefix = prefix
        .unwrap_or("novel-filter-report")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect::<String>();
    let ext = match format {
        ExportFormat::Json => "json",
        ExportFormat::Csv => "csv",
    };
    base.join(format!("{}-{}.{}", prefix, stamp, ext))
}
```

Handler changes:
- `serve(...)` accepts the resolved config path from `main.rs` and stores it in `WebState`
- add a small pure helper in `main.rs`, e.g. `save_config_target_path(config_path: Option<&Path>) -> PathBuf`, and reuse it for both `run_web_command(...)` and `run_save_config_command(...)`
- update the Task 4 helper `fn web_state_for_delete_tests(engine: Engine) -> WebState` into a shape like `fn web_state_for_delete_tests(engine: Engine, temp: &tempfile::TempDir) -> WebState`, and switch the earlier `delete_handler_only_accepts_current_scan_candidates` test to that expanded helper once `WebState` gains `resolved_config_path` and `export_dir`
- `save_config_handler` uses `save_config(&config, Some(&state.resolved_config_path))`
- `start_scan` rejects empty final `scan_dirs` before launching work
- `export_handler` reads the current `ScanReport`, chooses format from `request.format`, writes only under `state.export_dir`, and returns `Json(ApiExportResponse { ok: true, message: "导出成功".into(), file_name })`
- path-sensitive endpoints stop using raw `error.to_string()` as user-facing output; replace with stable messages like `"保存配置失败"`, `"导出失败"`, `"所选文件不属于当前扫描结果或不可删除"`, and the sanitized warning `"删除已完成，但删除日志写入失败"`

Update the embedded frontend to match the new API:

```js
body: JSON.stringify({ paths: [...state.selectedDeletes] })
```

```js
body: JSON.stringify({
  format: el('exportFormat').value,
  file_name_prefix: el('exportNamePrefix').value.trim() || null,
})
```

Replace the old export-path input with:
- a format select (`json` / `csv`)
- an optional file-name prefix input

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test web::tests -- --nocapture && cargo test save_config_target_path_prefers_explicit_path -- --nocapture`
Expected: PASS with config saved to the active path, the explicit `--config` path preserved through `main.rs`, `/api/export` failing cleanly when no report exists, `/api/scan` rejecting an empty effective scan-dir set before background work starts, export constrained to the safe export directory, and no absolute paths surfaced in success/error payloads.

- [ ] **Step 5: Commit**

```bash
git add src/export.rs src/config.rs src/web.rs src/main.rs
git commit -m "fix: harden web export and config persistence"
```

### Task 6: Remove CLI scan persistence side effects

**Files:**
- Modify: `src/main.rs:124-213`
- Test: `src/main.rs`

- [ ] **Step 1: Write the failing CLI behavior tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scan_command_does_not_write_config_file() {
        let temp = tempdir().unwrap();
        let config_path = temp.path().join("config.json");
        let scan_root = temp.path().join("books");
        fs::create_dir(&scan_root).unwrap();
        fs::write(scan_root.join("sample.txt"), b"x").unwrap();

        let mut config = AppConfig::default();
        let args = ScanArgs {
            dirs: vec![scan_root],
            extensions: vec!["txt".into()],
            similarity: None,
            review: None,
            stopwords: vec![],
            export: None,
            no_recursive: false,
            include_hidden: false,
            dry_run: true,
        };

        run_scan_command(&mut config, Some(&config_path), args).unwrap();

        assert!(!config_path.exists());
    }

    #[test]
    fn save_config_command_still_writes_to_the_explicit_path() {
        let temp = tempdir().unwrap();
        let config_path = temp.path().join("config.json");
        let mut config = AppConfig::default();

        let args = ConfigArgs {
            dirs: vec![PathBuf::from("C:/books")],
            extensions: vec!["txt".into()],
            similarity: Some(0.9),
            review: Some(0.7),
            stopwords: vec!["完结".into()],
            port: Some(9000),
        };

        run_save_config_command(&mut config, Some(&config_path), args).unwrap();
        assert!(config_path.exists());
    }

    #[test]
    fn save_config_path_defaults_when_no_explicit_path_is_given() {
        assert_eq!(save_config_target_path(None), default_config_path());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test scan_command_does_not_write_config_file -- --nocapture && cargo test save_config_path_defaults_when_no_explicit_path_is_given -- --nocapture`
Expected: FAIL because `run_scan_command` still persists config before scanning and the default-path helper is not wired yet.

- [ ] **Step 3: Write the minimal implementation**

```rust
fn run_scan_command(config: &mut AppConfig, _config_path: Option<&Path>, args: ScanArgs) -> Result<()> {
    apply_common_updates(
        config,
        &args.dirs,
        &args.extensions,
        args.similarity,
        args.review,
        &args.stopwords,
    );
    config.recursive = !args.no_recursive;
    config.include_hidden = args.include_hidden;

    let engine = Engine::new(config.clone());
    let report = engine.run_scan(config.clone())?;
    // no save_config call here
    ...
}
```

If the now-unused `config_path` parameter becomes noisy, remove it from `run_scan_command` and its caller instead of keeping a dead argument around.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test scan_command_does_not_write_config_file -- --nocapture && cargo test save_config_command_still_writes_to_the_explicit_path -- --nocapture && cargo test save_config_path_defaults_when_no_explicit_path_is_given -- --nocapture`
Expected: PASS with `scan` side-effect-free and `save-config` writing to both the requested explicit path and the default path when no explicit path is provided.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "fix: stop persisting config from scan command"
```

### Task 7: Regression sweep and manual smoke checks

**Files:**
- No new code expected unless verification uncovers a regression

- [ ] **Step 1: Run the focused automated suite**

Run:
```bash
cargo test model::tests -- --nocapture && cargo test scanner::tests -- --nocapture && cargo test engine::tests -- --nocapture && cargo test safety::tests -- --nocapture && cargo test web::tests -- --nocapture && cargo test scan_command_does_not_write_config_file -- --nocapture && cargo test save_config_command_still_writes_to_the_explicit_path -- --nocapture && cargo test save_config_path_defaults_when_no_explicit_path_is_given -- --nocapture
```
Expected: PASS for all focused module suites.

- [ ] **Step 2: Run the project-level verification**

Run:
```bash
cargo fmt --check && cargo test
```
Expected: `cargo fmt --check` reports no diffs; `cargo test` passes.

- [ ] **Step 3: Run a manual Web UI smoke test**

Run:
```bash
cargo run -- web --no-browser --port 8765
```
Then verify in a browser against `http://127.0.0.1:8765`:
- clearing scan directories/extensions/stopwords actually clears them
- scanning with no directories returns a friendly validation message
- export UI now offers format + file-name prefix instead of arbitrary output path
- delete request works only for checked current-scan deletable candidates and never shows raw absolute-path error dumps

Expected: all four checks behave as specified in `docs/superpowers/specs/2026-03-24-novel-tool-correctness-hardening-design.md`.

- [ ] **Step 4: Re-run the Windows helper once the SDK blocker is fixed**

Run:
```bash
powershell.exe -NoProfile -File "C:\Users\lisheng\Desktop\1\novel filter tool\.omc\run_tests_ascii.ps1"
```
Expected: PASS once `kernel32.lib` / Windows SDK libs are installed; if it still fails, capture the new linker output before making more code changes.

- [ ] **Step 5: Commit the final verification-safe state**

```bash
git add src/model.rs src/engine.rs src/scanner.rs src/safety.rs src/export.rs src/config.rs src/web.rs src/main.rs
git commit -m "fix: harden correctness and localhost-only web boundaries"
```
