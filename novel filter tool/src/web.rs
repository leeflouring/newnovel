use crate::config::save_config;
use crate::engine::Engine;
use crate::export::{ExportFormat, build_export_target, export_report};
use crate::model::{AppConfig, DeleteRequest, DeleteResult, ScanRequest};
use crate::safety::delete_latest_scan_candidates;
use anyhow::{Result as AnyResult, anyhow};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
    pub resolved_config_path: PathBuf,
    pub export_dir: PathBuf,
}

pub async fn serve(
    engine: Engine,
    resolved_config_path: PathBuf,
    export_dir: PathBuf,
    port: u16,
    open_browser: bool,
) -> AnyResult<()> {
    let state = WebState {
        engine: engine.clone(),
        resolved_config_path,
        export_dir,
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/api/config", get(get_config).post(save_config_handler))
        .route("/api/scan", post(start_scan))
        .route("/api/progress", get(get_progress))
        .route("/api/report", get(get_report))
        .route("/api/export", post(export_handler))
        .route("/api/delete", post(delete_handler))
        .route("/api/pick-folder", post(pick_folder))
        .route("/api/cancel", post(cancel_scan))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    if open_browser {
        let _ = webbrowser::open(&format!("http://{addr}"));
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn get_config(State(state): State<WebState>) -> Json<AppConfig> {
    Json(state.engine.config())
}

async fn save_config_handler(
    State(state): State<WebState>,
    Json(config): Json<AppConfig>,
) -> Result<Json<ApiMessage>, ApiError> {
    save_config(&config, Some(&state.resolved_config_path)).map_err(sanitize_web_error)?;
    state.engine.set_config(config);
    Ok(Json(ApiMessage::ok("配置已保存")))
}

async fn start_scan(
    State(state): State<WebState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<ApiMessage>, ApiError> {
    let config = request.apply_to(state.engine.config());
    if config.scan_dirs.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "请至少选择一个扫描目录",
        ));
    }
    state
        .engine
        .start_scan_background(config)
        .await
        .map_err(sanitize_web_error)?;
    Ok(Json(ApiMessage::ok("扫描已启动")))
}

async fn get_progress(State(state): State<WebState>) -> Json<crate::model::ProgressSnapshot> {
    Json(state.engine.progress())
}

async fn get_report(
    State(state): State<WebState>,
) -> Result<Json<crate::model::ScanReport>, ApiError> {
    state
        .engine
        .report()
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "暂无扫描结果"))
}

#[derive(serde::Deserialize)]
struct ExportRequest {
    format: Option<String>,
    file_name_prefix: Option<String>,
}

async fn export_handler(
    State(state): State<WebState>,
    Json(request): Json<ExportRequest>,
) -> Result<Json<ApiMessage>, ApiError> {
    let report = state
        .engine
        .report()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "暂无扫描结果"))?;
    let format = request
        .format
        .as_deref()
        .and_then(ExportFormat::from_str)
        .unwrap_or(ExportFormat::Json);
    let target = build_export_target(
        &state.export_dir,
        request.file_name_prefix.as_deref(),
        format,
    );
    export_report(&report, &target.path, format).map_err(sanitize_web_error)?;
    Ok(Json(ApiMessage::ok_with_file_name(
        format!("导出成功：{}", target.file_name),
        target.file_name,
    )))
}

async fn delete_handler(
    State(state): State<WebState>,
    Json(request): Json<DeleteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let latest_scan_state = state
        .engine
        .latest_scan_state()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "暂无扫描结果"))?;
    let paths = request
        .paths
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let result = delete_latest_scan_candidates(&paths, &latest_scan_state.deletable)
        .map_err(sanitize_delete_error)?;
    state.engine.clear_completed_scan_state();
    Ok(Json(build_delete_response(&result)))
}

fn build_delete_response(result: &DeleteResult) -> serde_json::Value {
    let partial = result.is_partial();
    let failed_count = result.failed_count();
    let message = if partial {
        format!(
            "已移入回收站 {} 个文件，另有 {} 个文件删除失败",
            result.deleted_count, failed_count
        )
    } else {
        format!("已移入回收站 {} 个文件", result.deleted_count)
    };

    serde_json::json!({
        "ok": true,
        "partial": partial,
        "message": message,
        "requested_count": result.requested_count,
        "deleted_count": result.deleted_count,
        "failed_count": failed_count,
        "log_write_warning": result.log_write_warning.clone(),
    })
}

