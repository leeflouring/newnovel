from __future__ import annotations

import argparse
import csv
import json
import math
import re
import threading
import unicodedata
import webbrowser
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from itertools import combinations
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse


HTML_PAGE = r'''<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>小说同名筛选器</title>
  <style>
    @import url("https://fonts.googleapis.com/css2?family=ZCOOL+XiaoWei&family=Noto+Serif+SC:wght@400;600;700&display=swap");

    :root {
      --bg-1: #f6f2e8;
      --bg-2: #e8dfcd;
      --card: #fffaf0;
      --ink: #29231d;
      --muted: #6b6157;
      --accent: #a3382b;
      --accent-soft: #d46b4b;
      --line: #d8ccbd;
      --ok: #2f7a57;
      --shadow: 0 12px 32px rgba(44, 28, 17, 0.12);
    }

    * { box-sizing: border-box; }

    body {
      margin: 0;
      color: var(--ink);
      background:
        radial-gradient(circle at 10% 12%, rgba(211, 131, 96, 0.18), transparent 38%),
        radial-gradient(circle at 90% 22%, rgba(123, 91, 56, 0.13), transparent 40%),
        linear-gradient(135deg, var(--bg-1), var(--bg-2));
      font-family: "Noto Serif SC", "Microsoft YaHei", serif;
      min-height: 100vh;
    }

    .frame {
      max-width: 1140px;
      margin: 30px auto;
      padding: 20px;
    }

    .header {
      margin-bottom: 16px;
      padding: 18px 20px;
      border: 1px solid var(--line);
      border-radius: 16px;
      background: linear-gradient(180deg, rgba(255, 248, 233, 0.96), rgba(255, 245, 223, 0.88));
      box-shadow: var(--shadow);
    }

    .title {
      margin: 0;
      font-size: 30px;
      letter-spacing: 1px;
      font-family: "ZCOOL XiaoWei", serif;
    }

    .subtitle {
      margin: 10px 0 0;
      color: var(--muted);
      font-size: 14px;
      line-height: 1.5;
    }

    .panel {
      display: grid;
      grid-template-columns: 1fr;
      gap: 14px;
      margin-bottom: 16px;
      padding: 18px;
      border: 1px solid var(--line);
      border-radius: 16px;
      background: var(--card);
      box-shadow: var(--shadow);
    }

    .grid {
      display: grid;
      grid-template-columns: repeat(12, minmax(0, 1fr));
      gap: 10px;
    }

    .field {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    .field label {
      font-size: 13px;
      color: var(--muted);
    }

    .field input[type="text"],
    .field input[type="number"] {
      border: 1px solid var(--line);
      border-radius: 10px;
      padding: 10px 12px;
      background: #fff;
      color: var(--ink);
      font-size: 14px;
      outline: none;
    }

    .field input:focus {
      border-color: var(--accent-soft);
      box-shadow: 0 0 0 3px rgba(212, 107, 75, 0.2);
    }

    .span-12 { grid-column: span 12; }
    .span-8 { grid-column: span 8; }
    .span-4 { grid-column: span 4; }
    .span-3 { grid-column: span 3; }
    .span-2 { grid-column: span 2; }

    .checks {
      display: flex;
      align-items: center;
      gap: 12px;
      flex-wrap: wrap;
      font-size: 14px;
    }

    .checks label {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      color: var(--ink);
    }

    .actions {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 10px;
      margin-top: 2px;
    }

    button {
      border: none;
      border-radius: 10px;
      padding: 10px 18px;
      font-size: 14px;
      cursor: pointer;
      transition: transform 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
    }

    button:hover {
      transform: translateY(-1px);
      box-shadow: 0 8px 18px rgba(161, 56, 43, 0.22);
    }

    .primary {
      background: linear-gradient(135deg, #b53f31, #893226);
      color: #fff;
    }

    .secondary {
      background: #efe3d2;
      color: #5f4b36;
      border: 1px solid #d8c8b4;
    }

    .secondary:disabled { opacity: 0.5; cursor: not-allowed; }

    .status {
      font-size: 13px;
      color: var(--muted);
      min-height: 20px;
    }

    .summary {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: 10px;
      margin-bottom: 14px;
    }

    .metric {
      border: 1px solid var(--line);
      border-radius: 12px;
      background: rgba(255, 250, 241, 0.9);
      padding: 12px;
    }

    .metric .k {
      font-size: 12px;
      color: var(--muted);
    }

    .metric .v {
      margin-top: 6px;
      font-size: 24px;
      font-family: "ZCOOL XiaoWei", serif;
      color: #4b3224;
    }

    .results {
      display: grid;
      gap: 12px;
    }

    .group {
      border: 1px solid var(--line);
      border-radius: 14px;
      background: #fffdf7;
      overflow: hidden;
    }

    .group-head {
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 10px;
      padding: 12px 14px;
      background: linear-gradient(180deg, rgba(243, 232, 214, 0.85), rgba(245, 236, 222, 0.65));
      border-bottom: 1px solid var(--line);
    }

    .group-head h3 {
      margin: 0;
      font-size: 16px;
      font-family: "ZCOOL XiaoWei", serif;
    }

    .group-right {
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      gap: 8px;
    }

    .group-actions {
      display: flex;
      gap: 8px;
      flex-wrap: wrap;
      justify-content: flex-end;
    }

    .mini-btn {
      border: 1px solid #d8c8b4;
      border-radius: 8px;
      padding: 6px 10px;
      font-size: 12px;
      line-height: 1.2;
      background: #f6ebdc;
      color: #5f4b36;
      cursor: pointer;
    }

    .mini-btn.danger {
      background: #f6d9d3;
      border-color: #e4b0a5;
      color: #7b2b22;
    }

    .mini-btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    .group.collapsed .group-body {
      display: none;
    }

    .group.collapsed .group-head {
      border-bottom: none;
    }

    .chips {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-top: 8px;
    }

    .chip {
      border-radius: 999px;
      padding: 2px 10px;
      font-size: 12px;
      background: #f7ece0;
      border: 1px solid #e6d6c3;
      color: #6a4a34;
    }

    .file-list {
      width: 100%;
      border-collapse: collapse;
      font-size: 13px;
    }

    .file-list th,
    .file-list td {
      text-align: left;
      padding: 8px 10px;
      border-bottom: 1px solid #efe4d7;
      vertical-align: top;
      word-break: break-all;
    }

    .file-list th {
      color: var(--muted);
      font-weight: 600;
      background: rgba(250, 245, 236, 0.85);
    }

    .muted {
      color: var(--muted);
      font-size: 12px;
    }

    @media (max-width: 960px) {
      .summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      .span-8, .span-4, .span-3, .span-2 { grid-column: span 12; }
      .group-head {
        flex-direction: column;
      }
      .group-right {
        align-items: flex-start;
      }
      .group-actions {
        justify-content: flex-start;
      }
    }
  </style>
</head>
<body>
  <main class="frame">
    <section class="header">
      <h1 class="title">小说同名筛选器</h1>
      <p class="subtitle">按文件名中的连续字符片段进行分组。支持 2-6（可扩展）字顺序匹配；命中长片段会优先判定同组，常用词（如 妈妈、姐姐、fgo、coser）可作为停用词过滤，降低误判。</p>
    </section>

    <section class="panel">
      <div class="grid">
        <div class="field span-12">
          <label for="folderPath">扫描目录</label>
          <input id="folderPath" type="text" placeholder="例如 C:/Users/lisheng/Desktop/1" />
        </div>

        <div class="field span-4">
          <label for="exts">文件后缀（逗号分隔）</label>
          <input id="exts" type="text" value=".txt,.doc,.docx,.epub" />
        </div>

        <div class="field span-8">
          <label for="stopwords">停用词（逗号分隔，命中即忽略，如 妈妈,姐姐,fgo,coser）</label>
          <input id="stopwords" type="text" value="" />
        </div>

        <div class="field span-2">
          <label for="minLen">最小片段长度</label>
          <input id="minLen" type="number" min="2" max="12" value="2" />
        </div>

        <div class="field span-2">
          <label for="maxLen">最大片段长度</label>
          <input id="maxLen" type="number" min="2" max="12" value="6" />
        </div>

        <div class="field span-2">
          <label for="minPairMatches">仅 2 字匹配时最少命中数</label>
          <input id="minPairMatches" type="number" min="1" value="2" />
        </div>

        <div class="field span-3">
          <label for="maxDfAbs">片段最大文档频次（绝对值）</label>
          <input id="maxDfAbs" type="number" min="2" value="120" />
        </div>

        <div class="field span-3">
          <label for="maxDfRatio">片段最大文档频次（百分比）</label>
          <input id="maxDfRatio" type="number" min="1" max="100" value="4" />
        </div>

        <div class="field span-12">
          <label>匹配片段长度（连续且顺序一致）</label>
          <div class="checks" id="lengthChecks"></div>
          <div class="muted">勾选或取消后会自动刷新预览。</div>
        </div>
      </div>

      <div class="actions">
        <button id="runBtn" class="primary">开始分析</button>
        <button id="csvBtn" class="secondary" disabled>导出当前结果 CSV</button>
        <span id="status" class="status">等待开始</span>
      </div>
    </section>

    <section id="summary" class="summary" hidden></section>
    <section id="groupControls" class="actions" hidden>
      <button id="expandAllBtn" class="secondary" type="button">全部展开</button>
      <button id="collapseAllBtn" class="secondary" type="button">全部收起</button>
    </section>
    <section id="results" class="results"></section>
  </main>

  <script>
    const state = {
      latest: null,
      autoTimer: null,
      collapsedGroupByKey: {},
      deleteInProgress: false,
    };

    const folderPathInput = document.getElementById("folderPath");
    const extsInput = document.getElementById("exts");
    const stopwordsInput = document.getElementById("stopwords");
    const minLenInput = document.getElementById("minLen");
    const maxLenInput = document.getElementById("maxLen");
    const minPairMatchesInput = document.getElementById("minPairMatches");
    const maxDfAbsInput = document.getElementById("maxDfAbs");
    const maxDfRatioInput = document.getElementById("maxDfRatio");
    const lengthChecksEl = document.getElementById("lengthChecks");
    const runBtn = document.getElementById("runBtn");
    const csvBtn = document.getElementById("csvBtn");
    const statusEl = document.getElementById("status");
    const summaryEl = document.getElementById("summary");
    const groupControlsEl = document.getElementById("groupControls");
    const expandAllBtn = document.getElementById("expandAllBtn");
    const collapseAllBtn = document.getElementById("collapseAllBtn");
    const resultsEl = document.getElementById("results");

    function clampInt(value, minV, maxV) {
      if (!Number.isFinite(value)) {
        return minV;
      }
      return Math.min(maxV, Math.max(minV, Math.trunc(value)));
    }

    function getLengthRange() {
      let minLen = clampInt(Number(minLenInput.value || 2), 2, 12);
      let maxLen = clampInt(Number(maxLenInput.value || 6), 2, 12);
      if (minLen > maxLen) {
        const t = minLen;
        minLen = maxLen;
        maxLen = t;
      }
      minLenInput.value = String(minLen);
      maxLenInput.value = String(maxLen);
      return [minLen, maxLen];
    }

    function buildLengthChecks() {
      const selected = new Set(getSelectedLengths());
      const [minLen, maxLen] = getLengthRange();
      lengthChecksEl.innerHTML = "";

      for (let len = minLen; len <= maxLen; len += 1) {
        const label = document.createElement("label");
        const input = document.createElement("input");
        input.type = "checkbox";
        input.value = String(len);
        input.checked = selected.size ? selected.has(len) : true;
        input.addEventListener("change", () => scheduleAutoPreview("长度勾选已变化，正在刷新预览..."));
        label.append(input);
        label.append(` ${len} 字`);
        lengthChecksEl.append(label);
      }

      if (!getSelectedLengths().length) {
        const first = lengthChecksEl.querySelector("input[type='checkbox']");
        if (first) {
          first.checked = true;
        }
      }
    }

    function getSelectedLengths() {
      const checks = document.querySelectorAll("#lengthChecks input[type='checkbox']");
      return [...checks]
        .filter((el) => el.checked)
        .map((el) => Number(el.value))
        .sort((a, b) => a - b);
    }

    function escapeHtml(text) {
      return String(text)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }

    function setStatus(text) {
      statusEl.textContent = text;
    }

    function stemFromFilename(name) {
      const idx = String(name).lastIndexOf(".");
      return idx > 0 ? String(name).slice(0, idx) : String(name);
    }

    function filenameFromPath(path) {
      const str = String(path || "");
      const parts = str.split(/[\\/]/);
      return parts[parts.length - 1] || str;
    }

    function formatSizeFromBytes(bytes) {
      const units = ["B", "KB", "MB", "GB", "TB"];
      let value = Number(bytes || 0);
      let unitIdx = 0;
      while (value >= 1024 && unitIdx < units.length - 1) {
        value /= 1024;
        unitIdx += 1;
      }
      if (unitIdx === 0) {
        return `${Math.trunc(value)} ${units[unitIdx]}`;
      }
      return `${value.toFixed(2)} ${units[unitIdx]}`;
    }

    function createGroupKey(group) {
      const snippets = [...(group.shared_snippets || [])]
        .map((s) => String(s))
        .sort()
        .slice(0, 6);
      if (snippets.length) {
        return `snip::${snippets.join("|")}`;
      }
      const rep = String(group.representative || "").replace(/[0-9\s_]+/g, "");
      if (rep) {
        return `rep::${rep}`;
      }
      const names = [...(group.files || [])]
        .map((f) => stemFromFilename(String(f.name || "")))
        .sort();
      return `name::${names.join("|")}`;
    }

    function recomputeGroupMeta(group) {
      group.size = group.files.length;
      const latestSize = Math.max(...group.files.map((f) => Number(f.size_bytes || 0)));
      group.latest_size_bytes = latestSize;
      group.latest_size_text = formatSizeFromBytes(latestSize);
      group.old_file_count = 0;
      group.files.forEach((f) => {
        f.is_latest_by_size = Number(f.size_bytes || 0) >= latestSize;
        f.size_text = formatSizeFromBytes(Number(f.size_bytes || 0));
        if (!f.is_latest_by_size) {
          group.old_file_count += 1;
        }
      });
      if (group.files[0]?.name) {
        group.representative = stemFromFilename(group.files[0].name);
      }
    }

    function applyLocalDeletion(deletedPaths) {
      if (!state.latest || !Array.isArray(deletedPaths) || !deletedPaths.length) {
        return 0;
      }
      const oldGroups = state.latest.groups.map((g) => ({
        key: createGroupKey(g),
        paths: new Set((g.files || []).map((f) => String(f.path))),
      }));
      const deletedSet = new Set(deletedPaths.map((p) => String(p)));
      let removedCount = 0;
      const nextGroups = [];

      state.latest.groups.forEach((group) => {
        const before = group.files.length;
        group.files = group.files.filter((f) => !deletedSet.has(String(f.path)));
        removedCount += before - group.files.length;
        if (group.files.length >= 2) {
          recomputeGroupMeta(group);
          nextGroups.push(group);
        }
      });

      state.latest.groups = nextGroups;
      state.latest.group_count = nextGroups.length;
      state.latest.duplicate_file_count = nextGroups.reduce((acc, g) => acc + g.files.length, 0);
      state.latest.total_files = Math.max(0, Number(state.latest.total_files || 0) - removedCount);

      const nextCollapsed = {};
      nextGroups.forEach((g) => {
        const newKey = createGroupKey(g);
        const newPaths = new Set((g.files || []).map((f) => String(f.path)));

        let bestKey = "";
        let bestOverlap = -1;
        oldGroups.forEach((og) => {
          let overlap = 0;
          newPaths.forEach((p) => {
            if (og.paths.has(p)) {
              overlap += 1;
            }
          });
          if (overlap > bestOverlap) {
            bestOverlap = overlap;
            bestKey = og.key;
          }
        });

        if (bestKey && Object.prototype.hasOwnProperty.call(state.collapsedGroupByKey, bestKey)) {
          nextCollapsed[newKey] = Boolean(state.collapsedGroupByKey[bestKey]);
          return;
        }
        nextCollapsed[newKey] = Boolean(state.collapsedGroupByKey[newKey]);
      });
      state.collapsedGroupByKey = nextCollapsed;
      return removedCount;
    }

    function setDeleteBusy(busy) {
      state.deleteInProgress = busy;
      document.querySelectorAll(".file-delete-btn,.group-delete-old-btn").forEach((btn) => {
        if (btn instanceof HTMLButtonElement) {
          btn.disabled = busy || btn.dataset.fixedDisabled === "true";
        }
      });
    }

    function setGroupCollapsedState(collapsed) {
      document.querySelectorAll("#results .group").forEach((el) => {
        el.classList.toggle("collapsed", collapsed);
        const keyEncoded = el.getAttribute("data-group-key") || "";
        if (keyEncoded) {
          const key = decodeURIComponent(keyEncoded);
          state.collapsedGroupByKey[key] = collapsed;
        }
      });
      document.querySelectorAll("#results .toggle-group-btn").forEach((btn) => {
        btn.textContent = collapsed ? "展开" : "收起";
      });
    }

    function scheduleAutoPreview(message) {
      if (!state.latest || runBtn.disabled) {
        return;
      }
      if (state.autoTimer) {
        clearTimeout(state.autoTimer);
      }
      setStatus(message);
      state.autoTimer = setTimeout(() => {
        runAnalysis(true);
      }, 220);
    }

    function renderSummary(result) {
      summaryEl.hidden = false;
      const cards = [
        ["扫描文件总数", result.total_files],
        ["候选分组数", result.group_count],
        ["候选文件数", result.duplicate_file_count],
        ["单文件数量", result.total_files - result.duplicate_file_count],
      ];

      summaryEl.innerHTML = cards
        .map(([k, v]) => `
          <article class="metric">
            <div class="k">${escapeHtml(k)}</div>
            <div class="v">${escapeHtml(v)}</div>
          </article>
        `)
        .join("");
    }

    function renderGroupArticle(group, idx) {
      const groupKey = createGroupKey(group);
      const groupKeyEncoded = encodeURIComponent(groupKey);
      const collapsed = Boolean(state.collapsedGroupByKey[groupKey]);
      const filesRows = group.files
        .map((file) => `
          <tr>
                <td>${file.is_latest_by_size ? "<span style='color:#2f7a57;font-weight:700;'>最新</span>" : "旧"}</td>
                <td>${escapeHtml(file.name)}</td>
                <td>${escapeHtml(file.modified)}</td>
                <td>${escapeHtml(file.size_text)}</td>
            <td><button class="mini-btn danger file-delete-btn" data-file-path="${encodeURIComponent(file.path)}" ${state.deleteInProgress ? "disabled" : ""}>删除</button></td>
          </tr>
        `)
        .join("");

      const chips = group.shared_snippets
        .map((s) => `<span class="chip">${escapeHtml(s)}</span>`)
        .join("");

      return `
        <article class="group${collapsed ? " collapsed" : ""}" data-group-index="${idx}" data-group-key="${groupKeyEncoded}">
          <div class="group-head">
            <div>
              <h3>分组 ${idx + 1} · ${group.size} 个文件</h3>
              <div class="muted">代表名：${escapeHtml(group.representative)}</div>
              <div class="chips">${chips || "<span class='chip'>无高置信公共片段</span>"}</div>
            </div>
            <div class="group-right">
              <div class="muted">命中长度统计：${escapeHtml(group.length_stats_text)}</div>
              <div class="muted">最大文件大小：${escapeHtml(group.latest_size_text)}</div>
              <div class="group-actions">
                <button class="mini-btn toggle-group-btn" data-group-key="${groupKeyEncoded}">${collapsed ? "展开" : "收起"}</button>
                <button class="mini-btn danger group-delete-old-btn" data-group-index="${idx}" data-fixed-disabled="${group.old_file_count === 0 ? "true" : "false"}" ${group.old_file_count === 0 || state.deleteInProgress ? "disabled" : ""}>删除旧文件（保留最大）</button>
              </div>
            </div>
          </div>
          <div class="group-body">
            <table class="file-list">
              <thead>
                <tr>
                  <th style="width: 70px;">新旧</th>
                  <th>文件名</th>
                  <th style="width: 180px;">修改时间</th>
                  <th style="width: 120px;">大小</th>
                  <th style="width: 92px;">操作</th>
                </tr>
              </thead>
              <tbody>
                ${filesRows}
              </tbody>
            </table>
          </div>
        </article>
      `;
    }

    function renderResults(result) {
      if (!result.groups.length) {
        groupControlsEl.hidden = true;
        resultsEl.innerHTML = "<div class='muted'>未发现满足条件的分组，请尝试放宽或收紧参数。</div>";
        return;
      }

      groupControlsEl.hidden = false;
      resultsEl.innerHTML = result.groups
        .map((group, idx) => renderGroupArticle(group, idx))
        .join("");
    }

    async function runAnalysis(isAutoRefresh = false) {
      const lengths = getSelectedLengths();
      if (!lengths.length) {
        setStatus("请至少选择一个片段长度。");
        return;
      }

      const payload = {
        folder_path: folderPathInput.value.trim(),
        extensions: extsInput.value.trim(),
        stopwords: stopwordsInput.value.trim(),
        lengths,
        min_pair_matches: Number(minPairMatchesInput.value || 2),
        max_df_abs: Number(maxDfAbsInput.value || 120),
        max_df_ratio: Number(maxDfRatioInput.value || 4) / 100,
      };
      runBtn.disabled = true;
      csvBtn.disabled = true;
      setStatus("分析中，请稍候...");
      groupControlsEl.hidden = true;
      resultsEl.innerHTML = "";
      summaryEl.hidden = true;

      try {
        const resp = await fetch("/api/analyze", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || "分析失败");
        }

        state.latest = data;
        if (data.folder) {
          folderPathInput.value = data.folder;
        }
        renderSummary(data);
        renderResults(data);
        csvBtn.disabled = !data.groups.length;
        const triggerText = isAutoRefresh ? "自动刷新" : "完成";
        setStatus(`${triggerText}：扫描 ${data.total_files} 个文件，得到 ${data.group_count} 个候选分组。`);
      } catch (err) {
        setStatus(`失败：${err.message}`);
      } finally {
        runBtn.disabled = false;
      }
    }

    async function saveDefaultPath() {
      const path = folderPathInput.value.trim();
      if (!path) {
        return;
      }
      try {
        const resp = await fetch("/api/default-path", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ default_path: path }),
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || "保存默认目录失败");
        }
        folderPathInput.value = data.default_path || path;
      } catch (err) {
        setStatus(`默认目录保存失败：${err.message}`);
      }
    }

    function collectAffectedGroupKeys(paths) {
      const affected = new Set();
      if (!state.latest) {
        return affected;
      }
      const deletedSet = new Set(paths.map((p) => String(p)));
      state.latest.groups.forEach((group) => {
        if ((group.files || []).some((f) => deletedSet.has(String(f.path)))) {
          affected.add(createGroupKey(group));
        }
      });
      return affected;
    }

    function patchResultsAfterDeletion(affectedGroupKeys) {
      if (!state.latest) {
        return;
      }

      if (!state.latest.groups.length) {
        groupControlsEl.hidden = true;
        resultsEl.innerHTML = "<div class='muted'>未发现满足条件的分组，请尝试放宽或收紧参数。</div>";
        return;
      }

      groupControlsEl.hidden = false;
      const nextGroups = state.latest.groups;
      const nextKeySet = new Set(nextGroups.map((g) => createGroupKey(g)));

      resultsEl.querySelectorAll(".group").forEach((article) => {
        const keyEncoded = article.getAttribute("data-group-key") || "";
        const key = keyEncoded ? decodeURIComponent(keyEncoded) : "";
        if (!nextKeySet.has(key)) {
          article.remove();
        }
      });

      nextGroups.forEach((group, idx) => {
        const key = createGroupKey(group);
        const keyEncoded = encodeURIComponent(key);
        const selector = `.group[data-group-key="${keyEncoded}"]`;
        const current = resultsEl.querySelector(selector);

        if (!current || affectedGroupKeys.has(key)) {
          const template = document.createElement("template");
          template.innerHTML = renderGroupArticle(group, idx).trim();
          const nextEl = template.content.firstElementChild;
          if (!nextEl) {
            return;
          }
          if (current) {
            current.replaceWith(nextEl);
          } else {
            resultsEl.appendChild(nextEl);
          }
          return;
        }

        current.setAttribute("data-group-index", String(idx));
        const h3 = current.querySelector("h3");
        if (h3) {
          h3.textContent = `分组 ${idx + 1} · ${group.size} 个文件`;
        }
        const deleteOldBtn = current.querySelector(".group-delete-old-btn");
        if (deleteOldBtn instanceof HTMLButtonElement) {
          deleteOldBtn.dataset.groupIndex = String(idx);
          deleteOldBtn.dataset.fixedDisabled = group.old_file_count === 0 ? "true" : "false";
          deleteOldBtn.disabled = group.old_file_count === 0 || state.deleteInProgress;
        }
      });
    }

    async function deleteFiles(paths, purpose) {
      if (!paths.length) {
        setStatus("没有可删除的文件。");
        return;
      }
      if (state.deleteInProgress) {
        return;
      }

      const preview = paths
        .slice(0, 3)
        .map((p) => `- ${filenameFromPath(p)}`)
        .join("\n");
      const suffix = paths.length > 3 ? `\n- ...其余 ${paths.length - 3} 个文件` : "";
      const confirmText = `⚠️ 危险操作检测！\n操作类型：删除文件\n影响范围：${paths.length} 个文件\n风险评估：删除后不可恢复\n\n预览：\n${preview}${suffix}\n\n请确认是否继续？`;
      if (!window.confirm(confirmText)) {
        return;
      }

      const affectedGroupKeys = collectAffectedGroupKeys(paths);
      setDeleteBusy(true);
      try {
        const resp = await fetch("/api/delete-files", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            folder_path: folderPathInput.value.trim(),
            paths,
          }),
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || "删除失败");
        }

        const deletedPaths = Array.isArray(data.deleted) ? data.deleted : [];
        const removedByUi = applyLocalDeletion(deletedPaths);
        const deletedCount = removedByUi || Number(data.deleted_count || 0);
        const keepX = window.scrollX;
        const keepY = window.scrollY;
        if (state.latest) {
          renderSummary(state.latest);
          patchResultsAfterDeletion(affectedGroupKeys);
          csvBtn.disabled = !state.latest.groups.length;
        }
        window.requestAnimationFrame(() => window.scrollTo(keepX, keepY));
        const failed = data.failed_count || 0;
        setStatus(`${purpose}完成：成功删除 ${deletedCount} 个文件${failed ? `，失败 ${failed} 个` : ""}。`);
      } catch (err) {
        setStatus(`删除失败：${err.message}`);
      } finally {
        setDeleteBusy(false);
      }
    }

    async function saveStopwords() {
      try {
        const resp = await fetch("/api/stopwords", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ stopwords: stopwordsInput.value.trim() }),
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || "保存停用词失败");
        }
        stopwordsInput.value = data.stopwords || "";
        scheduleAutoPreview("停用词已更新，正在刷新预览...");
        if (!runBtn.disabled) {
          setStatus("停用词已保存，下次启动会自动加载。");
        }
      } catch (err) {
        setStatus(`停用词保存失败：${err.message}`);
      }
    }

    function onResultsClick(event) {
      const target = event.target;
      if (!(target instanceof HTMLElement)) {
        return;
      }

      const toggleBtn = target.closest(".toggle-group-btn");
      if (toggleBtn) {
        const keyEncoded = String(toggleBtn.dataset.groupKey || "");
        const groupEl = resultsEl.querySelector(`.group[data-group-key="${keyEncoded}"]`);
        if (groupEl) {
          const collapsed = groupEl.classList.toggle("collapsed");
          toggleBtn.textContent = collapsed ? "展开" : "收起";
          if (keyEncoded) {
            const key = decodeURIComponent(keyEncoded);
            state.collapsedGroupByKey[key] = collapsed;
          }
        }
        return;
      }

      const deleteOldBtn = target.closest(".group-delete-old-btn");
      if (deleteOldBtn && state.latest) {
        const groupIdx = Number(deleteOldBtn.dataset.groupIndex);
        const group = state.latest.groups[groupIdx];
        if (!group) {
          return;
        }
        const oldPaths = group.files
          .filter((f) => !f.is_latest_by_size)
          .map((f) => f.path);
        deleteFiles(oldPaths, `分组 ${groupIdx + 1} 删除旧文件`);
        return;
      }

      const deleteFileBtn = target.closest(".file-delete-btn");
      if (deleteFileBtn) {
        const pathEncoded = String(deleteFileBtn.dataset.filePath || "");
        if (!pathEncoded) {
          return;
        }
        const path = decodeURIComponent(pathEncoded);
        const name = path.split(/[\\/]/).pop() || path;
        deleteFiles([path], `删除文件 ${name}`);
      }
    }

    function exportCsv() {
      if (!state.latest || !state.latest.groups.length) {
        return;
      }
      const rows = [["group_id", "is_latest_by_size", "file_name", "modified", "size", "path", "shared_snippets"]];
      state.latest.groups.forEach((group, gi) => {
        const snippet = group.shared_snippets.join("|");
        group.files.forEach((file) => {
          rows.push([
            String(gi + 1),
            file.is_latest_by_size ? "yes" : "no",
            file.name,
            file.modified,
            file.size_text,
            file.path,
            snippet,
          ]);
        });
      });

      const csvText = rows
        .map((r) => r.map((x) => `"${String(x).replaceAll('"', '""')}"`).join(","))
        .join("\n");

      const blob = new Blob(["\ufeff" + csvText], { type: "text/csv;charset=utf-8;" });
      const link = document.createElement("a");
      link.href = URL.createObjectURL(blob);
      link.download = `novel_groups_${Date.now()}.csv`;
      link.click();
      URL.revokeObjectURL(link.href);
    }

    runBtn.addEventListener("click", runAnalysis);
    csvBtn.addEventListener("click", exportCsv);
    expandAllBtn.addEventListener("click", () => setGroupCollapsedState(false));
    collapseAllBtn.addEventListener("click", () => setGroupCollapsedState(true));
    resultsEl.addEventListener("click", onResultsClick);
    minLenInput.addEventListener("change", () => {
      buildLengthChecks();
      scheduleAutoPreview("长度范围已变化，正在刷新预览...");
    });
    maxLenInput.addEventListener("change", () => {
      buildLengthChecks();
      scheduleAutoPreview("长度范围已变化，正在刷新预览...");
    });
    stopwordsInput.addEventListener("change", saveStopwords);
    folderPathInput.addEventListener("change", saveDefaultPath);
    folderPathInput.value = "__DEFAULT_PATH__";
    stopwordsInput.value = __DEFAULT_STOPWORDS_JSON__;
    buildLengthChecks();
  </script>
</body>
</html>
'''


