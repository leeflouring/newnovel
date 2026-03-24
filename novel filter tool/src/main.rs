mod config;
mod engine;
mod export;
mod matcher;
mod model;
mod normalize;
mod recommend;
mod safety;
mod scanner;
mod web;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use config::{load_config, resolve_config_path, save_config};
use engine::Engine;
use export::{ExportFormat, default_export_dir, export_report};
use model::AppConfig;
use safety::delete_to_trash;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "novel_filter_tool",
    version,
    about = "本地小说文件重名/近似重名筛选工具"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Scan(ScanArgs),
    Export(ExportArgs),
    Delete(DeleteArgs),
    SaveConfig(ConfigArgs),
    Web(WebArgs),
}

#[derive(Args, Debug)]
struct ScanArgs {
    #[arg(short = 'd', long = "dir")]
    dirs: Vec<PathBuf>,
    #[arg(short = 'e', long = "ext", value_delimiter = ',')]
    extensions: Vec<String>,
    #[arg(long)]
    similarity: Option<f32>,
    #[arg(long)]
    review: Option<f32>,
    #[arg(long = "stopword")]
    stopwords: Vec<String>,
    #[arg(long)]
    export: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    no_recursive: bool,
    #[arg(long, default_value_t = false)]
    include_hidden: bool,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Args, Debug)]
struct ExportArgs {
    #[arg(short = 'i', long)]
    input: PathBuf,
    #[arg(short = 'o', long)]
    output: PathBuf,
    #[arg(long)]
    format: Option<String>,
}

#[derive(Args, Debug)]
struct DeleteArgs {
    #[arg(short = 'r', long = "root")]
    roots: Vec<PathBuf>,
    #[arg(short = 'p', long = "path")]
    paths: Vec<PathBuf>,
}

#[derive(Args, Debug)]
struct ConfigArgs {
    #[arg(short = 'd', long = "dir")]
    dirs: Vec<PathBuf>,
    #[arg(short = 'e', long = "ext", value_delimiter = ',')]
    extensions: Vec<String>,
    #[arg(long)]
    similarity: Option<f32>,
    #[arg(long)]
    review: Option<f32>,
    #[arg(long = "stopword")]
    stopwords: Vec<String>,
    #[arg(long)]
    port: Option<u16>,
}

#[derive(Args, Debug)]
struct WebArgs {
    #[arg(short = 'd', long = "dir")]
    dirs: Vec<PathBuf>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long, default_value_t = false)]
    no_browser: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: config_path,
        command,
    } = Cli::parse();
    let resolved_config_path = resolve_config_path(config_path.as_deref());
    let mut config = load_config(Some(&resolved_config_path))?;

    match command {
        Command::Scan(args) => run_scan_command(&mut config, args),
        Command::Export(args) => run_export_command(args),
        Command::Delete(args) => run_delete_command(args),
        Command::SaveConfig(args) => {
            run_save_config_command(&mut config, config_path.as_deref(), args)
        }
        Command::Web(args) => run_web_command(&mut config, &resolved_config_path, args).await,
    }
}

fn run_scan_command(config: &mut AppConfig, args: ScanArgs) -> Result<()> {
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
    print_summary(&report);

    if let Some(path) = args.export {
        let format = export::infer_format(&path);
        export_report(&report, &path, format)?;
        println!("已导出到 {}", path.display());
    } else if !args.dry_run {
        println!("提示：可通过 --export result.json 或 --export result.csv 导出结果。");
    }

    Ok(())
}

fn run_export_command(args: ExportArgs) -> Result<()> {
    let data = std::fs::read_to_string(&args.input)?;
    let report = serde_json::from_str::<model::ScanReport>(&data)?;
    let format = args
        .format
        .as_deref()
        .and_then(ExportFormat::from_str)
        .unwrap_or_else(|| export::infer_format(&args.output));
    export_report(&report, &args.output, format)?;
    println!("已导出到 {}", args.output.display());
    Ok(())
}

fn run_delete_command(args: DeleteArgs) -> Result<()> {
    if args.roots.is_empty() || args.paths.is_empty() {
        return Err(anyhow!("delete 需要至少一个 --root 和一个 --path"));
    }
    let result = delete_to_trash(&args.paths, &args.roots)?;
    println!("已移入回收站 {} 个文件", result.deleted_count);
    if result.is_partial() {
        println!("警告: 另有 {} 个文件删除失败", result.failed_count());
    }
    if let Some(log_path) = result.log_path {
        println!("日志文件: {}", log_path.display());
    } else if let Some(warning) = result.log_write_warning {
        println!("警告: {warning}");
    }
    Ok(())
}

fn run_save_config_command(
    config: &mut AppConfig,
    config_path: Option<&Path>,
    args: ConfigArgs,
) -> Result<()> {
    apply_common_updates(
        config,
        &args.dirs,
        &args.extensions,
        args.similarity,
        args.review,
        &args.stopwords,
    );
    if let Some(port) = args.port {
        config.ui_port = port;
    }
    let path = save_config(config, config_path)?;
    println!("配置已保存到 {}", path.display());
    Ok(())
}

async fn run_web_command(
    config: &mut AppConfig,
    resolved_config_path: &Path,
    args: WebArgs,
) -> Result<()> {
    if !args.dirs.is_empty() {
        config.scan_dirs = args.dirs;
        save_config(config, Some(resolved_config_path))?;
    }
    let port = args.port.unwrap_or(config.ui_port);
    let engine = Engine::new(config.clone());
    let export_dir = default_export_dir(resolved_config_path);
    println!("Web UI 已启动: http://127.0.0.1:{port}");
    web::serve(
        engine,
        resolved_config_path.to_path_buf(),
        export_dir,
        port,
        !args.no_browser,
    )
    .await
}

