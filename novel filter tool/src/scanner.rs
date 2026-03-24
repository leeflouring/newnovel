use crate::engine::ScanRuntime;
use crate::model::{AppConfig, FileRecord};
use crate::normalize::normalize_filename;
use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::{DirEntry, WalkDir};

pub fn scan_files(
    config: &AppConfig,
    runtime: Option<&ScanRuntime>,
) -> Result<(Vec<PathBuf>, Vec<FileRecord>, Vec<String>)> {
    if config.scan_dirs.is_empty() {
        return Err(anyhow!("请至少提供一个扫描目录"));
    }
    let mut roots = Vec::new();
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let allowed_extensions = config
        .normalized_extensions()
        .into_iter()
        .collect::<HashSet<_>>();
    let stopwords = config.sanitized_stopwords();

    for root in &config.scan_dirs {
        let canonical_root = root
            .canonicalize()
            .with_context(|| format!("扫描目录不存在或无法访问: {}", root.display()))?;
        roots.push(canonical_root.clone());

        let walker = WalkDir::new(&canonical_root).follow_links(false);
        let walker = if config.recursive {
            walker
        } else {
            walker.max_depth(1)
        };

        for entry in walker.into_iter().filter_entry(|entry| {
            config.include_hidden || !entry.file_type().is_dir() || !is_hidden_entry(entry)
        }) {
            if let Some(runtime) = runtime {
                if runtime.is_cancelled() {
                    return Err(anyhow!("扫描已取消"));
                }
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!("遍历失败: {err}"));
                    continue;
                }
            };

            if !config.include_hidden && is_hidden_entry(&entry) {
                continue;
            }
            if entry.file_type().is_symlink() || !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if !allowed_extensions.contains(&extension) {
                continue;
            }

            let canonical_path = match path.canonicalize() {
                Ok(value) => value,
                Err(err) => {
                    warnings.push(format!("路径无法规范化 {}: {err}", path.display()));
                    continue;
                }
            };
            if !seen.insert(canonical_path.clone()) {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(value) => value,
                Err(err) => {
                    warnings.push(format!("读取元数据失败 {}: {err}", path.display()));
                    continue;
                }
            };

            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or_default();
            let normalized = normalize_filename(&stem, &stopwords);
            let relative_path = canonical_path
                .strip_prefix(&canonical_root)
                .unwrap_or(&canonical_path)
                .to_path_buf();

            let id = records.len();
            records.push(FileRecord {
                id,
                path: canonical_path,
                relative_path,
                file_name,
                stem,
                extension,
                size: metadata.len(),
                modified_ms,
                normalized_name: normalized.normalized,
                compact_name: normalized.compact,
                tokens: normalized.tokens,
                grams: normalized.grams,
            });

            if let Some(runtime) = runtime {
                if records.len() % 128 == 0 {
                    runtime.update_scan(records.len(), warnings.len());
                }
            }
        }
    }

    if let Some(runtime) = runtime {
        runtime.update_scan(records.len(), warnings.len());
    }

    Ok((roots, records, warnings))
}

fn is_hidden_entry(entry: &DirEntry) -> bool {
    if is_dot_hidden(entry.path()) {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(metadata) = entry.metadata() {
            return metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }

    false
}

fn is_dot_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::Builder;

    fn config_for(root: &Path, include_hidden: bool) -> AppConfig {
        AppConfig {
            scan_dirs: vec![root.to_path_buf()],
            extensions: vec!["txt".into()],
            include_hidden,
            ..AppConfig::default()
        }
    }

    #[cfg(windows)]
    fn mark_hidden(path: &Path) {
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg(path)
            .status()
            .expect("set hidden attribute");
        assert!(
            status.success(),
            "attrib +h failed for {} with status {:?}",
            path.display(),
            status
        );
    }

    #[test]
    fn skips_hidden_directories_when_include_hidden_is_false() {
        let root = Builder::new()
            .prefix("scanner-test-")
            .tempdir()
            .expect("tempdir");
        fs::create_dir(root.path().join(".hidden")).expect("hidden dir");
        fs::write(root.path().join(".hidden/inside.txt"), b"x").expect("hidden file");
        fs::write(root.path().join("visible.txt"), b"x").expect("visible file");

        let (_, records, warnings) = scan_files(&config_for(root.path(), false), None).unwrap();

        assert!(warnings.is_empty());
        assert_eq!(
            records
                .iter()
                .map(|item| item.file_name.as_str())
                .collect::<Vec<_>>(),
            vec!["visible.txt"]
        );
    }

    #[test]
    fn keeps_hidden_directories_when_include_hidden_is_true() {
        let root = Builder::new()
            .prefix("scanner-test-")
            .tempdir()
            .expect("tempdir");
        fs::create_dir(root.path().join(".hidden")).expect("hidden dir");
        fs::write(root.path().join(".hidden/inside.txt"), b"x").expect("hidden file");
        fs::write(root.path().join("visible.txt"), b"x").expect("visible file");

        let (_, records, _) = scan_files(&config_for(root.path(), true), None).unwrap();

        assert_eq!(records.len(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn honors_windows_hidden_attributes() {
        let root = Builder::new()
            .prefix("scanner-test-")
            .tempdir()
            .expect("tempdir");
        let hidden_dir = root.path().join("hidden-dir");
        let hidden_file = root.path().join("hidden-file.txt");
        let visible_file = root.path().join("visible.txt");

        fs::create_dir(&hidden_dir).expect("hidden dir");
        fs::write(hidden_dir.join("inside.txt"), b"x").expect("hidden nested file");
        fs::write(&hidden_file, b"x").expect("hidden file");
        fs::write(&visible_file, b"x").expect("visible file");
        mark_hidden(&hidden_dir);
        mark_hidden(&hidden_file);

        let (_, skipped_records, skipped_warnings) =
            scan_files(&config_for(root.path(), false), None).unwrap();
        assert!(skipped_warnings.is_empty());
        let mut skipped_names = skipped_records
            .iter()
            .map(|item| item.file_name.as_str())
            .collect::<Vec<_>>();
        skipped_names.sort_unstable();
        assert_eq!(skipped_names, vec!["visible.txt"]);

        let (_, included_records, included_warnings) =
            scan_files(&config_for(root.path(), true), None).unwrap();
        assert!(included_warnings.is_empty());
        let mut included_names = included_records
            .iter()
            .map(|item| item.file_name.as_str())
            .collect::<Vec<_>>();
        included_names.sort_unstable();
        assert_eq!(
            included_names,
            vec!["hidden-file.txt", "inside.txt", "visible.txt"]
        );
    }

    #[test]
    fn skips_hidden_scan_root_when_include_hidden_is_false() {
        let root = Builder::new()
            .prefix("scanner-test-")
            .tempdir()
            .expect("tempdir");
        let hidden_root = root.path().join(".hidden-root");
        fs::create_dir(&hidden_root).expect("hidden root");
        fs::write(hidden_root.join("inside.txt"), b"x").expect("hidden file");

        let (_, records, warnings) = scan_files(&config_for(&hidden_root, false), None).unwrap();

        assert!(warnings.is_empty());
        assert!(records.is_empty());
    }
}
