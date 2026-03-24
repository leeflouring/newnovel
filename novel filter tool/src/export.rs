use crate::model::ScanReport;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Clone)]
pub struct ExportTarget {
    pub file_name: String,
    pub path: PathBuf,
}

impl ExportFormat {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "csv" => Some(Self::Csv),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

pub fn infer_format(path: &Path) -> ExportFormat {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(ExportFormat::from_str)
        .unwrap_or(ExportFormat::Json)
}

pub fn default_export_dir(resolved_config_path: &Path) -> PathBuf {
    resolved_config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("exports")
}

pub fn build_export_target(
    export_dir: &Path,
    file_name_prefix: Option<&str>,
    format: ExportFormat,
) -> ExportTarget {
    let file_name = build_safe_export_file_name(file_name_prefix, format);
    ExportTarget {
        path: export_dir.join(&file_name),
        file_name,
    }
}

fn build_safe_export_file_name(file_name_prefix: Option<&str>, format: ExportFormat) -> String {
    let raw = file_name_prefix.unwrap_or_default().trim();
    let base_name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let base_name = strip_known_export_suffix(base_name);
    let mut sanitized = base_name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ if ch.is_control() => '-',
            _ => ch,
        })
        .collect::<String>();
    sanitized = sanitized
        .trim_matches(|ch: char| ch.is_whitespace() || ch == '.')
        .to_string();
    if sanitized.is_empty() {
        sanitized = "novel-duplicates".to_string();
    }
    format!("{}.{}", sanitized, format.extension())
}

fn strip_known_export_suffix(value: &str) -> &str {
    if value.len() > 5 && value[value.len() - 5..].eq_ignore_ascii_case(".json") {
        &value[..value.len() - 5]
    } else if value.len() > 4 && value[value.len() - 4..].eq_ignore_ascii_case(".csv") {
        &value[..value.len() - 4]
    } else {
        value
    }
}

pub fn export_report(report: &ScanReport, path: &Path, format: ExportFormat) -> Result<()> {
    let bytes = match format {
        ExportFormat::Json => serde_json::to_vec_pretty(report).context("序列化 JSON 失败")?,
        ExportFormat::Csv => report_to_csv_bytes(report)?,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建导出目录失败: {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("写入导出文件失败: {}", path.display()))?;
    Ok(())
}

pub fn report_to_csv_bytes(report: &ScanReport) -> Result<Vec<u8>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record([
        "group_id",
        "result_type",
        "group_reason",
        "group_score",
        "file_id",
        "keep_recommended",
        "recommendation_reason",
        "file_name",
        "normalized_name",
        "path",
        "relative_path",
        "extension",
        "size",
        "modified_ms",
    ])?;

    for group in &report.groups {
        for member in &group.members {
            writer.write_record([
                group.group_id.to_string(),
                group.result_type.label().to_string(),
                group.summary_reason.clone(),
                format!("{:.3}", group.max_score),
                member.file_id.to_string(),
                member.keep_recommended.to_string(),
                member.recommendation_reason.clone(),
                member.file_name.clone(),
                member.normalized_name.clone(),
                member.path.display().to_string(),
                member.relative_path.display().to_string(),
                member.extension.clone(),
                member.size.to_string(),
                member.modified_ms.to_string(),
            ])?;
        }
    }

    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(writer.into_inner().context("生成 CSV 内容失败")?);
    Ok(bytes)
}