fn apply_common_updates(
    config: &mut AppConfig,
    dirs: &[PathBuf],
    extensions: &[String],
    similarity: Option<f32>,
    review: Option<f32>,
    stopwords: &[String],
) {
    if !dirs.is_empty() {
        config.scan_dirs = dirs.to_vec();
    }
    if !extensions.is_empty() {
        config.extensions = extensions.to_vec();
    }
    if let Some(value) = similarity {
        config.similarity_threshold = value;
    }
    if let Some(value) = review {
        config.review_threshold = value;
    }
    if !stopwords.is_empty() {
        config.stopwords = stopwords.to_vec();
    }
}

fn print_summary(report: &model::ScanReport) {
    println!("扫描完成");
    println!("- 文件数: {}", report.summary.scanned_files);
    println!("- 候选对: {}", report.summary.candidate_pairs);
    println!("- 实际比较: {}", report.summary.compared_pairs);
    println!("- 完全重名组: {}", report.summary.exact_groups);
    println!("- 高相似组: {}", report.summary.near_groups);
    println!("- 待复核组: {}", report.summary.review_groups);
    println!("- 告警: {}", report.summary.warnings);
    println!("- 耗时(ms): {}", report.summary.duration_ms);

    for group in report.groups.iter().take(10) {
        println!(
            "\n组 #{} [{}] {:.3}",
            group.group_id,
            group.result_type.label(),
            group.max_score
        );
        println!("  依据: {}", group.summary_reason);
        for member in &group.members {
            println!(
                "  - {}{}",
                if member.keep_recommended {
                    "[保留] "
                } else {
                    "[候选] "
                },
                member.path.display()
            );
        }
    }
    if report.groups.len() > 10 {
        println!("\n仅展示前 10 组，完整结果请导出 JSON/CSV 或使用 web 模式查看。");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::{TempDir, tempdir};

    struct DefaultConfigPathGuard {
        _lock: MutexGuard<'static, ()>,
        path: PathBuf,
        original: Option<Vec<u8>>,
    }

    impl DefaultConfigPathGuard {
        fn capture() -> Self {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

            let lock = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("lock default config path");
            let path = crate::config::default_config_path();
            let original = fs::read(&path).ok();
            Self {
                _lock: lock,
                path,
                original,
            }
        }
    }

    impl Drop for DefaultConfigPathGuard {
        fn drop(&mut self) {
            if let Some(bytes) = &self.original {
                if let Some(parent) = self.path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&self.path, bytes);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    fn create_scan_root() -> (TempDir, PathBuf) {
        let root = tempdir().expect("tempdir");
        let scan_dir = root.path().join("books");
        fs::create_dir_all(&scan_dir).expect("create scan dir");
        fs::write(scan_dir.join("novel.txt"), b"sample novel").expect("write sample novel");
        (root, scan_dir)
    }

    fn scan_args(scan_dir: PathBuf) -> ScanArgs {
        ScanArgs {
            dirs: vec![scan_dir],
            extensions: vec!["txt".into()],
            similarity: None,
            review: None,
            stopwords: Vec::new(),
            export: None,
            no_recursive: false,
            include_hidden: false,
            dry_run: true,
        }
    }

    fn save_config_args(scan_dir: PathBuf, port: u16) -> ConfigArgs {
        ConfigArgs {
            dirs: vec![scan_dir],
            extensions: vec!["txt".into(), "epub".into()],
            similarity: Some(0.91),
            review: Some(0.71),
            stopwords: vec!["完结".into()],
            port: Some(port),
        }
    }

    #[test]
    fn scan_command_does_not_write_config_file() {
        let _default_guard = DefaultConfigPathGuard::capture();
        let (_root, scan_dir) = create_scan_root();
        let mut config = AppConfig::default();
        let default_path = resolve_config_path(None);
        let sentinel = b"scan-command-should-not-overwrite-config";

        if let Some(parent) = default_path.parent() {
            fs::create_dir_all(parent).expect("create default config parent");
        }
        fs::write(&default_path, sentinel).expect("seed default config sentinel");

        run_scan_command(&mut config, scan_args(scan_dir)).expect("scan command should succeed");

        assert_eq!(
            fs::read(&default_path).expect("read default config after scan"),
            sentinel,
            "scan command should not persist config to the default path"
        );
    }

    #[test]
    fn save_config_command_still_writes_to_the_explicit_path() {
        let (root, scan_dir) = create_scan_root();
        let mut config = AppConfig::default();
        let config_path = root.path().join("custom").join("config.json");

        run_save_config_command(
            &mut config,
            Some(&config_path),
            save_config_args(scan_dir.clone(), 9123),
        )
        .expect("save-config command should succeed");

        let saved = load_config(Some(&config_path)).expect("load saved config");
        assert_eq!(saved.scan_dirs, vec![scan_dir]);
        assert_eq!(saved.extensions, vec!["txt", "epub"]);
        assert_eq!(saved.stopwords, vec!["完结"]);
        assert_eq!(saved.ui_port, 9123);
    }

    #[test]
    fn save_config_path_defaults_when_no_explicit_path_is_given() {
        let _default_guard = DefaultConfigPathGuard::capture();
        let (_root, scan_dir) = create_scan_root();
        let mut config = AppConfig::default();
        let default_path = resolve_config_path(None);
        let _ = fs::remove_file(&default_path);

        run_save_config_command(&mut config, None, save_config_args(scan_dir.clone(), 9234))
            .expect("save-config command should use the default path");

        let saved = load_config(Some(&default_path)).expect("load saved default config");
        assert_eq!(saved.scan_dirs, vec![scan_dir]);
        assert_eq!(saved.ui_port, 9234);
    }
}
