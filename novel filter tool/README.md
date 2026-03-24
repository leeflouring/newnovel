# novel_filter_tool

本地运行的小说文件重名 / 近似重名筛选工具，适合整理 `txt`、`doc`、`docx`、`epub` 书库。项目同时提供命令行和 localhost Web UI，两者共用同一套扫描、导出、删除逻辑。

## 功能概览

- 按文件名识别三类结果：`完全重名`、`高相似`、`待复核`
- 为每组结果给出一个“推荐保留”文件
- 支持 CLI 扫描、导出、删除、保存配置、启动 Web UI
- Web UI 仅监听 `127.0.0.1`，不对局域网开放
- 删除操作只允许处理当前扫描结果中的候选文件，并移入系统回收站
- 支持导出 `JSON` / `CSV`
- Web 导出不接受任意输出路径，只允许选择导出格式和可选文件名前缀
- `scan` 只运行扫描，不会偷偷改写配置文件；只有 `save-config` 和 Web 的“保存配置”会落盘

## 默认配置

默认值来自 `src/model.rs`：

- 扫描扩展名：`txt`, `doc`, `docx`, `epub`
- 递归扫描：开启
- 包含隐藏文件：关闭
- 相似阈值：`0.80`
- 复核阈值：`0.64`
- Web 端口：`8765`
- 默认停用词包含：`完结`、`精校`、`番外`、`修订版`、`作者` 等
- 推荐保留的扩展名优先级：`epub > txt > docx > doc`

## 安装与运行

### 1. 构建

```bash
cargo build --release
```

### 2. 直接运行

```bash
cargo run -- <subcommand> [options]
```

## 命令行用法

### `scan`：扫描目录

```bash
cargo run -- scan -d "D:/Books" -e txt,epub --export result.json
```

常用参数：

- `-d, --dir <DIR>`：扫描目录，可重复传入多个
- `-e, --ext <EXT,...>`：扩展名列表，逗号分隔
- `--similarity <FLOAT>`：高相似阈值
- `--review <FLOAT>`：待复核阈值
- `--stopword <WORD>`：补充停用词，可重复传入
- `--export <PATH>`：导出结果到 JSON / CSV
- `--no-recursive`：关闭递归扫描
- `--include-hidden`：包含隐藏文件 / 目录
- `--dry-run`：只打印扫描摘要，不额外提示导出

### `export`：把已有 JSON 报告再导出成 JSON / CSV

```bash
cargo run -- export -i report.json -o report.csv --format csv
```

### `delete`：把指定文件移入回收站

```bash
cargo run -- delete -r "D:/Books" -p "D:/Books/重复书名.txt"
```

说明：

- `delete` 需要至少一个 `--root` 和一个 `--path`
- CLI 删除要求目标文件位于给定扫描根目录内
- Web 删除限制更严格，只允许删除“当前扫描结果里、且不是推荐保留”的候选文件

### `save-config`：保存默认配置

```bash
cargo run -- save-config -d "D:/Books" -e txt,epub --similarity 0.85 --review 0.70 --port 9000
```

### `web`：启动本地 Web UI

```bash
cargo run -- web -d "D:/Books"
```

常用参数：

- `-d, --dir <DIR>`：启动前更新扫描目录，并保存到配置文件
- `--port <PORT>`：指定端口，默认 `8765`
- `--no-browser`：启动后不自动打开浏览器

启动成功后访问：

```text
http://127.0.0.1:8765
```

## Web UI 行为说明

Web UI 提供以下能力：

- 读取 / 保存配置
- 选择本地目录作为扫描根目录
- 启动后台扫描并轮询进度
- 查看当前报告分组和推荐保留结果
- 导出当前报告
- 删除当前勾选的候选文件
- 取消正在运行的扫描

安全约束：

- 服务端固定绑定 `127.0.0.1`
- `/api/delete` 只接收路径列表，不接受客户端伪造的扫描根目录
- 删除前会校验路径是否属于当前扫描结果中的可删集合
- 删除响应不会回传内部路径等敏感细节
- 导出文件名会做安全清洗，非法字符会被替换

## 配置与输出位置

### 配置文件

默认配置文件路径由 `dirs` crate 决定：

- 优先：`dirs::config_dir()/novel-filter-tool/config.json`
- 回退：`dirs::home_dir()/novel-filter-tool/config.json`

Windows 上通常类似：

```text
C:\Users\<用户名>\AppData\Roaming\novel-filter-tool\config.json
```

也可以通过全局参数显式指定：

```bash
cargo run -- --config custom-config.json web
```

### 导出目录

Web 导出的默认目录为：

```text
<resolved_config_path 的同级目录>/exports/
```

如果未提供合法前缀，默认文件名为：

- `novel-duplicates.json`
- `novel-duplicates.csv`

### 删除日志

删除日志默认写入：

```text
dirs::data_local_dir()/novel-filter-tool/logs/
```

Windows 上通常类似：

```text
C:\Users\<用户名>\AppData\Local\novel-filter-tool\logs\
```

## 测试

```bash
cargo test
cargo fmt --check
```

## 项目结构

```text
src/
├── main.rs        # CLI 入口
├── web.rs         # localhost Web UI 与 API
├── engine.rs      # 扫描状态与任务调度
├── scanner.rs     # 文件遍历与元数据采集
├── matcher.rs     # 相似度分组逻辑
├── recommend.rs   # 推荐保留文件规则
├── safety.rs      # 删除边界校验与回收站删除
├── export.rs      # JSON / CSV 导出
├── config.rs      # 配置读写与路径解析
├── model.rs       # 核心数据结构
└── normalize.rs   # 文件名归一化
```

## 适用场景

- 清理同名小说文本 / 电子书副本
- 从多个来源合并书库后筛查重复项
- 先批量识别，再人工复核“高相似 / 待复核”结果

## 注意事项

- 工具依据文件名及归一化结果判断，不读取书籍正文内容
- 建议先导出报告或先在 Web UI 中复核，再执行删除
- 删除动作走系统回收站，但仍建议在大批量清理前先备份
