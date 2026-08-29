// dgmon dashboard — app logic (Chart.js)
// All data comes from the Prometheus-compatible API (/api/v1/) using PromQL.
"use strict";

const REFRESH_MS = 10000;
const PALETTE = ["#2563eb", "#16a34a", "#d97706", "#dc2626", "#7c3aed", "#0891b2", "#ca8a04", "#be185d"];

const state = { nodes: [], selected: null, charts: {}, timer: null, view: "cluster" };

/* -- DOM update cache: skip innerHTML when content is unchanged (prevents flicker) -- */
const _lastHTML = {};
function updateHTML(el, html, key) {
  if (_lastHTML[key] === html) return;
  _lastHTML[key] = html;
  el.innerHTML = html;
}
function clearHTML(key) { delete _lastHTML[key]; }

/* -- theme -- */
function getThemeColors() {
  const cs = getComputedStyle(document.documentElement);
  return {
    text: cs.getPropertyValue("--chart-text").trim() || "#8a94a6",
    grid: cs.getPropertyValue("--chart-grid").trim() || "rgba(138,148,166,0.18)",
    tooltipBg: cs.getPropertyValue("--tooltip-bg").trim() || "rgba(255,255,255,0.95)",
    tooltipTitle: cs.getPropertyValue("--tooltip-title").trim() || "#1a2233",
    tooltipBody: cs.getPropertyValue("--tooltip-body").trim() || "#4b5563",
    tooltipBorder: cs.getPropertyValue("--tooltip-border").trim() || "#e3e8ef",
  };
}

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme);
  localStorage.setItem("dgmon-theme", theme);
  // Destroy all existing Chart.js instances before rebuilding.
  for (const id in state.charts) {
    if (state.charts[id] && typeof state.charts[id].destroy === "function") {
      state.charts[id].destroy();
    }
  }
  state.charts = {};
  // Clear DOM cache so cards rebuild with new theme.
  for (const k in _lastHTML) delete _lastHTML[k];
  refreshAll();
}

function setupThemeToggle() {
  const saved = localStorage.getItem("dgmon-theme");
  if (saved) document.documentElement.setAttribute("data-theme", saved);
  document.getElementById("theme-toggle").addEventListener("click", () => {
    const current = document.documentElement.getAttribute("data-theme");
    applyTheme(current === "dark" ? "light" : "dark");
  });
}

const fetchJSON = async (url, opts) => {
  const r = await fetch(url, opts);
  if (!r.ok) throw new Error(`${url} -> ${r.status}`);
  return r.json();
};

/* -- boot -- */
async function init() {
  setupThemeToggle();
  setupTabs();
  setupControlModal();
  setInterval(refreshNodes, 15000);
  await probeControlEnabled();
  await refreshAll();
  state.timer = setInterval(refreshAll, REFRESH_MS);
}

/* -- tabs -- */
function setupTabs() {
  document.querySelectorAll(".tab").forEach((btn) =>
    btn.addEventListener("click", () => {
      state.view = btn.dataset.view;
      document.querySelectorAll(".tab").forEach((b) => b.classList.toggle("active", b === btn));
      document.getElementById("view-cluster").hidden = state.view !== "cluster";
      document.getElementById("view-host").hidden = state.view !== "host";
      document.getElementById("view-control").hidden = state.view !== "control";
      if (state.view === "control") refreshControl();
    })
  );
}

/* -- nodes -- */
async function refreshNodes() {
  try {
    const resp = await fetchJSON("/api/v1/label/hostname/values");
    state.nodes = resp.data || [];
    renderNodeBar();
    document.getElementById("node-count").textContent = `${state.nodes.length} node${state.nodes.length === 1 ? "" : "s"}`;
    if (!state.selected && state.nodes.length) state.selected = state.nodes[0];
    setStatus("online");
  } catch { setStatus("offline"); }
}

function renderNodeBar() {
  const bar = document.getElementById("node-bar");
  bar.innerHTML = state.nodes.map((n) =>
    `<button class="${n === state.selected ? "active" : ""}" data-host="${n}">${n}</button>`
  ).join("");
  bar.querySelectorAll("button").forEach((b) =>
    b.addEventListener("click", () => {
      state.selected = b.dataset.host;
      renderNodeBar();
      refreshAll();
    })
  );
  document.getElementById("sel-host").textContent = state.selected || "\u2014";
}

