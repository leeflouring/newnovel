use crate::model::AppConfig;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn default_config_path() -> PathBuf {
    let base = dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("novel-filter-tool").join("config.json")
}

pub fn resolve_config_path(path: Option<&Path>) -> PathBuf {
    path.map(PathBuf::from).unwrap_or_else(default_config_path)
}

pub fn load_config(path: Option<&Path>) -> Result<AppConfig> {
    let path = resolve_config_path(path);
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("读取配置失败: {}", path.display()))?;
    let config = serde_json::from_str::<AppConfig>(&text)
        .with_context(|| format!("解析配置失败: {}", path.display()))?;
    Ok(config)
}

pub fn save_config(config: &AppConfig, path: Option<&Path>) -> Result<PathBuf> {
    let path = resolve_config_path(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(config).context("序列化配置失败")?;
    fs::write(&path, text).with_context(|| format!("保存配置失败: {}", path.display()))?;
    Ok(path)
}