PATTERN_KEEP = re.compile(r"[0-9a-z\u4e00-\u9fff]+")
SPACE_RE = re.compile(r"\s+")

DEFAULT_STOPWORDS = [
    "妈妈",
    "姐姐",
    "妹妹",
    "哥哥",
    "弟弟",
    "指挥官",
    "调教",
    "作者",
    "搜书吧",
    "第一人称",
    "完整版",
    "修订版",
    "番外",
    "fgo",
    "coser",
    "cos",
]
CONFIG_FILENAME = "novel_similarity_webui_config.json"
DEFAULT_SCAN_DIR = "H:/桌面/CRNovel/CRNovel1"


@dataclass(frozen=True)
class FileMeta:
    name: str
    path: str
    stem: str
    normalized: str
    size_bytes: int
    modified_ts: float


class UnionFind:
    def __init__(self, size: int) -> None:
        self.parent = list(range(size))
        self.rank = [0] * size

    def find(self, x: int) -> int:
        while self.parent[x] != x:
            self.parent[x] = self.parent[self.parent[x]]
            x = self.parent[x]
        return x

    def union(self, a: int, b: int) -> None:
        ra = self.find(a)
        rb = self.find(b)
        if ra == rb:
            return
        if self.rank[ra] < self.rank[rb]:
            self.parent[ra] = rb
            return
        if self.rank[ra] > self.rank[rb]:
            self.parent[rb] = ra
            return
        self.parent[rb] = ra
        self.rank[ra] += 1


