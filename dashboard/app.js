// dgmon dashboard — app logic (Chart.js)
"use strict";

const REFRESH_MS = 5000;
const PALETTE = ["#22d3ee", "#34d399", "#fbbf24", "#f87171", "#a78bfa", "#67e8f9", "#fde68a", "#fca5a5"];

const state = { nodes: [], selected: null, snaps: [], charts: {}, timer: null };

const fetchJSON = async (url) => {
  const r = await fetch(url);
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
    state.nodes = await fetchJSON("/nodes");
    renderNodeBar();
    document.getElementById("node-count").textContent = `${state.nodes.length} node${state.nodes.length === 1 ? "" : "s"}`;
    if (!state.selected && state.nodes.length) state.selected = state.nodes[0].hostname;
    setStatus("online");
  } catch { setStatus("offline"); }
}

function renderNodeBar() {
  const bar = document.getElementById("node-bar");
  bar.innerHTML = state.nodes.map((n) =>
    `<button class="${n.hostname === state.selected ? "active" : ""}" data-host="${n.hostname}">${n.hostname}</button>`
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
    const snaps = await fetchJSON("/snapshot");
    state.snaps = Array.isArray(snaps) ? snaps : [snaps];
    if (!state.selected && state.snaps.length) {
      state.selected = state.snaps[0].host.hostname;
      renderNodeBar();
    }
    renderHostCards();
    renderGpuTable();
    updateCharts();
    document.getElementById("refresh-time").textContent = new Date().toLocaleTimeString();
  } catch { /* noop */ }
}

function selSnap() { return state.snaps.find((s) => s.host.hostname === state.selected) || state.snaps[0]; }

/* -- host cards -- */
function renderHostCards() {
  const snap = selSnap();
  const host = document.getElementById("host-cards");
  if (!snap) { host.innerHTML = '<div class="empty-note">No data yet\u2026</div>'; return; }
  const h = snap.host;
  const cpuPct = h.cpu_usage_pct;
  const memPct = h.memory_total_mb ? Math.round((h.memory_used_mb / h.memory_total_mb) * 100) : 0;
  const diskPct = h.disk_total_gb ? Math.round((h.disk_used_gb / h.disk_total_gb) * 100) : 0;
  host.innerHTML = `
    <div class="stat">
      <div class="k">CPU</div>
      <div class="v">${fmt(cpuPct,1)}% <small>used</small></div>
      <div class="bar${cpuPct>80?cpuPct>95?' crit':' warn':''}"><i style="width:${cpuPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Memory</div>
      <div class="v">${fmt(h.memory_used_mb/1024,1)} <small>/ ${fmt(h.memory_total_mb/1024,0)} GiB</small></div>
      <div class="bar${memPct>80?memPct>95?' crit':' warn':''}"><i style="width:${memPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Disk</div>
      <div class="v">${fmt(h.disk_used_gb,0)} <small>/ ${fmt(h.disk_total_gb,0)} GiB</small></div>
      <div class="bar${diskPct>80?diskPct>95?' crit':' warn':''}"><i style="width:${diskPct}%"></i></div>
    </div>
    <div class="stat">
      <div class="k">Network</div>
      <div class="v">${fmt(h.network_rx_bytes/1e6,1)} <small>\u2193 / ${fmt(h.network_tx_bytes/1e6,1)} \u2191 MB/s</small></div>
    </div>
    <div class="stat">
      <div class="k">Uptime</div>
      <div class="v">${fmtUptime(h.uptime_seconds)}</div>
    </div>`;
}