fn sanitize_delete_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.starts_with("目标文件不存在或无法访问:") {
        ApiError::new(StatusCode::BAD_REQUEST, "所选文件不存在或无法访问")
    } else if message.starts_with("移入回收站失败:") {
        ApiError::new(StatusCode::BAD_REQUEST, "移入回收站失败")
    } else if message.starts_with("所选文件不属于当前扫描结果或不可删除") {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "所选文件不属于当前扫描结果或不可删除",
        )
    } else if message.starts_with("所选文件包含重复项") {
        ApiError::new(StatusCode::BAD_REQUEST, "所选文件包含重复项")
    } else if message.starts_with("未提供待删除文件") {
        ApiError::new(StatusCode::BAD_REQUEST, "未提供待删除文件")
    } else {
        ApiError::new(StatusCode::BAD_REQUEST, "删除失败，请重试")
    }
}

fn sanitize_web_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.starts_with("已有扫描任务正在运行") {
        ApiError::new(StatusCode::BAD_REQUEST, "已有扫描任务正在运行")
    } else if message.starts_with("请至少提供一个扫描目录") {
        ApiError::new(StatusCode::BAD_REQUEST, "请至少选择一个扫描目录")
    } else if message.starts_with("创建配置目录失败:") || message.starts_with("保存配置失败:")
    {
        ApiError::new(StatusCode::BAD_REQUEST, "保存配置失败")
    } else if message.starts_with("创建导出目录失败:") || message.starts_with("写入导出文件失败:")
    {
        ApiError::new(StatusCode::BAD_REQUEST, "导出失败")
    } else if message.starts_with("选择目录失败:") {
        ApiError::new(StatusCode::BAD_REQUEST, "选择目录失败")
    } else if message.starts_with("扫描目录不存在或无法访问:")
        || message.starts_with("后台任务失败: 扫描目录不存在或无法访问:")
    {
        ApiError::new(StatusCode::BAD_REQUEST, "扫描失败，请检查扫描目录后重试")
    } else {
        ApiError::new(StatusCode::BAD_REQUEST, "操作失败，请重试")
    }
}

async fn cancel_scan(State(state): State<WebState>) -> Json<ApiMessage> {
    state.engine.cancel();
    Json(ApiMessage::ok("已请求取消扫描"))
}

async fn pick_folder() -> Result<Json<ApiFolderResponse>, ApiError> {
    let handle = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|err| sanitize_web_error(anyhow!("选择目录失败: {err}")))?;
    Ok(Json(ApiFolderResponse {
        path: handle.map(|path| path.display().to_string()),
    }))
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
}

impl ApiMessage {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            file_name: None,
        }
    }

    fn ok_with_file_name(message: impl Into<String>, file_name: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            file_name: Some(file_name.into()),
        }
    }
}

