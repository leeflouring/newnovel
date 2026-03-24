use crate::model::DeleteResult;
use anyhow::{Context, Result, anyhow};
use chrono::Local;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub fn canonicalize_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut items = roots
        .iter()
        .map(|root| {
            root.canonicalize()
                .with_context(|| format!("扫描根目录不存在: {}", root.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    items.sort();
    items.dedup();
    Ok(items)
}

pub fn ensure_within_roots(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("目标文件不存在或无法访问: {}", path.display()))?;
    if roots.iter().any(|root| canonical.starts_with(root)) {
        Ok(canonical)
    } else {
        Err(anyhow!(
            "拒绝处理扫描目录之外的文件: {}",
            canonical.display()
        ))
    }
}

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
    requested_count: usize,
    deleted_paths: Vec<PathBuf>,
    log_result: Result<PathBuf>,
) -> Result<DeleteResult> {
    let deleted_count = deleted_paths.len();
    match log_result {
        Ok(log_path) => Ok(DeleteResult {
            requested_count,
            deleted_count,
            deleted_paths,
            log_path: Some(log_path),
            log_write_warning: None,
        }),
        Err(_) => Ok(DeleteResult {
            requested_count,
            deleted_count,
            deleted_paths,
            log_path: None,
            log_write_warning: Some("删除已完成，但删除日志写入失败".to_string()),
        }),
    }
}

fn delete_canonical_paths(canonical_paths: &[PathBuf]) -> Result<DeleteResult> {
    let mut seen = std::collections::HashSet::with_capacity(canonical_paths.len());
    for path in canonical_paths {
        if !seen.insert(path.clone()) {
            return Err(anyhow!("所选文件包含重复项"));
        }
    }

    let mut deleted_paths = Vec::with_capacity(canonical_paths.len());
    let mut first_error = None;
    for path in canonical_paths {
        match trash::delete(path).with_context(|| format!("移入回收站失败: {}", path.display()))
        {
            Ok(()) => deleted_paths.push(path.clone()),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    if deleted_paths.is_empty() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }

    let log_result = write_delete_log(&deleted_paths);
    finalize_delete_result(canonical_paths.len(), deleted_paths, log_result)
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
    if canonical_roots.is_empty() {
        return Err(anyhow!("删除前必须提供扫描根目录"));
    }
    if paths.is_empty() {
        return Err(anyhow!("未提供待删除文件"));
    }

    let canonical_paths = paths
        .iter()
        .map(|path| ensure_within_roots(path, &canonical_roots))
        .collect::<Result<Vec<_>>>()?;
    delete_canonical_paths(&canonical_paths)
}

fn write_delete_log(deleted_paths: &[PathBuf]) -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("novel-filter-tool")
        .join("logs");
    fs::create_dir_all(&base).with_context(|| format!("创建日志目录失败: {}", base.display()))?;
    let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let path = base.join(format!("delete-{timestamp}.json"));
    let payload = DeleteLog {
        generated_at: Local::now().to_rfc3339(),
        deleted_paths: deleted_paths.to_vec(),
    };
    fs::write(&path, serde_json::to_vec_pretty(&payload)?)
        .with_context(|| format!("写入删除日志失败: {}", path.display()))?;
    Ok(path)
}

#[derive(Serialize)]
struct DeleteLog {
    generated_at: String,
    deleted_paths: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn blocks_paths_outside_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let file = outside.path().join("a.txt");
        fs::write(&file, b"x").expect("write file");
        let result = ensure_within_roots(&file, &[root.path().to_path_buf()]);
        assert!(result.is_err());
    }

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

    #[test]
    fn midstream_delete_failure_returns_partial_result_after_prior_successes() {
        let root = tempdir().expect("tempdir");
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, b"first").expect("write first file");
        fs::write(&second, b"second").expect("write second file");

        let first = first.canonicalize().expect("canonical first");
        let second = second.canonicalize().expect("canonical second");
        fs::remove_file(&second).expect("remove second file before delete");

        let result = delete_canonical_paths(&[first.clone(), second])
            .expect("partial delete should return structured result");

        assert_eq!(result.requested_count, 2);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.deleted_paths, vec![first.clone()]);
        assert!(!first.exists(), "first file should already be gone");
    }

    #[test]
    fn middle_delete_failure_still_attempts_later_paths() {
        let root = tempdir().expect("tempdir");
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        let third = root.path().join("third.txt");
        fs::write(&first, b"first").expect("write first file");
        fs::write(&second, b"second").expect("write second file");
        fs::write(&third, b"third").expect("write third file");

        let first = first.canonicalize().expect("canonical first");
        let second = second.canonicalize().expect("canonical second");
        let third = third.canonicalize().expect("canonical third");
        fs::remove_file(&second).expect("remove second file before delete");

        let result = delete_canonical_paths(&[first.clone(), second, third.clone()])
            .expect("partial delete should return structured result");

        assert_eq!(result.requested_count, 3);
        assert_eq!(result.deleted_count, 2);
        assert_eq!(result.deleted_paths, vec![first.clone(), third.clone()]);
        assert!(!first.exists(), "first file should already be gone");
        assert!(!third.exists(), "third file should also be attempted");
    }
}