def normalize_title(stem: str) -> str:
    s = unicodedata.normalize("NFKC", stem).lower()
    return "".join(PATTERN_KEEP.findall(s))


def normalize_word(raw: str) -> str:
    s = unicodedata.normalize("NFKC", raw).lower()
    return "".join(PATTERN_KEEP.findall(s))


def parse_stopwords(raw: str) -> list[str]:
    parts = re.split(r"[,\s;，；、]+", raw)
    out = []
    seen: set[str] = set()
    for part in parts:
        token = normalize_word(part)
        if not token:
            continue
        if token in seen:
            continue
        seen.add(token)
        out.append(token)
    out.sort(key=len, reverse=True)
    return out


def load_config(config_path: Path) -> dict[str, Any]:
    if not config_path.exists():
        return {}

    try:
        data = json.loads(config_path.read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001
        return {}

    if not isinstance(data, dict):
        return {}
    return data


def save_config(config_path: Path, updates: dict[str, Any]) -> dict[str, Any]:
    data = load_config(config_path)
    data.update(updates)
    config_path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    return data


def load_saved_stopwords(config_path: Path, fallback: str) -> str:
    data = load_config(config_path)
    if not data:
        return fallback

    raw = str(data.get("stopwords", "")).strip()
    if raw == "":
        return ""

    tokens = parse_stopwords(raw)
    if not tokens:
        return fallback
    return ",".join(tokens)


def load_saved_default_path(config_path: Path, fallback: str) -> str:
    data = load_config(config_path)
    if not data:
        return fallback

    raw = str(data.get("default_path", "")).strip()
    if not raw:
        return fallback
    return raw


def save_stopwords(config_path: Path, stopwords_text: str) -> str:
    normalized = ",".join(parse_stopwords(stopwords_text)) if stopwords_text.strip() else ""
    save_config(config_path, {"stopwords": normalized})
    return normalized


def save_default_path(config_path: Path, default_path: str) -> str:
    normalized = str(Path(default_path).resolve())
    save_config(config_path, {"default_path": normalized})
    return normalized


def remove_stopwords(text: str, stopwords: list[str]) -> str:
    if not stopwords:
        return text
    cleaned = text
    for word in stopwords:
        cleaned = cleaned.replace(word, " ")
    return SPACE_RE.sub(" ", cleaned).strip()


def split_extensions(raw: str) -> set[str]:
    exts: set[str] = set()
    for part in raw.split(","):
        part = part.strip().lower()
        if not part:
            continue
        if not part.startswith("."):
            part = f".{part}"
        exts.add(part)
    return exts


def collect_files(folder: Path, extensions: set[str]) -> list[FileMeta]:
    if not folder.exists() or not folder.is_dir():
        raise FileNotFoundError(f"目录不存在：{folder}")

    files: list[FileMeta] = []
    for path in folder.iterdir():
        if not path.is_file():
            continue
        if extensions and path.suffix.lower() not in extensions:
            continue
        stat = path.stat()
        normalized = normalize_title(path.stem)
        if not normalized:
            continue
        files.append(
            FileMeta(
                name=path.name,
                path=str(path.resolve()),
                stem=path.stem,
                normalized=normalized,
                size_bytes=int(stat.st_size),
                modified_ts=stat.st_mtime,
            )
        )

    files.sort(key=lambda f: f.name.lower())
    return files


def build_file_ngrams(text: str, lengths: list[int], stopwords: list[str]) -> set[str]:
    tokens: set[str] = set()
    cleaned = remove_stopwords(text, stopwords)
    if not cleaned:
        return tokens

    segments = [seg for seg in cleaned.split(" ") if seg]
    for seg in segments:
        for n in lengths:
            if len(seg) < n:
                continue
            for i in range(len(seg) - n + 1):
                token = seg[i : i + n]
                if token.isdigit():
                    continue
                tokens.add(token)
    return tokens


def should_link_pair(
    len_counter: Counter[int],
    lengths: list[int],
    min_pair_matches: int,
    short_title_pair: bool,
    min_title_len: int,
) -> bool:
    selected = set(lengths)
    count2 = len_counter.get(2, 0) if 2 in selected else 0
    count3 = len_counter.get(3, 0) if 3 in selected else 0
    count4 = len_counter.get(4, 0) if 4 in selected else 0
    count5plus = sum(c for n, c in len_counter.items() if n in selected and n >= 5)
    max_shared_len = max((n for n, c in len_counter.items() if c > 0), default=0)

    # 高置信：命中 >=5 字核心片段
    if count5plus >= 1:
        return True

    # 中置信：4 字片段至少 2 个，或 3 字片段至少 3 个
    if 4 in selected and count4 >= 2:
        return True
    if 3 in selected and count3 >= 3:
        return True

    # 仅 2 字模式：按用户阈值放行（用于粗筛预览）
    if selected == {2}:
        return count2 >= min_pair_matches

    # 短标题兜底：短标题常见“标题+版本标签”，允许更宽松
    if short_title_pair:
        if min_title_len <= 8 and max_shared_len >= min_title_len:
            return True
        if count2 >= min_pair_matches and (count3 >= 2 or count4 >= 1):
            return True

    return False


def fmt_time(ts: float) -> str:
    return datetime.fromtimestamp(ts).strftime("%Y-%m-%d %H:%M:%S")


def fmt_size(size_bytes: int) -> str:
    units = ["B", "KB", "MB", "GB", "TB"]
    value = float(size_bytes)
    unit_idx = 0
    while value >= 1024 and unit_idx < len(units) - 1:
        value /= 1024.0
        unit_idx += 1
    if unit_idx == 0:
        return f"{int(value)} {units[unit_idx]}"
    return f"{value:.2f} {units[unit_idx]}"


def analyze_folder(
    folder_path: str,
    extensions_raw: str,
    stopwords_raw: str,
    lengths: list[int],
    min_pair_matches: int,
    max_df_abs: int,
    max_df_ratio: float,
) -> dict[str, Any]:
    if not lengths:
        raise ValueError("必须至少选择一种片段长度")

    lengths = sorted({int(x) for x in lengths if int(x) >= 2})
    if not lengths:
        raise ValueError("片段长度无效")

    min_pair_matches = max(1, int(min_pair_matches))
    max_df_abs = max(2, int(max_df_abs))
    max_df_ratio = min(max(float(max_df_ratio), 0.0), 1.0)

    folder = Path(folder_path).resolve()
    extensions = split_extensions(extensions_raw)
    stopwords = parse_stopwords(stopwords_raw)
    files = collect_files(folder, extensions)
    total = len(files)
    if total == 0:
        raise ValueError("未找到符合后缀条件的文件")

    per_file_cleaned: list[str] = [
        remove_stopwords(f.normalized, stopwords).replace(" ", "") for f in files
    ]
    per_file_tokens: list[set[str]] = [build_file_ngrams(f.normalized, lengths, stopwords) for f in files]

    token_docs: dict[str, list[int]] = defaultdict(list)
    for file_idx, tokens in enumerate(per_file_tokens):
        for token in tokens:
            token_docs[token].append(file_idx)

    if max_df_ratio > 0:
        ratio_limit = max(3, math.ceil(total * max_df_ratio))
        max_allowed = min(max_df_abs, ratio_limit)
    else:
        max_allowed = max_df_abs
    filtered_token_docs: dict[str, list[int]] = {}
    for token, idxs in token_docs.items():
        if len(idxs) < 2:
            continue
        if len(idxs) > max_allowed:
            continue
        filtered_token_docs[token] = idxs

    pair_counts: dict[tuple[int, int], int] = defaultdict(int)
    pair_len_counts: dict[tuple[int, int], Counter[int]] = defaultdict(Counter)
    pair_tokens: dict[tuple[int, int], set[str]] = defaultdict(set)

    for token, idxs in filtered_token_docs.items():
        if len(idxs) < 2:
            continue
        for a, b in combinations(sorted(idxs), 2):
            key = (a, b)
            pair_counts[key] += 1
            pair_len_counts[key][len(token)] += 1
            if len(pair_tokens[key]) < 12:
                pair_tokens[key].add(token)

    uf = UnionFind(total)
    for a, b in pair_counts:
        len_counter = pair_len_counts[(a, b)]
        short_title_pair = min(len(per_file_cleaned[a]), len(per_file_cleaned[b])) <= 12
        min_title_len = min(len(per_file_cleaned[a]), len(per_file_cleaned[b]))
        if should_link_pair(
            len_counter=len_counter,
            lengths=lengths,
            min_pair_matches=min_pair_matches,
            short_title_pair=short_title_pair,
            min_title_len=min_title_len,
        ):
            uf.union(a, b)

    comp: dict[int, list[int]] = defaultdict(list)
    for idx in range(total):
        root = uf.find(idx)
        comp[root].append(idx)

    groups_idx = [g for g in comp.values() if len(g) >= 2]
    groups_idx.sort(key=lambda g: (-len(g), -max(files[i].modified_ts for i in g)))

    groups: list[dict[str, Any]] = []
    for g in groups_idx:
        file_entries = sorted(
            (files[i] for i in g),
            key=lambda x: (-x.size_bytes, x.name.lower()),
        )
        latest_size = max((f.size_bytes for f in file_entries), default=0)

        local_counter: Counter[str] = Counter()
        for i in g:
            local_counter.update(per_file_tokens[i])

        shared_candidates = [
            token
            for token, c in local_counter.items()
            if c >= 2 and len(token) in lengths
        ]
        shared_candidates.sort(key=lambda t: (-len(t), -local_counter[t], t))
        top_shared = shared_candidates[:8]

        length_stats = []
        for n in lengths:
            count_n = sum(1 for t in shared_candidates if len(t) == n)
            length_stats.append(f"{n}字:{count_n}")

        representative = file_entries[0].stem

        groups.append(
            {
                "size": len(file_entries),
                "representative": representative,
                "shared_snippets": top_shared,
                "length_stats_text": " / ".join(length_stats),
                "latest_size_bytes": latest_size,
                "latest_size_text": fmt_size(latest_size),
                "old_file_count": sum(1 for f in file_entries if f.size_bytes < latest_size),
                "files": [
                    {
                        "name": f.name,
                        "path": f.path,
                        "modified": fmt_time(f.modified_ts),
                        "size_bytes": f.size_bytes,
                        "size_text": fmt_size(f.size_bytes),
                        "is_latest_by_size": f.size_bytes >= latest_size,
                    }
                    for f in file_entries
                ],
            }
        )

    duplicate_file_count = sum(len(g["files"]) for g in groups)
    return {
        "folder": str(folder),
        "total_files": total,
        "group_count": len(groups),
        "duplicate_file_count": duplicate_file_count,
        "params": {
            "extensions": sorted(extensions),
            "stopwords": stopwords,
            "lengths": lengths,
            "min_pair_matches": min_pair_matches,
            "max_df_abs": max_df_abs,
            "max_df_ratio": max_df_ratio,
        },
        "groups": groups,
    }


def export_csv(data: dict[str, Any], output_path: Path) -> None:
    with output_path.open("w", encoding="utf-8-sig", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(
            ["group_id", "is_latest_by_size", "file_name", "modified", "size", "path", "shared_snippets"]
        )
        for gid, group in enumerate(data.get("groups", []), start=1):
            snippets = "|".join(group.get("shared_snippets", []))
            for file in group.get("files", []):
                writer.writerow(
                    [
                        gid,
                        "yes" if file.get("is_latest_by_size") else "no",
                        file.get("name", ""),
                        file.get("modified", ""),
                        file.get("size_text", ""),
                        file.get("path", ""),
                        snippets,
                    ]
                )


def delete_files(paths: list[str], root_folder: Path) -> dict[str, Any]:
    deleted: list[str] = []
    failed: list[dict[str, str]] = []

    root = root_folder.resolve()
    for raw in paths:
        p = Path(str(raw)).resolve()
        if not p.exists():
            failed.append({"path": str(p), "reason": "文件不存在"})
            continue
        if not p.is_file():
            failed.append({"path": str(p), "reason": "不是文件"})
            continue
        if root not in p.parents and p != root:
            failed.append({"path": str(p), "reason": "路径超出扫描目录"})
            continue
        try:
            p.unlink()
            deleted.append(str(p))
        except Exception as exc:  # noqa: BLE001
            failed.append({"path": str(p), "reason": str(exc)})

    return {
        "deleted_count": len(deleted),
        "deleted": deleted,
        "failed_count": len(failed),
        "failed": failed,
    }


class Handler(BaseHTTPRequestHandler):
    default_path = str(Path.cwd())
    persisted_default_path = str(Path.cwd())
    config_path = Path.cwd() / CONFIG_FILENAME
    persisted_stopwords = ",".join(DEFAULT_STOPWORDS)
    latest_result: dict[str, Any] | None = None
    lock = threading.Lock()

    def _send_json(self, payload: dict[str, Any], status: int = 200) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_html(self, html: str, status: int = 200) -> None:
        body = html.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path == "/":
            html = HTML_PAGE.replace(
                "__DEFAULT_PATH__",
                type(self).persisted_default_path.replace("\\", "/"),
            )
            html = html.replace(
                "__DEFAULT_STOPWORDS_JSON__",
                json.dumps(type(self).persisted_stopwords, ensure_ascii=False),
            )
            self._send_html(html)
            return

        if parsed.path == "/api/export":
            with self.lock:
                data = self.latest_result

            if not data:
                self._send_json({"error": "当前没有可导出的结果"}, status=400)
                return

            qs = parse_qs(parsed.query)
            output = qs.get("output", [""])[0].strip()
            if not output:
                output = str(Path.cwd() / "novel_groups.csv")

            out_path = Path(output).resolve()
            export_csv(data, out_path)
            self._send_json({"ok": True, "output": str(out_path)})
            return

        self._send_json({"error": "Not Found"}, status=404)

    def do_POST(self) -> None:  # noqa: N802
        try:
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length)
            payload = json.loads(body.decode("utf-8"))

            if self.path == "/api/stopwords":
                raw = str(payload.get("stopwords", "")).strip()
                normalized = save_stopwords(type(self).config_path, raw)
                with self.lock:
                    type(self).persisted_stopwords = normalized
                self._send_json({"ok": True, "stopwords": normalized})
                return

            if self.path == "/api/default-path":
                raw = str(payload.get("default_path", "")).strip()
                if not raw:
                    self._send_json({"error": "default_path 不能为空"}, status=400)
                    return
                normalized = save_default_path(type(self).config_path, raw)
                with self.lock:
                    type(self).persisted_default_path = normalized
                    type(self).default_path = normalized
                self._send_json({"ok": True, "default_path": normalized})
                return

            if self.path == "/api/delete-files":
                raw_paths = payload.get("paths", [])
                if not isinstance(raw_paths, list) or not raw_paths:
                    self._send_json({"error": "paths 不能为空"}, status=400)
                    return
                folder_raw = str(payload.get("folder_path", "")).strip()
                folder = Path(folder_raw).resolve() if folder_raw else Path(type(self).default_path).resolve()
                result = delete_files([str(p) for p in raw_paths], folder)
                self._send_json({"ok": True, **result})
                return

            if self.path != "/api/analyze":
                self._send_json({"error": "Not Found"}, status=404)
                return

            stopwords_raw = str(payload.get("stopwords", type(self).persisted_stopwords))
            folder_path_raw = str(payload.get("folder_path", "")).strip() or type(self).persisted_default_path
            result = analyze_folder(
                folder_path=folder_path_raw,
                extensions_raw=str(payload.get("extensions", ".txt,.doc,.docx,.epub")),
                stopwords_raw=stopwords_raw,
                lengths=[int(x) for x in payload.get("lengths", [2, 3, 4, 5, 6])],
                min_pair_matches=int(payload.get("min_pair_matches", 2)),
                max_df_abs=int(payload.get("max_df_abs", 120)),
                max_df_ratio=float(payload.get("max_df_ratio", 0.04)),
            )
            normalized = ",".join(result["params"]["stopwords"])
            try:
                folder_saved = save_default_path(type(self).config_path, result["folder"])
                save_stopwords(type(self).config_path, normalized)
            except Exception:  # noqa: BLE001
                folder_saved = result["folder"]
                pass
            with self.lock:
                self.latest_result = result
                type(self).persisted_stopwords = normalized
                type(self).persisted_default_path = folder_saved
                type(self).default_path = folder_saved
            self._send_json(result)
        except Exception as exc:  # noqa: BLE001
            self._send_json({"error": str(exc)}, status=400)

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A003
        return


def main() -> int:
    parser = argparse.ArgumentParser(description="小说文件名同名筛选 WebUI")
    parser.add_argument("--host", default="127.0.0.1", help="监听地址，默认 127.0.0.1")
    parser.add_argument("--port", type=int, default=18080, help="监听端口，默认 18080")
    parser.add_argument(
        "--default-path",
        default=DEFAULT_SCAN_DIR,
        help="WebUI 默认扫描目录",
    )
    parser.add_argument(
        "--export-json",
        default="",
        help="可选：命令行模式输出分析 JSON 文件路径（提供后不启动 Web）",
    )
    parser.add_argument(
        "--folder",
        default="",
        help="命令行模式：待分析目录（与 --export-json 搭配使用）",
    )
    parser.add_argument(
        "--extensions",
        default=".txt,.doc,.docx,.epub",
        help="命令行模式：后缀列表",
    )
    parser.add_argument(
        "--lengths",
        default="2,3,4,5,6",
        help="命令行模式：片段长度，逗号分隔",
    )
    parser.add_argument(
        "--stopwords",
        default=",".join(DEFAULT_STOPWORDS),
        help="命令行模式：停用词，逗号分隔",
    )
    parser.add_argument("--min-pair-matches", type=int, default=2)
    parser.add_argument("--max-df-abs", type=int, default=120)
    parser.add_argument("--max-df-ratio", type=float, default=0.04)
    parser.add_argument(
        "--open-browser",
        action="store_true",
        help="服务启动后自动打开浏览器",
    )
    args = parser.parse_args()

    if args.export_json:
        folder = args.folder or args.default_path
        lengths = [int(x.strip()) for x in str(args.lengths).split(",") if x.strip()]
        result = analyze_folder(
            folder_path=folder,
            extensions_raw=args.extensions,
            stopwords_raw=args.stopwords,
            lengths=lengths,
            min_pair_matches=args.min_pair_matches,
            max_df_abs=args.max_df_abs,
            max_df_ratio=args.max_df_ratio,
        )

        output = Path(args.export_json).resolve()
        output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"已输出分析结果：{output}")
        return 0

    config_path = Path(__file__).resolve().parent / CONFIG_FILENAME
    fallback_stopwords = ",".join(parse_stopwords(args.stopwords))
    persisted_stopwords = load_saved_stopwords(config_path, fallback_stopwords)
    persisted_default_path = load_saved_default_path(config_path, args.default_path)

    Handler.default_path = str(Path(persisted_default_path).resolve())
    Handler.persisted_default_path = str(Path(persisted_default_path).resolve())
    Handler.config_path = config_path
    Handler.persisted_stopwords = persisted_stopwords
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"小说同名筛选 WebUI 已启动：http://{args.host}:{args.port}")
    print("按 Ctrl+C 退出")
    if args.open_browser:
        threading.Timer(0.8, lambda: webbrowser.open(f"http://{args.host}:{args.port}")).start()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
