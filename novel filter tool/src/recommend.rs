use crate::model::{AppConfig, FileRecord};
use std::cmp::Reverse;

pub fn choose_keep<'a>(records: &[&'a FileRecord], config: &AppConfig) -> (&'a FileRecord, String) {
    let best = records
        .iter()
        .copied()
        .max_by_key(|record| {
            (
                extension_weight(record, config),
                record.size,
                record.modified_ms,
                Reverse(record.relative_path.components().count()),
                Reverse(record.file_name.chars().count()),
            )
        })
        .expect("group must not be empty");

    let reason = format!(
        "优先保留 {} 格式，文件更大且较新",
        if best.extension.is_empty() {
            "无扩展名".to_string()
        } else {
            best.extension.to_uppercase()
        }
    );
    (best, reason)
}

fn extension_weight(record: &FileRecord, config: &AppConfig) -> i32 {
    let total = config.extension_priority.len() as i32;
    (total - config.extension_rank(&record.extension) as i32).max(0)
}