/* -- data refresh -- */
async function refreshAll() {
  try {
    const now = Date.now();
    const start = now - 3600e3;
    const step = 30000;
    const host = state.selected || "";
    const hostSel = host ? `{hostname="${host}"}` : "";

    // Build one batch request with all instant and range queries.
    const queries = [
      // ── Cluster overview (instant) ──
      { id: "cl_gpu_util", expr: `avg(dgmon_gpu_utilization)` },
      { id: "cl_gpu_mem_used", expr: `sum(dgmon_gpu_memory_used_mb)` },
      { id: "cl_gpu_mem_total", expr: `sum(dgmon_gpu_memory_total_mb)` },
      { id: "cl_gpu_count", expr: `count(dgmon_gpu_utilization)` },
      { id: "cl_cpu", expr: `avg(dgmon_cpu_usage_pct)` },
      { id: "cl_mem_used", expr: `sum(dgmon_memory_used_mb)` },
      { id: "cl_mem_total", expr: `sum(dgmon_memory_total_mb)` },
      { id: "cl_net_rx", expr: `sum(dgmon_network_rx_bytes)` },
      { id: "cl_net_tx", expr: `sum(dgmon_network_tx_bytes)` },
      // ── Inference overview (instant) ──
      { id: "inf_running", expr: `sum(dgmon_inference_num_requests_running)` },
      { id: "inf_waiting", expr: `sum(dgmon_inference_num_requests_waiting)` },
      { id: "inf_kv_cache", expr: `avg(dgmon_inference_kv_cache_usage_perc) * 100` },
      { id: "inf_tok_sec", expr: `sum(rate(dgmon_inference_generation_tokens_total[1m]))` },
      { id: "inf_in_tok_sec", expr: `sum(rate(dgmon_inference_prompt_tokens_total[1m]))` },
      { id: "inf_ttft_p50", expr: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))` },
      { id: "inf_ttft_p95", expr: `histogram_quantile(0.95, sum by (le) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))` },
      { id: "inf_itl_p50", expr: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_inter_token_latency_seconds_bucket[5m])))` },
      { id: "inf_success", expr: `sum(dgmon_inference_request_success_total)` },
      // ── Per-node table (instant) ──
      { id: "node_gpu_util", expr: `avg by (hostname) (dgmon_gpu_utilization)` },
      { id: "node_gpu_mem_used", expr: `sum by (hostname) (dgmon_gpu_memory_used_mb)` },
      { id: "node_gpu_mem_total", expr: `sum by (hostname) (dgmon_gpu_memory_total_mb)` },
      { id: "node_gpu_count", expr: `count by (hostname) (dgmon_gpu_utilization)` },
      { id: "node_tok_sec", expr: `sum by (hostname) (rate(dgmon_inference_generation_tokens_total[1m]))` },
      { id: "node_in_tok_sec", expr: `sum by (hostname) (rate(dgmon_inference_prompt_tokens_total[1m]))` },
      { id: "node_req_run", expr: `sum by (hostname) (dgmon_inference_num_requests_running)` },
      { id: "node_ttft", expr: `histogram_quantile(0.5, sum by (le, hostname) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))` },
      // ── Host cards (instant) ──
      { id: "cpu", expr: `dgmon_cpu_usage_pct${hostSel}` },
      { id: "mem_used", expr: `dgmon_memory_used_mb${hostSel}` },
      { id: "mem_total", expr: `dgmon_memory_total_mb${hostSel}` },
      { id: "disk_used", expr: `dgmon_disk_used_gb${hostSel}` },
      { id: "disk_total", expr: `dgmon_disk_total_gb${hostSel}` },
      { id: "net_rx", expr: `dgmon_network_rx_bytes${hostSel}` },
      { id: "net_tx", expr: `dgmon_network_tx_bytes${hostSel}` },
      { id: "uptime", expr: `dgmon_uptime_seconds${hostSel}` },
      // GPU table (instant).
      { id: "gpu_util", expr: `dgmon_gpu_utilization${hostSel}` },
      { id: "gpu_mem_util", expr: `dgmon_gpu_mem_utilization${hostSel}` },
      { id: "gpu_mem_used", expr: `dgmon_gpu_memory_used_mb${hostSel}` },
      { id: "gpu_mem_total", expr: `dgmon_gpu_memory_total_mb${hostSel}` },
      { id: "gpu_temp", expr: `dgmon_gpu_temp_c${hostSel}` },
      { id: "gpu_power", expr: `dgmon_gpu_power_w${hostSel}` },
      { id: "gpu_fan", expr: `dgmon_gpu_fan_speed_pct${hostSel}` },
      { id: "gpu_sm_clock", expr: `dgmon_gpu_sm_clock_mhz${hostSel}` },
      { id: "gpu_mem_clock", expr: `dgmon_gpu_mem_clock_mhz${hostSel}` },
      // ── Cluster charts (range) ──
      { id: "chart-cgpu", expr: `avg(dgmon_gpu_utilization)`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-cgpu-mem", expr: `sum(dgmon_gpu_memory_used_mb)`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-tok", expr: `sum(rate(dgmon_inference_generation_tokens_total[1m]))`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-in-tok", expr: `sum(rate(dgmon_inference_prompt_tokens_total[1m]))`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-ttft", expr: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-itl", expr: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_inter_token_latency_seconds_bucket[5m])))`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      // ── Host charts (range) ──
      { id: "chart-cpu", expr: `dgmon_cpu_usage_pct${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-mem", expr: `dgmon_memory_used_mb${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-gpu", expr: `dgmon_gpu_utilization${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-temp", expr: `dgmon_gpu_temp_c${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-gpu-mem", expr: `dgmon_gpu_memory_used_mb${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-gpu-power", expr: `dgmon_gpu_power_w${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-sm-clock", expr: `dgmon_gpu_sm_clock_mhz${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
      { id: "chart-mem-clock", expr: `dgmon_gpu_mem_clock_mhz${hostSel}`, range: { start: start / 1000, end: now / 1000, step: step / 1000 } },
    ];

    const resp = await fetchJSON("/api/v1/query_batch", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ queries }),
    });

    const data = resp.data || {};
    renderClusterCards(data);
    renderInferenceCards(data);
    renderClusterTable(data);
    renderHostCards(data);
    renderGpuTable(data);
    updateCharts(data, now);
    document.getElementById("refresh-time").textContent = new Date().toLocaleTimeString();
  } catch (e) { console.warn("refreshAll:", e); }
}

/* -- helpers for Prometheus vector results -- */
function vectorResults(data, id) {
  const q = data[id];
  if (!q || q.resultType !== "vector") return [];
  return q.result || [];
}

function firstValue(data, id) {
  const r = vectorResults(data, id);
  if (!r.length) return null;
  const v = r[0].value;
  return v ? parseFloat(v[1]) : null;
}

/* ── cluster overview cards ── */
function renderClusterCards(data) {
  const el = document.getElementById("cluster-cards");
  const gpuUtil = firstValue(data, "cl_gpu_util");
  const gpuMemUsed = firstValue(data, "cl_gpu_mem_used");
  const gpuMemTotal = firstValue(data, "cl_gpu_mem_total");
  const gpuCount = firstValue(data, "cl_gpu_count");
  const cpu = firstValue(data, "cl_cpu");
  const memUsed = firstValue(data, "cl_mem_used");
  const memTotal = firstValue(data, "cl_mem_total");
  const netRx = firstValue(data, "cl_net_rx");
  const netTx = firstValue(data, "cl_net_tx");

  if (gpuUtil == null) { updateHTML(el, '<div class="empty-note">No data yet…</div>', 'cluster-cards'); return; }

  const memPct = gpuMemTotal ? Math.round((gpuMemUsed / gpuMemTotal) * 100) : 0;
  const sysMemPct = memTotal ? Math.round((memUsed / memTotal) * 100) : 0;
  updateHTML(el, `
    <div class="stat">
      <div class="k">GPUs</div>
      <div class="v">${fmt(gpuCount,0)}</div>
    </div>
    <div class="stat">
      <div class="k">GPU Util</div>
      <div class="v">${fmt(gpuUtil,1)}% <small>avg</small></div>
      <div class="bar${gpuUtil>80?gpuUtil>95?' crit':' warn':''}"><i style="width:${gpuUtil}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">GPU Memory</div>
      <div class="v">${fmt(gpuMemUsed/1024,1)} <small>/ ${fmt(gpuMemTotal/1024,0)} GiB</small></div>
      <div class="bar${memPct>80?memPct>95?' crit':' warn':''}"><i style="width:${memPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">CPU</div>
      <div class="v">${fmt(cpu,1)}% <small>avg</small></div>
      <div class="bar${cpu>80?cpu>95?' crit':' warn':''}"><i style="width:${cpu}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">System Memory</div>
      <div class="v">${fmt(memUsed/1024,1)} <small>/ ${fmt(memTotal/1024,0)} GiB</small></div>
      <div class="bar${sysMemPct>80?sysMemPct>95?' crit':' warn':''}"><i style="width:${sysMemPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Network</div>
      <div class="v">${fmt(netRx/1e6,1)} <small>↓ / ${fmt(netTx/1e6,1)} ↑ MB/s</small></div>
    </div>`, 'cluster-cards');
}

/* ── inference overview cards ── */
function renderInferenceCards(data) {
  const el = document.getElementById("inference-cards");
  const running = firstValue(data, "inf_running");
  const waiting = firstValue(data, "inf_waiting");
  const kvCache = firstValue(data, "inf_kv_cache");
  const tokSec = firstValue(data, "inf_tok_sec");
  const inTokSec = firstValue(data, "inf_in_tok_sec");
  const ttftP50 = firstValue(data, "inf_ttft_p50");
  const ttftP95 = firstValue(data, "inf_ttft_p95");
  const itlP50 = firstValue(data, "inf_itl_p50");
  const success = firstValue(data, "inf_success");

  if (running == null && tokSec == null) { updateHTML(el, '<div class="empty-note">No inference data yet…</div>', 'inference-cards'); return; }

  updateHTML(el, `
    <div class="stat">
      <div class="k">Requests Running</div>
      <div class="v">${fmt(running,0)}</div>
    </div>
    <div class="stat">
      <div class="k">Requests Waiting</div>
      <div class="v">${fmt(waiting,0)}</div>
    </div>
    <div class="stat">
      <div class="k">Output Tokens/s</div>
      <div class="v">${fmt(tokSec,1)} <small>tok/s</small></div>
    </div>
    <div class="stat">
      <div class="k">Input Tokens/s</div>
      <div class="v">${fmt(inTokSec,1)} <small>tok/s</small></div>
    </div>
    <div class="stat">
      <div class="k">TTFT p50</div>
      <div class="v">${fmt(ttftP50,3)}s</div>
    </div>
    <div class="stat">
      <div class="k">TTFT p95</div>
      <div class="v">${fmt(ttftP95,3)}s</div>
    </div>
    <div class="stat">
      <div class="k">Inter-token Latency p50</div>
      <div class="v">${fmt(itlP50,3)}s</div>
    </div>
    <div class="stat">
      <div class="k">KV Cache</div>
      <div class="v">${fmt(kvCache,1)}%</div>
      <div class="bar${kvCache>80?kvCache>95?' crit':' warn':''}"><i style="width:${kvCache}%"></i></div>
    </div>`, 'inference-cards');
}

/* ── cluster per-node table ── */
function renderClusterTable(data) {
  const body = document.getElementById("cluster-table-body");
  const empty = document.getElementById("cluster-empty");
  const gpuUtil = vectorResults(data, "node_gpu_util");
  const gpuMemUsed = vectorResults(data, "node_gpu_mem_used");
  const gpuMemTotal = vectorResults(data, "node_gpu_mem_total");
  const gpuCount = vectorResults(data, "node_gpu_count");
  const tokSec = vectorResults(data, "node_tok_sec");
  const inTokSec = vectorResults(data, "node_in_tok_sec");
  const reqRun = vectorResults(data, "node_req_run");
  const ttft = vectorResults(data, "node_ttft");

  document.getElementById("cluster-node-count").textContent = gpuUtil.length ? `${gpuUtil.length} node${gpuUtil.length!==1?'s':''}` : "—";
  if (!gpuUtil.length) { updateHTML(body, "", 'cluster-table'); empty.style.display = "block"; return; }
  empty.style.display = "none";

  // Index by hostname.
  const byHost = (arr) => {
    const m = {};
    for (const r of arr) {
      const h = r.metric && r.metric.hostname;
      if (h != null) m[h] = r;
    }
    return m;
  };
  const utilM = byHost(gpuUtil), memUsedM = byHost(gpuMemUsed), memTotalM = byHost(gpuMemTotal);
  const countM = byHost(gpuCount), tokM = byHost(tokSec), inTokM = byHost(inTokSec), reqM = byHost(reqRun), ttftM = byHost(ttft);

  const hosts = [...new Set([...Object.keys(utilM), ...Object.keys(memUsedM), ...Object.keys(memTotalM), ...Object.keys(countM), ...Object.keys(tokM), ...Object.keys(inTokM), ...Object.keys(reqM), ...Object.keys(ttftM)])].sort();

  updateHTML(body, hosts.map((h) => {
    const u = utilM[h] ? parseFloat(utilM[h].value[1]) : null;
    const mu = memUsedM[h] ? parseFloat(memUsedM[h].value[1]) : null;
    const mt = memTotalM[h] ? parseFloat(memTotalM[h].value[1]) : null;
    const gc = countM[h] ? parseFloat(countM[h].value[1]) : null;
    const ts = tokM[h] ? parseFloat(tokM[h].value[1]) : null;
    const its = inTokM[h] ? parseFloat(inTokM[h].value[1]) : null;
    const rr = reqM[h] ? parseFloat(reqM[h].value[1]) : null;
    const tt = ttftM[h] ? parseFloat(ttftM[h].value[1]) : null;
    const memStr = mu != null ? (mt != null ? `${fmt(mu/1024,1)}/${fmt(mt/1024,0)}G` : `${fmt(mu/1024,1)}G`) : "—";
    return `<tr>
      <td class="idx">${esc(h)}</td>
      <td class="num">${gc != null ? fmt(gc,0) : "—"}</td>
      <td class="num">${u != null ? fmt(u,0)+"%" : "—"}</td>
      <td class="num">${memStr}</td>
      <td class="num">${ts != null ? fmt(ts,1) : "—"}</td>
      <td class="num">${its != null ? fmt(its,1) : "—"}</td>
      <td class="num">${rr != null ? fmt(rr,0) : "—"}</td>
      <td class="num">${tt != null ? fmt(tt,3)+"s" : "—"}</td>
    </tr>`;
  }).join(""), 'cluster-table');
}

/* -- host cards -- */
function renderHostCards(data) {
  const host = document.getElementById("host-cards");
  const cpuPct = firstValue(data, "cpu");
  const memUsed = firstValue(data, "mem_used");
  const memTotal = firstValue(data, "mem_total");
  const diskUsed = firstValue(data, "disk_used");
  const diskTotal = firstValue(data, "disk_total");
  const netRx = firstValue(data, "net_rx");
  const netTx = firstValue(data, "net_tx");
  const uptime = firstValue(data, "uptime");

  if (cpuPct == null) { updateHTML(host, '<div class="empty-note">No data yet…</div>', 'host-cards'); return; }

  const memPct = memTotal ? Math.round((memUsed / memTotal) * 100) : 0;
  const diskPct = diskTotal ? Math.round((diskUsed / diskTotal) * 100) : 0;
  updateHTML(host, `
    <div class="stat">
      <div class="k">CPU</div>
      <div class="v">${fmt(cpuPct,1)}% <small>used</small></div>
      <div class="bar${cpuPct>80?cpuPct>95?' crit':' warn':''}"><i style="width:${cpuPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Memory</div>
      <div class="v">${fmt(memUsed/1024,1)} <small>/ ${fmt(memTotal/1024,0)} GiB</small></div>
      <div class="bar${memPct>80?memPct>95?' crit':' warn':''}"><i style="width:${memPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Disk</div>
      <div class="v">${fmt(diskUsed,0)} <small>/ ${fmt(diskTotal,0)} GiB</small></div>
      <div class="bar${diskPct>80?diskPct>95?' crit':' warn':''}"><i style="width:${diskPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Network</div>
      <div class="v">${fmt(netRx/1e6,1)} <small>↓ / ${fmt(netTx/1e6,1)} ↑ MB/s</small></div>
    </div>
    <div class="stat">
      <div class="k">Uptime</div>
      <div class="v">${fmtUptime(uptime)}</div>
    </div>`, 'host-cards');
}

/* -- gpu table -- */
function renderGpuTable(data) {
  const body = document.getElementById("gpu-table-body");
  const empty = document.getElementById("gpu-empty");
  const util = vectorResults(data, "gpu_util");
  const memUtil = vectorResults(data, "gpu_mem_util");
  const memUsed = vectorResults(data, "gpu_mem_used");
  const memTotal = vectorResults(data, "gpu_mem_total");
  const temp = vectorResults(data, "gpu_temp");
  const power = vectorResults(data, "gpu_power");
  const fan = vectorResults(data, "gpu_fan");
  const smClock = vectorResults(data, "gpu_sm_clock");
  const memClock = vectorResults(data, "gpu_mem_clock");

  document.getElementById("gpu-count").textContent = util.length ? `${util.length} GPU${util.length!==1?'s':''}` : "—";
  if (!util.length) { updateHTML(body, "", 'gpu-table'); empty.style.display = "block"; return; }
  empty.style.display = "none";

  // Index results by gpu label for easy lookup.
  const byGpu = (arr) => {
    const m = {};
    for (const r of arr) {
      const g = r.metric && r.metric.gpu;
      if (g != null) m[g] = r;
    }
    return m;
  };
  const utilM = byGpu(util), memUtilM = byGpu(memUtil), memUsedM = byGpu(memUsed), memTotalM = byGpu(memTotal);
  const tempM = byGpu(temp), powerM = byGpu(power), fanM = byGpu(fan), smClockM = byGpu(smClock), memClockM = byGpu(memClock);

  // Collect the union of gpu indices, sorted numerically.
  const idxSet = new Set([...Object.keys(utilM), ...Object.keys(memUtilM), ...Object.keys(memUsedM), ...Object.keys(memTotalM), ...Object.keys(tempM), ...Object.keys(powerM), ...Object.keys(fanM), ...Object.keys(smClockM), ...Object.keys(memClockM)]);
  const idxs = [...idxSet].sort((a, b) => Number(a) - Number(b));

  updateHTML(body, idxs.map((g) => {
    const u = utilM[g] ? parseFloat(utilM[g].value[1]) : 0;
    const barCls = u > 80 ? (u > 95 ? "crit" : "hot") : "";
    const metric = utilM[g]?.metric || {};
    const model = metric.model || "—";
    const uuid = metric.uuid || "—";
    const mu = memUtilM[g] ? parseFloat(memUtilM[g].value[1]) : null;
    const muUsed = memUsedM[g] ? parseFloat(memUsedM[g].value[1]) : null;
    const muTotal = memTotalM[g] ? parseFloat(memTotalM[g].value[1]) : null;
    const t = tempM[g] ? parseFloat(tempM[g].value[1]) : null;
    const p = powerM[g] ? parseFloat(powerM[g].value[1]) : null;
    const f = fanM[g] ? parseFloat(fanM[g].value[1]) : null;
    const sc = smClockM[g] ? parseFloat(smClockM[g].value[1]) : null;
    const mc = memClockM[g] ? parseFloat(memClockM[g].value[1]) : null;
    const memUsedStr = muUsed != null ? (muTotal != null ? `${fmt(muUsed/1024,1)}/${fmt(muTotal/1024,0)}G` : `${fmt(muUsed/1024,1)}G`) : "—";
    return `<tr>
      <td class="idx">${g}</td>
      <td>${esc(model)}</td>
      <td style="font-size:10px;color:var(--txt-3)">${esc(uuid.slice(0,20))}…</td>
      <td class="num"><span class="bar ${barCls}"><i style="width:${u}%"></i></span>${fmt(u,0)}%</td>
      <td class="num">${mu != null ? fmt(mu,0)+"%" : "—"}</td>
      <td class="num">${memUsedStr}</td>
      <td class="num">${t != null ? fmt(t,0)+"°" : "—"}</td>
      <td class="num">${p != null ? fmt(p,0) : "—"}</td>
      <td class="num">${f != null ? fmt(f,0)+"%" : "—"}</td>
      <td class="num">${sc != null ? fmt(sc,0)+" MHz" : "—"}</td>
      <td class="num">${mc != null ? fmt(mc,0)+" MHz" : "—"}</td>
    </tr>`;
  }).join(""), 'gpu-table');
}

/* -- charts (Chart.js) -- */
const CHART_DEFS = [
  // Host charts
  { id: "chart-cpu", qid: "chart-cpu", nowId: "now-cpu", label: "CPU %", unit: "%" },
  { id: "chart-mem", qid: "chart-mem", nowId: "now-mem", label: "Mem MB", unit: " MB" },
  { id: "chart-gpu", qid: "chart-gpu", nowId: "now-gpu", label: "GPU %", unit: "%" },
  { id: "chart-temp", qid: "chart-temp", nowId: "now-temp", label: "°C", unit: "°" },
  { id: "chart-gpu-mem", qid: "chart-gpu-mem", nowId: "now-gpu-mem", label: "GPU Mem GB", unit: " GB", div: 1024 },
  { id: "chart-gpu-power", qid: "chart-gpu-power", nowId: "now-gpu-power", label: "Power W", unit: " W" },
  { id: "chart-sm-clock", qid: "chart-sm-clock", nowId: "now-sm-clock", label: "SM MHz", unit: " MHz" },
  { id: "chart-mem-clock", qid: "chart-mem-clock", nowId: "now-mem-clock", label: "Mem MHz", unit: " MHz" },
  // Cluster charts
  { id: "chart-cgpu", qid: "chart-cgpu", nowId: "now-cgpu", label: "GPU %", unit: "%" },
  { id: "chart-cgpu-mem", qid: "chart-cgpu-mem", nowId: "now-cgpu-mem", label: "GPU Mem GB", unit: " GB", div: 1024 },
  { id: "chart-tok", qid: "chart-tok", nowId: "now-tok", label: "tok/s", unit: "" },
  { id: "chart-in-tok", qid: "chart-in-tok", nowId: "now-in-tok", label: "tok/s", unit: "" },
  { id: "chart-ttft", qid: "chart-ttft", nowId: "now-ttft", label: "s", unit: "s" },
  { id: "chart-itl", qid: "chart-itl", nowId: "now-itl", label: "s", unit: "s" },
];

function createOrUpdateChart(def, resp, now) {
  const el = document.getElementById(def.id);
  if (!el) return;
  const canvas = el.querySelector("canvas");
  if (!canvas) return;

  const seriesList = resp && resp.resultType === "matrix" ? resp.result : [];
  if (!seriesList || !seriesList.length) return;

  // Limit to first 4 GPU series
  const sel = seriesList.slice(0, 4);
  const maxPts = 121; // 1h / 30s

  // Build aligned time axis (milliseconds)
  const tEnd = Math.floor(now / 1000);
  const tStart = tEnd - maxPts * 30;
  const times = new Array(maxPts);
  for (let i = 0; i < maxPts; i++) times[i] = (tStart + i * 30) * 1000;

  // Format labels as HH:mm for display
  const labels = times.map(t => {
    const d = new Date(t);
    return `${String(d.getUTCHours()).padStart(2,"0")}:${String(d.getUTCMinutes()).padStart(2,"0")}`;
  });

  // Build datasets
  const datasets = sel.map((s, si) => {
    const map = new Map(s.values.map(([ts, v]) => [Math.floor(ts), v]));
    const div = def.div || 1;
    const tol = 15; // seconds tolerance for timestamp matching
    const data = times.map((t) => {
      const sec = t / 1000;
      for (let dt = -tol; dt <= tol; dt += 1) {
        if (map.has(sec + dt)) return parseFloat(map.get(sec + dt)) / div;
      }
      return null;
    });
    const lbl = s.metric ? Object.entries(s.metric).filter(([k]) => k !== "__name__" && k !== "hostname").map(([, v]) => v).join(" ") : "";
    return {
      label: lbl || (s.metric && s.metric.__name__) || "",
      data,
      borderColor: PALETTE[si % PALETTE.length],
      backgroundColor: PALETTE[si % PALETTE.length] + "18",
      borderWidth: 1.5,
      pointRadius: 0,
      pointHitRadius: 6,
      tension: 0.1,
      fill: false,
      spanGaps: false,
    };
  });

  // Update "now" indicator
  const nowEl = document.getElementById(def.nowId);
  if (nowEl && sel.length) {
    const last = sel[0].values[sel[0].values.length - 1];
    const div = def.div || 1;
    if (last) nowEl.textContent = `${(parseFloat(last[1]) / div).toFixed(1)}${def.unit}`;
  }

  if (state.charts[def.id]) {
    const chart = state.charts[def.id];
    chart.data.labels = labels;
    chart.data.datasets = datasets;
    chart.update("none");
    return;
  }

  const tc = getThemeColors();
  state.charts[def.id] = new Chart(canvas, {
    type: "line",
    data: { labels, datasets },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: { duration: 400 },
      interaction: { mode: "nearest", axis: "x", intersect: false },
      plugins: {
        legend: {
          position: "bottom",
          labels: {
            color: tc.text,
            font: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
            boxWidth: 10,
            boxHeight: 6,
            padding: 8,
            usePointStyle: true,
          },
        },
        tooltip: {
          mode: "index",
          intersect: false,
          backgroundColor: tc.tooltipBg,
          titleColor: tc.tooltipTitle,
          bodyColor: tc.tooltipBody,
          borderColor: tc.tooltipBorder,
          borderWidth: 1,
          padding: 10,
          cornerRadius: 8,
          bodyFont: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
          titleFont: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
        },
      },
      scales: {
        x: {
          type: "category",
          ticks: {
            color: tc.text,
            font: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
            maxTicksLimit: 8,
            maxRotation: 0,
          },
          grid: { color: tc.grid, drawBorder: false },
        },
        y: {
          beginAtZero: true,
          ticks: {
            color: tc.text,
            font: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
            maxTicksLimit: 6,
          },
          grid: { color: tc.grid, drawBorder: false },
        },
      },
    },
  });
}

function updateCharts(data, now) {
  for (const def of CHART_DEFS) {
    const resp = data[def.qid];
    if (!resp) continue;
    createOrUpdateChart(def, resp, now);
  }
}

/* -- control view -- */
let controlNodes = [];
let controlDisabled = false;

/*
 * Probe the server to learn whether the control plane is enabled.
 * The buildinfo endpoint reports it. When the control plane is off,
 * hide the Control tab entirely instead of showing a dead section.
 */
async function probeControlEnabled() {
  try {
    const info = await fetchJSON("/api/v1/status/buildinfo");
    const enabled = !!(info.data && info.data.controlEnabled);
    setControlTabVisible(enabled);
    if (enabled) await refreshControl();
  } catch (_) {
    // Fall back to showing the tab; refreshControl handles the disabled case.
    setControlTabVisible(true);
  }
}

function setControlTabVisible(visible) {
  const tab = document.querySelector('.tab[data-view="control"]');
  if (tab) tab.style.display = visible ? "" : "none";
  if (!visible && state.view === "control") {
    // Switch back to the cluster view so the user is not left on a hidden tab.
    state.view = "cluster";
    document.querySelectorAll(".tab").forEach((b) => b.classList.toggle("active", b.dataset.view === "cluster"));
    document.getElementById("view-cluster").hidden = false;
    document.getElementById("view-host").hidden = true;
    document.getElementById("view-control").hidden = true;
  }
}
function setupControlModal() {
  document.getElementById("control-modal-cancel").addEventListener("click", closeControlModal);
  document.getElementById("control-modal-confirm").addEventListener("click", confirmControlActionSend);
  document.getElementById("control-modal").addEventListener("click", (e) => {
    if (e.target === document.getElementById("control-modal")) closeControlModal();
  });
}
async function refreshControl() {
  try {
    const nodes = await fetchJSON("/api/v1/control/nodes");
    controlDisabled = false;
    controlNodes = Array.isArray(nodes) ? nodes : [];
    renderControlTable();
    setControlDisabled(false);
  } catch (e) {
    // 404 means the control plane is disabled.
    controlDisabled = true;
    controlNodes = [];
    renderControlTable();
    setControlDisabled(true);
  }
}

function setControlDisabled(disabled) {
  document.getElementById("control-disabled").hidden = !disabled;
}

function renderControlTable() {
  const body = document.getElementById("control-table-body");
  const empty = document.getElementById("control-empty");

  if (controlDisabled) {
    updateHTML(body, "", "control-table");
    empty.style.display = "none";
    return;
  }

  if (!controlNodes.length) {
    updateHTML(body, "", "control-table");
    empty.style.display = "block";
    return;
  }
  empty.style.display = "none";

  updateHTML(body, controlNodes.map((n) => {
    const host = esc(n.hostname);
    const gpus = n.gpus != null ? fmt(n.gpus, 0) : "—";
    const lastSeen = n.timestamp ? new Date(n.timestamp).toLocaleTimeString() : "—";
    const pending = n.pending_command;
    const pendingHtml = pending
      ? `<span class="badge-pending ${pending.op === "restart" ? "restart" : "shutdown"}">${esc(pending.op)} · ${new Date(pending.issued_at).toLocaleTimeString()}</span>`
      : '<span class="badge-none">none</span>';
    const disabled = pending ? "disabled" : "";
    return `<tr>
      <td class="idx">${host}</td>
      <td class="num">${gpus}</td>
      <td class="num">${lastSeen}</td>
      <td>${pendingHtml}</td>
      <td class="actions">
        <button class="btn-action restart" data-host="${esc(n.hostname)}" data-op="restart" ${disabled}>Restart</button>
        <button class="btn-action shutdown" data-host="${esc(n.hostname)}" data-op="shutdown" ${disabled}>Shutdown</button>
      </td>
    </tr>`;
  }).join(""), "control-table");

  body.querySelectorAll(".btn-action").forEach((btn) =>
    btn.addEventListener("click", () => confirmControlAction(btn.dataset.host, btn.dataset.op))
  );
}

/* -- confirmation modal -- */
let pendingAction = null;

function confirmControlAction(hostname, op) {
  pendingAction = { hostname, op };
  const modal = document.getElementById("control-modal");
  const title = document.getElementById("control-modal-title");
  const msg = document.getElementById("control-modal-msg");
  title.textContent = op === "restart" ? "Restart Node" : "Shutdown Node";
  msg.textContent = `Are you sure you want to ${op} ${hostname}? This action is destructive and cannot be undone.`;
  modal.hidden = false;
}

function closeControlModal() {
  pendingAction = null;
  document.getElementById("control-modal").hidden = true;
}

async function confirmControlActionSend() {
  if (!pendingAction) return;
  const { hostname, op } = pendingAction;
  closeControlModal();
  try {
    const r = await fetch(`/api/v1/control/nodes/${encodeURIComponent(hostname)}/${op}`, { method: "POST" });
    if (r.ok) {
      await r.json();
      showToast(`${op} queued for ${hostname}`, "success");
    } else {
      let msg = `${op} failed (${r.status})`;
      try {
        const err = await r.json();
        if (err && err.error && err.error.message) msg = err.error.message;
      } catch (_) { /* ignore parse error */ }
      showToast(msg, "error");
    }
    refreshControl();
  } catch (e) {
    showToast(`request failed: ${e.message}`, "error");
  }
}

/* -- toast -- */
let toastTimer = null;

function showToast(message, type) {
  const toast = document.getElementById("control-toast");
  toast.textContent = message;
  toast.className = "toast " + (type === "success" ? "success" : "error");
  toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast.hidden = true; }, 4000);
}

/* -- helpers -- */
function setStatus(s) {
  const el = document.getElementById("server-status");
  el.className = "badge" + (s === "offline" ? " offline" : "");
  el.innerHTML = `<span class="pulse"></span>${s}`;
}

function fmt(v, d) { return (v == null || Number.isNaN(v)) ? "—" : Number(v).toFixed(d); }
function fmtUptime(s) {
  if (!s) return "—";
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return `${d}d ${h}h ${m}m`;
}
function esc(s) { return String(s).replace(/[&<>"']/g, (c) => ({ "&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;" })[c]); }

init();