#[derive(Debug, Serialize)]
struct ApiFolderResponse {
    path: Option<String>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "ok": false,
                "message": self.message,
            })),
        )
            .into_response()
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>小说重名/近似重名筛选工具</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #0f172a;
      --panel: #111827;
      --panel-soft: #1f2937;
      --text: #e5e7eb;
      --muted: #94a3b8;
      --accent: #38bdf8;
      --accent-2: #22c55e;
      --danger: #f87171;
      --border: rgba(148,163,184,.18);
      --chip: rgba(56,189,248,.15);
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: "Microsoft YaHei", "PingFang SC", system-ui, sans-serif;
      background: linear-gradient(180deg, #020617, #0f172a);
      color: var(--text);
    }
    .layout {
      display: grid;
      grid-template-columns: 360px 1fr;
      min-height: 100vh;
    }
    .sidebar, .content { padding: 24px; }
    .sidebar {
      border-right: 1px solid var(--border);
      background: rgba(15,23,42,.75);
      backdrop-filter: blur(10px);
    }
    .panel {
      background: rgba(17,24,39,.85);
      border: 1px solid var(--border);
      border-radius: 18px;
      padding: 18px;
      box-shadow: 0 16px 40px rgba(0,0,0,.25);
    }
    .panel + .panel { margin-top: 16px; }
    h1, h2, h3 { margin: 0 0 12px; }
    h1 { font-size: 24px; }
    h2 { font-size: 17px; }
    h3 { font-size: 15px; }
    .muted { color: var(--muted); }
    label { display: block; font-size: 13px; color: var(--muted); margin-bottom: 6px; }
    input, textarea, select {
      width: 100%;
      background: rgba(2,6,23,.45);
      color: var(--text);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 11px 12px;
      outline: none;
    }
    textarea { min-height: 90px; resize: vertical; }
    .row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
    .stack > div { margin-bottom: 12px; }
    button {
      border: 0;
      border-radius: 12px;
      padding: 11px 14px;
      font-weight: 700;
      cursor: pointer;
      color: white;
      background: linear-gradient(135deg, #0ea5e9, #2563eb);
    }
    button.secondary { background: rgba(148,163,184,.16); }
    button.success { background: linear-gradient(135deg, #22c55e, #16a34a); }
    button.danger { background: linear-gradient(135deg, #fb7185, #ef4444); }
    button:disabled { opacity: .45; cursor: not-allowed; }
    .actions { display: flex; gap: 10px; flex-wrap: wrap; }
    .stats { display: grid; grid-template-columns: repeat(4, minmax(0,1fr)); gap: 12px; margin-bottom: 18px; }
    .stat {
      background: rgba(15,23,42,.75);
      border: 1px solid var(--border);
      border-radius: 16px;
      padding: 16px;
    }
    .stat b { display: block; font-size: 26px; margin-top: 8px; }
    .groups { display: grid; gap: 14px; }
    .group {
      background: rgba(17,24,39,.9);
      border: 1px solid var(--border);
      border-radius: 18px;
      overflow: hidden;
    }
    .group-header {
      padding: 16px 18px;
      border-bottom: 1px solid var(--border);
      display: flex;
      justify-content: space-between;
      gap: 12px;
      align-items: center;
    }
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 6px 10px;
      border-radius: 999px;
      background: var(--chip);
      color: #7dd3fc;
      font-size: 12px;
      margin-right: 8px;
    }
    .members { padding: 14px 18px 18px; display: grid; gap: 10px; }
    .member {
      background: rgba(2,6,23,.35);
      border: 1px solid var(--border);
      border-radius: 16px;
      padding: 14px;
    }
    .member.keep { border-color: rgba(34,197,94,.5); box-shadow: inset 0 0 0 1px rgba(34,197,94,.2); }
    .member-top { display: flex; justify-content: space-between; gap: 10px; align-items: start; }
    .path { font-size: 12px; color: var(--muted); word-break: break-all; }
    .hint { font-size: 12px; color: #86efac; margin-top: 6px; }
    .danger-hint { color: #fca5a5; }
    .toolbar { display: flex; gap: 10px; margin: 12px 0 18px; flex-wrap: wrap; }
    .toolbar input { max-width: 340px; }
    .hidden { display: none; }
    @media (max-width: 1100px) {
      .layout { grid-template-columns: 1fr; }
      .sidebar { border-right: 0; border-bottom: 1px solid var(--border); }
      .stats { grid-template-columns: repeat(2, minmax(0,1fr)); }
    }
  </style>
</head>
<body>
  <div class="layout">
    <aside class="sidebar">
      <div class="panel">
        <h1>小说文件筛选</h1>
        <p class="muted">本地扫描、本地计算、本地删除。默认仅根据文件名和元数据识别疑似重复。</p>
      </div>
      <div class="panel stack">
        <h2>扫描设置</h2>
        <div>
          <label>扫描目录（每行一个）</label>
          <textarea id="scanDirs"></textarea>
          <div class="actions" style="margin-top:8px;">
            <button class="secondary" id="pickFolderBtn">选择目录</button>
          </div>
        </div>
        <div>
          <label>扩展名（逗号分隔）</label>
          <input id="extensions" />
        </div>
        <div class="row">
          <div>
            <label>高相似阈值</label>
            <input id="similarityThreshold" type="number" min="0" max="1" step="0.01" />
          </div>
          <div>
            <label>待复核阈值</label>
            <input id="reviewThreshold" type="number" min="0" max="1" step="0.01" />
          </div>
        </div>
        <div>
          <label>Stopwords（每行一个）</label>
          <textarea id="stopwords"></textarea>
        </div>
        <div class="actions">
          <button id="saveConfigBtn" class="secondary" disabled>保存配置</button>
          <button id="scanBtn" disabled>开始扫描</button>
          <button id="cancelBtn" class="danger">取消</button>
        </div>
      </div>
      <div class="panel stack">
        <h2>导出与清理</h2>
        <div class="row">
          <div>
            <label>导出格式</label>
            <select id="exportFormat">
              <option value="json">JSON</option>
              <option value="csv">CSV</option>
            </select>
          </div>
          <div>
            <label>文件名前缀（可选）</label>
            <input id="exportNamePrefix" placeholder="例如 novel-duplicates" />
          </div>
        </div>
        <div class="actions">
          <button id="exportBtn" class="success">导出结果</button>
          <button id="deleteBtn" class="danger">删除已勾选</button>
        </div>
        <p class="muted">删除只允许作用于本次扫描根目录内文件，并仅移入回收站。</p>
      </div>
    </aside>
    <main class="content">
      <div class="panel">
        <h2>运行状态</h2>
        <div id="statusText" class="muted">尚未开始扫描。</div>
      </div>
      <div class="toolbar">
        <input id="filterInput" placeholder="按文件名/路径过滤结果" />
      </div>
      <section class="stats" id="stats"></section>
      <section class="groups" id="groups"></section>
    </main>
  </div>
  <script>
    const state = {
      config: null,
      report: null,
      selectedDeletes: new Set(),
      pollTimer: null,
      formInitialized: false,
    };

    const el = (id) => document.getElementById(id);
    const statusText = el('statusText');
    const groupsEl = el('groups');
    const statsEl = el('stats');
    const saveConfigBtn = el('saveConfigBtn');
    const scanBtn = el('scanBtn');

    async function api(url, options = {}) {
      const response = await fetch(url, {
        headers: { 'Content-Type': 'application/json' },
        ...options,
      });
      const data = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(data.message || '请求失败');
      return data;
    }

    function setConfigActionsEnabled(enabled) {
      saveConfigBtn.disabled = !enabled;
      scanBtn.disabled = !enabled;
    }

    function loadConfigToForm(config) {
      state.config = config;
      el('scanDirs').value = (config.scan_dirs || []).join('\n');
      el('extensions').value = (config.extensions || []).join(', ');
      el('similarityThreshold').value = config.similarity_threshold ?? 0.8;
      el('reviewThreshold').value = config.review_threshold ?? 0.64;
      el('stopwords').value = (config.stopwords || []).join('\n');
      if (!el('exportNamePrefix').value) {
        el('exportNamePrefix').value = 'novel-duplicates';
      }
      state.formInitialized = true;
      setConfigActionsEnabled(true);
    }

    function formToRequest() {
      const request = {};
      if (!state.formInitialized) {
        return request;
      }
      request.scan_dirs = el('scanDirs').value.split(/\r?\n/).map(v => v.trim()).filter(Boolean);
      request.extensions = el('extensions').value.split(',').map(v => v.trim()).filter(Boolean);
      request.similarity_threshold = Number(el('similarityThreshold').value || 0.8);
      request.review_threshold = Number(el('reviewThreshold').value || 0.64);
      request.stopwords = el('stopwords').value.split(/\r?\n/).map(v => v.trim()).filter(Boolean);
      return request;
    }

    function currentConfigForSave() {
      return {
        ...(state.config || {}),
        ...formToRequest(),
      };
    }

    function updateStats(report) {
      const summary = report?.summary;
      if (!summary) {
        statsEl.innerHTML = '';
        return;
      }
      const items = [
        ['扫描文件', summary.scanned_files],
        ['候选对', summary.candidate_pairs],
        ['实际比较', summary.compared_pairs],
        ['分组数', report.groups.length],
      ];
      statsEl.innerHTML = items.map(([title, value]) => `
        <div class="stat">
          <div class="muted">${title}</div>
          <b>${value}</b>
        </div>
      `).join('');
    }

    function renderGroups() {
      const report = state.report;
      const filter = el('filterInput').value.trim().toLowerCase();
      if (!report) {
        groupsEl.innerHTML = '<div class="panel muted">暂无结果。</div>';
        return;
      }
      const html = report.groups
        .filter(group => {
          if (!filter) return true;
          return group.members.some(member =>
            member.file_name.toLowerCase().includes(filter) ||
            member.path.toLowerCase().includes(filter)
          );
        })
        .map(group => {
          const members = group.members.map(member => {
            const key = member.path;
            const checked = state.selectedDeletes.has(key) ? 'checked' : '';
            return `
              <div class="member ${member.keep_recommended ? 'keep' : ''}">
                <div class="member-top">
                  <div>
                    <div><strong>${escapeHtml(member.file_name)}</strong></div>
                    <div class="path">${escapeHtml(member.path)}</div>
                    <div class="hint ${member.keep_recommended ? '' : 'danger-hint'}">${escapeHtml(member.recommendation_reason)}</div>
                  </div>
                  <label><input type="checkbox" data-path="${escapeAttr(member.path)}" ${checked} ${member.keep_recommended ? 'disabled' : ''}/> 删除</label>
                </div>
              </div>
            `;
          }).join('');
          return `
            <div class="group">
              <div class="group-header">
                <div>
                  <span class="chip">${group.result_type}</span>
                  <strong>组 #${group.group_id}</strong>
                  <div class="muted">${escapeHtml(group.summary_reason)}</div>
                </div>
                <div>最高分 ${Number(group.max_score).toFixed(3)}</div>
              </div>
              <div class="members">${members}</div>
            </div>
          `;
        }).join('');
      groupsEl.innerHTML = html || '<div class="panel muted">没有匹配当前过滤条件的结果。</div>';
      groupsEl.querySelectorAll('input[type="checkbox"][data-path]').forEach(input => {
        input.addEventListener('change', (event) => {
          const path = event.target.getAttribute('data-path');
          if (event.target.checked) state.selectedDeletes.add(path);
          else state.selectedDeletes.delete(path);
        });
      });
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;');
    }
    function escapeAttr(value) {
      return escapeHtml(value).replaceAll("'", '&#39;');
    }

    async function refreshReport() {
      const report = await api('/api/report');
      state.report = report;
      updateStats(report);
      renderGroups();
      return report;
    }

    async function pollProgress() {
      try {
        const progress = await api('/api/progress');
        statusText.textContent = progress.message || '处理中...';
        let reportLoaded = !progress.finished_report_available;
        if (progress.finished_report_available) {
          await refreshReport();
          reportLoaded = true;
        }
        if (!progress.running && reportLoaded && state.pollTimer) {
          clearInterval(state.pollTimer);
          state.pollTimer = null;
        }
      } catch (error) {
        statusText.textContent = error.message;
      }
    }

    async function init() {
      try {
        loadConfigToForm(await api('/api/config'));
      } catch (error) {
        setConfigActionsEnabled(false);
        statusText.textContent = error.message;
        return;
      }
      await pollProgress();
      try {
        await refreshReport();
      } catch (error) {
        statusText.textContent = error.message;
      }
    }

    el('saveConfigBtn').addEventListener('click', async () => {
      if (!state.formInitialized) {
        statusText.textContent = '配置尚未加载完成，暂时无法保存。';
        return;
      }
      try {
        const result = await api('/api/config', { method: 'POST', body: JSON.stringify(currentConfigForSave()) });
        statusText.textContent = result.message;
      } catch (error) {
        statusText.textContent = error.message;
      }
    });

    el('pickFolderBtn').addEventListener('click', async () => {
      try {
        const result = await api('/api/pick-folder', { method: 'POST', body: '{}' });
        if (result.path) {
          const current = el('scanDirs').value.trim();
          el('scanDirs').value = current ? `${current}\n${result.path}` : result.path;
        }
      } catch (error) {
        statusText.textContent = error.message;
      }
    });

    el('scanBtn').addEventListener('click', async () => {
      if (!state.formInitialized) {
        statusText.textContent = '配置尚未加载完成，暂时无法开始扫描。';
        return;
      }
      try {
        state.selectedDeletes.clear();
        const result = await api('/api/scan', { method: 'POST', body: JSON.stringify(formToRequest()) });
        statusText.textContent = result.message;
        if (!state.pollTimer) state.pollTimer = setInterval(pollProgress, 1200);
        await pollProgress();
      } catch (error) {
        statusText.textContent = error.message;
      }
    });

    el('cancelBtn').addEventListener('click', async () => {
      const result = await api('/api/cancel', { method: 'POST', body: '{}' });
      statusText.textContent = result.message;
    });

    el('exportBtn').addEventListener('click', async () => {
      try {
        const result = await api('/api/export', {
          method: 'POST',
          body: JSON.stringify({
            format: el('exportFormat').value,
            file_name_prefix: el('exportNamePrefix').value.trim() || null,
          }),
        });
        statusText.textContent = result.message;
      } catch (error) {
        statusText.textContent = error.message;
      }
    });

    el('deleteBtn').addEventListener('click', async () => {
      if (!state.report || state.selectedDeletes.size === 0) {
        statusText.textContent = '请先勾选待删除文件。';
        return;
      }
      if (!confirm(`确认将 ${state.selectedDeletes.size} 个文件移入回收站吗？`)) return;
      try {
        const result = await api('/api/delete', {
          method: 'POST',
          body: JSON.stringify({ paths: [...state.selectedDeletes] }),
        });
        statusText.textContent = result.message;
        state.selectedDeletes.clear();
        state.report = null;
        updateStats(null);
        renderGroups();
      } catch (error) {
        statusText.textContent = error.message;
      }
    });

    el('filterInput').addEventListener('input', renderGroups);

    init();
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeleteResult, GroupMember, MatchGroup, MatchType, ScanReport, ScanSummary};
    use std::fs;
    use tempfile::{TempDir, tempdir};

    fn report_helper() -> (TempDir, WebState, PathBuf, PathBuf) {
        let root = tempdir().expect("tempdir");
        let keep_dir = root.path().join("keep");
        let delete_dir = root.path().join("delete");
        fs::create_dir_all(&keep_dir).expect("create keep dir");
        fs::create_dir_all(&delete_dir).expect("create delete dir");

        let keep = keep_dir.join("novel.txt");
        let delete = delete_dir.join("novel.txt");
        fs::write(&keep, b"keep file with more content").expect("write keep file");
        fs::write(&delete, b"x").expect("write delete file");

        let keep = keep.canonicalize().expect("canonical keep");
        let delete = delete.canonicalize().expect("canonical delete");
        let root_path = root.path().canonicalize().expect("canonical root");
        let config = AppConfig {
            scan_dirs: vec![root_path.clone()],
            extensions: vec!["txt".into()],
            ..AppConfig::default()
        };
        let engine = Engine::new(config.clone());
        let report = ScanReport {
            generated_at: "2026-03-24T10:00:00+08:00".into(),
            roots: vec![root_path],
            config,
            groups: vec![MatchGroup {
                group_id: 1,
                result_type: MatchType::Similar,
                summary_reason: "same title".into(),
                max_score: 0.95,
                recommended_keep_id: 1,
                members: vec![
                    GroupMember {
                        file_id: 1,
                        path: keep.clone(),
                        relative_path: PathBuf::from("keep/novel.txt"),
                        file_name: "novel.txt".into(),
                        extension: "txt".into(),
                        size: 32,
                        modified_ms: 2,
                        normalized_name: "novel".into(),
                        keep_recommended: true,
                        recommendation_reason: "largest".into(),
                    },
                    GroupMember {
                        file_id: 2,
                        path: delete.clone(),
                        relative_path: PathBuf::from("delete/novel.txt"),
                        file_name: "novel.txt".into(),
                        extension: "txt".into(),
                        size: 1,
                        modified_ms: 1,
                        normalized_name: "novel".into(),
                        keep_recommended: false,
                        recommendation_reason: "duplicate".into(),
                    },
                ],
                evidence: Vec::new(),
            }],
            summary: ScanSummary::default(),
            warnings: Vec::new(),
        };
        engine.store_completed_scan_for_tests(report);

        let latest = engine.latest_scan_state().expect("latest scan state");
        assert!(latest.keep_recommended.contains(&keep));
        assert!(latest.deletable.contains(&delete));

        let resolved_config_path = root.path().join("config.json");
        let export_dir = root.path().join("exports");

        (
            root,
            WebState {
                engine,
                resolved_config_path,
                export_dir,
            },
            keep,
            delete,
        )
    }

    #[tokio::test]
    async fn save_config_handler_writes_to_resolved_config_path() {
        let root = tempdir().expect("tempdir");
        let resolved_config_path = root.path().join("custom").join("config.json");
        let state = WebState {
            engine: Engine::new(AppConfig::default()),
            resolved_config_path: resolved_config_path.clone(),
            export_dir: root.path().join("exports"),
        };
        let config = AppConfig {
            ui_port: 9123,
            ..AppConfig::default()
        };

        let response = save_config_handler(State(state.clone()), Json(config.clone()))
            .await
            .expect("config save should succeed");

        assert_eq!(response.0.message, "配置已保存");
        assert_eq!(state.engine.config().ui_port, 9123);
        let saved = fs::read_to_string(&resolved_config_path).expect("read resolved config path");
        let saved = serde_json::from_str::<AppConfig>(&saved).expect("parse saved config");
        assert_eq!(saved.ui_port, 9123);
    }

    #[tokio::test]
    async fn start_scan_rejects_empty_effective_scan_dirs() {
        let root = tempdir().expect("tempdir");
        let state = WebState {
            engine: Engine::new(AppConfig {
                scan_dirs: vec![root.path().join("books")],
                ..AppConfig::default()
            }),
            resolved_config_path: root.path().join("config.json"),
            export_dir: root.path().join("exports"),
        };

        let error = start_scan(
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
        .expect_err("empty final scan dirs should be rejected");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "请至少选择一个扫描目录");
    }

    #[tokio::test]
    async fn export_handler_writes_to_safe_export_dir_and_returns_safe_file_name() {
        let (root, state, _keep, _delete) = report_helper();
        let requested_path = root.path().join("outside").join("custom-report.csv");

        let response = export_handler(
            State(state.clone()),
            Json(ExportRequest {
                format: Some("csv".to_string()),
                file_name_prefix: Some(requested_path.display().to_string()),
            }),
        )
        .await
        .expect("export should succeed");

        assert_eq!(response.0.message, "导出成功：custom-report.csv");
        assert_eq!(response.0.file_name.as_deref(), Some("custom-report.csv"));
        assert!(state.export_dir.join("custom-report.csv").exists());
        assert!(!requested_path.exists());
    }

    #[tokio::test]
    async fn export_handler_requires_current_report() {
        let root = tempdir().expect("tempdir");
        let state = WebState {
            engine: Engine::new(AppConfig::default()),
            resolved_config_path: root.path().join("config.json"),
            export_dir: root.path().join("exports"),
        };

        let error = export_handler(
            State(state),
            Json(ExportRequest {
                format: Some("json".to_string()),
                file_name_prefix: Some("report".to_string()),
            }),
        )
        .await
        .expect_err("export should require a current report");

        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.message, "暂无扫描结果");
    }

    #[test]
    fn index_html_uses_sanitized_export_request_shape() {
        assert!(INDEX_HTML.contains("file_name_prefix"));
        assert!(INDEX_HTML.contains("result.message"));
        assert!(!INDEX_HTML.contains("JSON.stringify({ path:"));
    }

    #[test]
    fn index_html_uses_separate_export_format_and_prefix_controls() {
        assert!(INDEX_HTML.contains("id=\"exportFormat\""));
        assert!(INDEX_HTML.contains("id=\"exportNamePrefix\""));
        assert!(INDEX_HTML.contains("format: el('exportFormat').value"));
        assert!(
            INDEX_HTML.contains("file_name_prefix: el('exportNamePrefix').value.trim() || null")
        );
        assert!(!INDEX_HTML.contains("id=\"exportPath\""));
        assert!(!INDEX_HTML.contains("导出文件名（.json / .csv）"));
    }

    #[tokio::test]
    async fn delete_handler_only_accepts_current_scan_candidates() {
        let (_root, state, keep, _delete) = report_helper();

        let error = delete_handler(
            State(state),
            Json(DeleteRequest {
                paths: vec![keep.display().to_string()],
            }),
        )
        .await
        .expect_err("keep-recommended path should be rejected");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "所选文件不属于当前扫描结果或不可删除");
    }

    #[tokio::test]
    async fn delete_handler_clears_completed_scan_state_after_success() {
        let (_root, state, _keep, delete) = report_helper();

        let response = delete_handler(
            State(state.clone()),
            Json(DeleteRequest {
                paths: vec![delete.display().to_string()],
            }),
        )
        .await
        .expect("delete candidate should be accepted");

        assert_eq!(response.0["ok"], serde_json::json!(true));
        assert_eq!(response.0["deleted_count"], serde_json::json!(1));
        assert!(state.engine.report().is_none());
        assert!(state.engine.latest_scan_state().is_none());
    }

    #[test]
    fn index_html_clears_stale_report_ui_after_successful_delete() {
        assert!(INDEX_HTML.contains("state.report = null;"));
        assert!(INDEX_HTML.contains("updateStats(null);"));
        assert!(INDEX_HTML.contains("renderGroups();"));
    }

    #[test]
    fn delete_log_failure_becomes_sanitized_warning() {
        let payload = build_delete_response(&DeleteResult {
            requested_count: 1,
            deleted_count: 1,
            deleted_paths: vec![PathBuf::from("C:/internal/delete.txt")],
            log_path: None,
            log_write_warning: Some("删除已完成，但删除日志写入失败".to_string()),
        });

        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(
            payload["message"],
            serde_json::json!("已移入回收站 1 个文件")
        );
        assert_eq!(payload["requested_count"], serde_json::json!(1));
        assert_eq!(payload["deleted_count"], serde_json::json!(1));
        assert_eq!(
            payload["log_write_warning"],
            serde_json::json!("删除已完成，但删除日志写入失败")
        );
        assert!(payload.get("log_path").is_none());
        assert!(payload.get("deleted_paths").is_none());
    }

    #[test]
    fn delete_success_payload_omits_internal_path_fields() {
        let payload = build_delete_response(&DeleteResult {
            requested_count: 2,
            deleted_count: 2,
            deleted_paths: vec![
                PathBuf::from("C:/internal/keep.txt"),
                PathBuf::from("C:/internal/delete.txt"),
            ],
            log_path: Some(PathBuf::from("C:/internal/logs/delete.json")),
            log_write_warning: None,
        });

        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(
            payload["message"],
            serde_json::json!("已移入回收站 2 个文件")
        );
        assert_eq!(payload["requested_count"], serde_json::json!(2));
        assert_eq!(payload["deleted_count"], serde_json::json!(2));
        assert_eq!(payload["log_write_warning"], serde_json::Value::Null);
        assert!(payload.get("log_path").is_none());
        assert!(payload.get("deleted_paths").is_none());
    }

    #[test]
    fn delete_partial_success_payload_is_sanitized_and_structured() {
        let payload = build_delete_response(&DeleteResult {
            requested_count: 2,
            deleted_count: 1,
            deleted_paths: vec![PathBuf::from("C:/internal/delete.txt")],
            log_path: None,
            log_write_warning: None,
        });

        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(payload["partial"], serde_json::json!(true));
        assert_eq!(payload["requested_count"], serde_json::json!(2));
        assert_eq!(payload["deleted_count"], serde_json::json!(1));
        assert_eq!(payload["failed_count"], serde_json::json!(1));
        assert_eq!(
            payload["message"],
            serde_json::json!("已移入回收站 1 个文件，另有 1 个文件删除失败")
        );
        assert!(payload.get("log_path").is_none());
        assert!(payload.get("deleted_paths").is_none());
    }

    #[tokio::test]
    async fn get_progress_hides_path_heavy_scan_errors() {
        let root = tempdir().expect("tempdir");
        let missing_scan_dir = root.path().join("missing-books");
        let engine = Engine::new(AppConfig::default());
        let config = AppConfig {
            scan_dirs: vec![missing_scan_dir.clone()],
            ..AppConfig::default()
        };

        let error = engine
            .run_scan(config)
            .expect_err("scan should fail for missing directory");
        assert!(
            error
                .to_string()
                .contains(&missing_scan_dir.display().to_string())
        );

        let progress = get_progress(State(WebState {
            engine,
            resolved_config_path: root.path().join("config.json"),
            export_dir: root.path().join("exports"),
        }))
        .await
        .0;

        assert_eq!(progress.stage, "error");
        assert_eq!(progress.message, "扫描失败，请检查扫描目录后重试");
        assert!(
            !progress
                .message
                .contains(&missing_scan_dir.display().to_string())
        );
    }

    #[test]
    fn sanitize_web_error_uses_stable_picker_message() {
        let error = sanitize_web_error(anyhow::anyhow!("选择目录失败: 无法访问 C:/secret/books"));

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "选择目录失败");
    }

    #[test]
    fn sanitize_web_error_hides_unknown_internal_messages() {
        let error = sanitize_web_error(anyhow::anyhow!(
            "内部错误: C:/secret/books/library.db 已被占用"
        ));

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "操作失败，请重试");
        assert!(!error.message.contains("C:/secret/books"));
    }

    #[test]
    fn sanitize_delete_error_hides_unknown_internal_messages() {
        let error =
            sanitize_delete_error(anyhow::anyhow!("内部删除错误: C:/secret/books/delete.txt"));

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "删除失败，请重试");
        assert!(!error.message.contains("C:/secret/books/delete.txt"));
    }
}