/* -- gpu table -- */
function renderGpuTable() {
  const snap = selSnap();
  const body = document.getElementById("gpu-table-body");
  const empty = document.getElementById("gpu-empty");
  document.getElementById("gpu-count").textContent = snap && snap.gpus ? `${snap.gpus.length} GPU${snap.gpus.length!==1?'s':''}` : "\u2014";
  if (!snap || !snap.gpus || !snap.gpus.length) { body.innerHTML = ""; empty.style.display = "block"; return; }
  empty.style.display = "none";
  body.innerHTML = snap.gpus.map((g, i) => {
    const u = g.utilization_gpu ?? 0;
    const barCls = u > 80 ? (u > 95 ? "crit" : "hot") : "";
    return `<tr>
      <td class="idx">${i}</td>
      <td>${esc(g.name)}</td>
      <td style="font-size:10px;color:var(--txt-3)">${esc(g.uuid.slice(0,20))}\u2026</td>
      <td class="num"><span class="bar ${barCls}"><i style="width:${u}%"></i></span>${u}%</td>
      <td class="num">${g.utilization_memory ?? "\u2014"}%</td>
      <td class="num">${g.temperature_c ?? "\u2014"}\u00b0</td>
      <td class="num">${g.power_w ?? "\u2014"}</td>
      <td class="num">${g.fan_speed_pct ?? "\u2014"}%</td>
    </tr>`;
  }).join("");
}

/* -- charts (Chart.js) -- */
const CHART_DEFS = [
  { id: "chart-cpu", metric: "dgmon_cpu_usage_pct", nowId: "now-cpu", label: "CPU %", unit: "%" },
  { id: "chart-mem", metric: "dgmon_memory_used_mb", nowId: "now-mem", label: "Mem MB", unit: " MB" },
  { id: "chart-gpu", metric: "dgmon_gpu_utilization", nowId: "now-gpu", label: "GPU %", unit: "%" },
  { id: "chart-temp", metric: "dgmon_gpu_temp_c", nowId: "now-temp", label: "\u00b0C", unit: "\u00b0" },
];

function createOrUpdateChart(def, resp, now) {
  const el = document.getElementById(def.id);
  if (!el) return;
  const canvas = el.querySelector("canvas");
  if (!canvas) return;

  const seriesList = resp.result_type === "matrix" ? resp.result : [];
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
    const map = new Map(s.samples.map(([ts, v]) => [Math.floor(ts / 1000), v]));
    const data = times.map((t) => map.has(t / 1000) ? map.get(t / 1000) : null);
    const lbl = s.labels ? s.labels.filter(([k]) => k !== "hostname").map(([k, v]) => v).join(" ") : "";
    return {
      label: lbl || s.metric,
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
    const last = sel[0].samples[sel[0].samples.length - 1];
    if (last) nowEl.textContent = `${last[1]}${def.unit}`;
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
            color: "#94a3bf",
            font: { family: "ui-monospace, SF Mono, monospace", size: 10 },
            boxWidth: 10,
            boxHeight: 6,
            padding: 8,
            usePointStyle: true,
          },
        },
        tooltip: {
          mode: "index",
          intersect: false,
          backgroundColor: "rgba(12,16,26,0.92)",
          titleColor: "#e6edf7",
          bodyColor: "#94a3bf",
          borderColor: "#1c2740",
          borderWidth: 1,
          padding: 10,
          cornerRadius: 8,
          bodyFont: { family: "ui-monospace, SF Mono, monospace", size: 11 },
          titleFont: { family: "ui-monospace, SF Mono, monospace", size: 11 },
        },
      },
      scales: {
        x: {
          type: "category",
          ticks: {
            color: "#5c6b8a",
            font: { family: "ui-monospace, SF Mono, monospace", size: 10 },
            maxTicksLimit: 8,
            maxRotation: 0,
          },
          grid: { color: "rgba(92,107,138,0.15)", drawBorder: false },
        },
        y: {
          beginAtZero: true,
          ticks: {
            color: "#5c6b8a",
            font: { family: "ui-monospace, SF Mono, monospace", size: 10 },
            maxTicksLimit: 6,
          },
          grid: { color: "rgba(92,107,138,0.15)", drawBorder: false },
        },
      },
    },
  });
}

async function updateCharts() {
  const now = Date.now();
  const start = now - 3600e3;
  const step = 30000;
  const host = state.selected || "";
  for (const def of CHART_DEFS) {
    try {
      const q = host ? `${def.metric}{hostname="${host}"}` : def.metric;
      const resp = await fetchJSON(`/query?q=${encodeURIComponent(q)}&start=${start}&end=${now}&step=${step}`);
      createOrUpdateChart(def, resp, now);
    } catch (e) { console.warn(`chart ${def.id}:`, e); }
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
