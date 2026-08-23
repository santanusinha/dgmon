// dgmon dashboard — app logic (Chart.js)
// All data comes from the Prometheus-compatible API (/api/v1/) using PromQL.
"use strict";

const REFRESH_MS = 5000;
const PALETTE = ["#2563eb", "#16a34a", "#d97706", "#dc2626", "#7c3aed", "#0891b2", "#ca8a04", "#be185d"];

const state = { nodes: [], selected: null, charts: {}, timer: null };

const fetchJSON = async (url, opts) => {
  const r = await fetch(url, opts);
  if (!r.ok) throw new Error(`${url} -> ${r.status}`);
  return r.json();
};

/* -- boot -- */
async function init() {
  await refreshNodes();
  setInterval(refreshNodes, 15000);
  await refreshAll();
  state.timer = setInterval(refreshAll, REFRESH_MS);
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
      // Host cards (instant).
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
      // Charts (range).
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

  if (cpuPct == null) { host.innerHTML = '<div class="empty-note">No data yet\u2026</div>'; return; }

  const memPct = memTotal ? Math.round((memUsed / memTotal) * 100) : 0;
  const diskPct = diskTotal ? Math.round((diskUsed / diskTotal) * 100) : 0;
  host.innerHTML = `
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
      <div class="v">${fmt(netRx/1e6,1)} <small>\u2193 / ${fmt(netTx/1e6,1)} \u2191 MB/s</small></div>
    </div>
    <div class="stat">
      <div class="k">Uptime</div>
      <div class="v">${fmtUptime(uptime)}</div>
    </div>`;
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

  document.getElementById("gpu-count").textContent = util.length ? `${util.length} GPU${util.length!==1?'s':''}` : "\u2014";
  if (!util.length) { body.innerHTML = ""; empty.style.display = "block"; return; }
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

  body.innerHTML = idxs.map((g) => {
    const u = utilM[g] ? parseFloat(utilM[g].value[1]) : 0;
    const barCls = u > 80 ? (u > 95 ? "crit" : "hot") : "";
    const metric = utilM[g]?.metric || {};
    const model = metric.model || "\u2014";
    const uuid = metric.uuid || "\u2014";
    const mu = memUtilM[g] ? parseFloat(memUtilM[g].value[1]) : null;
    const muUsed = memUsedM[g] ? parseFloat(memUsedM[g].value[1]) : null;
    const muTotal = memTotalM[g] ? parseFloat(memTotalM[g].value[1]) : null;
    const t = tempM[g] ? parseFloat(tempM[g].value[1]) : null;
    const p = powerM[g] ? parseFloat(powerM[g].value[1]) : null;
    const f = fanM[g] ? parseFloat(fanM[g].value[1]) : null;
    const sc = smClockM[g] ? parseFloat(smClockM[g].value[1]) : null;
    const mc = memClockM[g] ? parseFloat(memClockM[g].value[1]) : null;
    const memUsedStr = muUsed != null ? (muTotal != null ? `${fmt(muUsed/1024,1)}/${fmt(muTotal/1024,0)}G` : `${fmt(muUsed/1024,1)}G`) : "\u2014";
    return `<tr>
      <td class="idx">${g}</td>
      <td>${esc(model)}</td>
      <td style="font-size:10px;color:var(--txt-3)">${esc(uuid.slice(0,20))}\u2026</td>
      <td class="num"><span class="bar ${barCls}"><i style="width:${u}%"></i></span>${fmt(u,0)}%</td>
      <td class="num">${mu != null ? fmt(mu,0)+"%" : "\u2014"}</td>
      <td class="num">${memUsedStr}</td>
      <td class="num">${t != null ? fmt(t,0)+"\u00b0" : "\u2014"}</td>
      <td class="num">${p != null ? fmt(p,0) : "\u2014"}</td>
      <td class="num">${f != null ? fmt(f,0)+"%" : "\u2014"}</td>
      <td class="num">${sc != null ? fmt(sc,0)+" MHz" : "\u2014"}</td>
      <td class="num">${mc != null ? fmt(mc,0)+" MHz" : "\u2014"}</td>
    </tr>`;
  }).join("");
}

/* -- charts (Chart.js) -- */
const CHART_DEFS = [
  { id: "chart-cpu", qid: "chart-cpu", nowId: "now-cpu", label: "CPU %", unit: "%" },
  { id: "chart-mem", qid: "chart-mem", nowId: "now-mem", label: "Mem MB", unit: " MB" },
  { id: "chart-gpu", qid: "chart-gpu", nowId: "now-gpu", label: "GPU %", unit: "%" },
  { id: "chart-temp", qid: "chart-temp", nowId: "now-temp", label: "\u00b0C", unit: "\u00b0" },
  { id: "chart-gpu-mem", qid: "chart-gpu-mem", nowId: "now-gpu-mem", label: "GPU Mem GB", unit: " GB", div: 1024 },
  { id: "chart-gpu-power", qid: "chart-gpu-power", nowId: "now-gpu-power", label: "Power W", unit: " W" },
  { id: "chart-sm-clock", qid: "chart-sm-clock", nowId: "now-sm-clock", label: "SM MHz", unit: " MHz" },
  { id: "chart-mem-clock", qid: "chart-mem-clock", nowId: "now-mem-clock", label: "Mem MHz", unit: " MHz" },
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
            color: "#8a94a6",
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
          backgroundColor: "rgba(255,255,255,0.95)",
          titleColor: "#1a2233",
          bodyColor: "#4b5563",
          borderColor: "#e3e8ef",
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
            color: "#8a94a6",
            font: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
            maxTicksLimit: 8,
            maxRotation: 0,
          },
          grid: { color: "rgba(138,148,166,0.18)", drawBorder: false },
        },
        y: {
          beginAtZero: true,
          ticks: {
            color: "#8a94a6",
            font: { family: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif", size: 11 },
            maxTicksLimit: 6,
          },
          grid: { color: "rgba(138,148,166,0.18)", drawBorder: false },
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

/* -- helpers -- */
function setStatus(s) {
  const el = document.getElementById("server-status");
  el.className = "badge" + (s === "offline" ? " offline" : "");
  el.innerHTML = `<span class="pulse"></span>${s}`;
}

function fmt(v, d) { return (v == null || Number.isNaN(v)) ? "\u2014" : Number(v).toFixed(d); }
function fmtUptime(s) {
  if (!s) return "\u2014";
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return `${d}d ${h}h ${m}m`;
}
function esc(s) { return String(s).replace(/[&<>"']/g, (c) => ({ "&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;" })[c]); }

init();
