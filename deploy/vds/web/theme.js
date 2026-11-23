import { t, onLangChange } from "./i18n.js";

const THEMES = [
  { id: "terminal",  name: "Terminal",  base: [.17, .012, 265], contrast: [.86, .015, 265], accent: [.80, .16, 158] },
  { id: "ember",     name: "Ember",     base: [.16, .022, 40],  contrast: [.87, .020, 50],  accent: [.72, .18, 42] },
  { id: "flare",     name: "Flare",     base: [.15, .016, 95],  contrast: [.88, .020, 95],  accent: [.86, .17, 96] },
  { id: "cobalt",    name: "Cobalt",    base: [.16, .022, 250], contrast: [.87, .020, 250], accent: [.74, .15, 235] },
  { id: "orchid",    name: "Orchid",    base: [.16, .022, 330], contrast: [.88, .015, 330], accent: [.75, .19, 340] },
  { id: "wire",      name: "Wire",      base: [.13, 0, 0],      contrast: [.74, 0, 0],      accent: [.98, 0, 0] },
  { id: "void",      name: "Void",      base: [.06, 0, 0],      contrast: [.92, .010, 300], accent: [.72, .20, 300] },
  { id: "paper",     name: "Paper",     base: [.97, .006, 95],  contrast: [.26, .012, 95],  accent: [.52, .14, 155] },
  { id: "blueprint", name: "Blueprint", base: [.95, .012, 240], contrast: [.28, .030, 250], accent: [.50, .16, 245] },
];

const root = document.documentElement;
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const prefersLight = matchMedia("(prefers-color-scheme: light)").matches;

function enc(x) {
  const v = x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(Math.max(x, 0), 1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, v)) * 255);
}

function oklch(L, C, H) {
  const h = (H * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
  const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
  const s = Math.pow(L - 0.0894841775 * a - 1.2914855480 * b, 3);
  return [
    enc(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    enc(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    enc(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
  ];
}

function cssColor(c) {
  const r = oklch(c[0], c[1], c[2]);
  return `rgb(${r[0]},${r[1]},${r[2]})`;
}

function load(k, d) {
  try {
    const v = localStorage.getItem("svrc." + k);
    return v === null ? d : v;
  } catch {
    return d;
  }
}

function save(k, v) {
  try {
    localStorage.setItem("svrc." + k, String(v));
  } catch {}
}

const state = {
  theme: load("theme", prefersLight ? "paper" : "terminal"),
  speed: parseFloat(load("speed", reduced ? "0" : "1")),
  density: parseFloat(load("density", "1")),
};

function themeById(id) {
  return THEMES.find((x) => x.id === id) || THEMES[0];
}

function clonePalette(p) {
  return { base: p.base.slice(), contrast: p.contrast.slice(), accent: p.accent.slice() };
}

function lerp(a, b, k) {
  return a + (b - a) * k;
}

function lerpH(a, b, k) {
  const d = (((b - a) % 360) + 540) % 360 - 180;
  return a + d * k;
}

function lerpC(a, b, k) {
  return [lerp(a[0], b[0], k), lerp(a[1], b[1], k), lerpH(a[2], b[2], k)];
}

let current = null;
let target = null;
let fadeFrom = null;
let fadeAt = 0;
const FADE_MS = 450;

const NEAR = 0.34;
const FAR = 6.4;
const EDGES = [
  [0, 1], [1, 3], [3, 2], [2, 0],
  [4, 5], [5, 7], [7, 6], [6, 4],
  [0, 4], [1, 5], [2, 6], [3, 7],
];

const field = { boxes: [], w: 0, h: 0, F: 0, spreadX: 0, spreadY: 0, dirty: true };
let canvas = null;
let ctx = null;
let panel = null;
let panelNodes = {};

function rand(a, b) {
  return a + Math.random() * (b - a);
}

function makeBox(anywhere) {
  const s = rand(0.09, 0.30);
  return {
    x: rand(-field.spreadX, field.spreadX),
    y: rand(-field.spreadY, field.spreadY),
    z: anywhere ? rand(NEAR, FAR) : rand(FAR * 0.82, FAR),
    w: s * rand(0.55, 1.7),
    h: s * rand(0.55, 1.7),
    d: s * rand(0.55, 1.7),
    rx: rand(0, 6.28),
    ry: rand(0, 6.28),
    rz: rand(0, 6.28),
    vx: rand(-0.22, 0.22),
    vy: rand(-0.22, 0.22),
    vz: rand(-0.14, 0.14),
    accent: Math.random() < 0.18,
  };
}

function targetCount() {
  const area = field.w * field.h;
  return Math.round(Math.min(90, Math.max(10, area / 26000)) * state.density);
}

function reflow() {
  const want = targetCount();
  while (field.boxes.length < want) field.boxes.push(makeBox(true));
  if (field.boxes.length > want) field.boxes.length = want;
}

function resize() {
  if (!ctx || !canvas) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  field.w = canvas.clientWidth;
  field.h = canvas.clientHeight;
  canvas.width = Math.round(field.w * dpr);
  canvas.height = Math.round(field.h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  field.F = Math.min(field.w, field.h) * 0.95;
  field.spreadX = (field.w * FAR) / (2 * field.F) * 1.2;
  field.spreadY = (field.h * FAR) / (2 * field.F) * 1.2;
  reflow();
  field.dirty = true;
}

function easePalette(now) {
  if (!fadeAt) return;
  const k = Math.min(1, (now - fadeAt) / FADE_MS);
  const e = k * k * (3 - 2 * k);
  current = {
    base: lerpC(fadeFrom.base, target.base, e),
    contrast: lerpC(fadeFrom.contrast, target.contrast, e),
    accent: lerpC(fadeFrom.accent, target.accent, e),
  };
  field.dirty = true;
  if (k === 1) fadeAt = 0;
}

function draw() {
  if (!ctx || !current) return;
  const { w: W, h: H, F } = field;
  const cx = W / 2;
  const cy = H / 2;
  ctx.clearRect(0, 0, W, H);

  const cCon = oklch(current.contrast[0], current.contrast[1], current.contrast[2]);
  const cAcc = oklch(current.accent[0], current.accent[1], current.accent[2]);
  const px = new Array(8);
  const py = new Array(8);

  for (let i = 0; i < field.boxes.length; i++) {
    const b = field.boxes[i];
    if (b.z <= NEAR) continue;
    let a = Math.min(1, (FAR - b.z) / 1.3) * Math.min(1, (b.z - NEAR) / 0.9);
    if (a <= 0.01) continue;
    a *= 0.5;

    const sx = Math.sin(b.rx), cx1 = Math.cos(b.rx);
    const sy = Math.sin(b.ry), cy1 = Math.cos(b.ry);
    const sz = Math.sin(b.rz), cz1 = Math.cos(b.rz);
    const hw = b.w / 2, hh = b.h / 2, hd = b.d / 2;
    let ok = true;

    for (let v = 0; v < 8; v++) {
      const X = v & 1 ? hw : -hw;
      const Y = v & 2 ? hh : -hh;
      const Z = v & 4 ? hd : -hd;
      const y1 = Y * cx1 - Z * sx;
      const z1 = Y * sx + Z * cx1;
      const x2 = X * cy1 + z1 * sy;
      const z2 = -X * sy + z1 * cy1;
      const x3 = x2 * cz1 - y1 * sz;
      const y3 = x2 * sz + y1 * cz1;
      const wz = b.z + z2;
      if (wz <= 0.05) {
        ok = false;
        break;
      }
      px[v] = cx + ((b.x + x3) / wz) * F;
      py[v] = cy + ((b.y + y3) / wz) * F;
    }
    if (!ok) continue;

    const c = b.accent ? cAcc : cCon;
    ctx.strokeStyle = `rgba(${c[0]},${c[1]},${c[2]},${a.toFixed(3)})`;
    ctx.lineWidth = Math.min(1.6, Math.max(0.5, 1.1 / b.z));
    ctx.beginPath();
    for (let e = 0; e < 12; e++) {
      const p = EDGES[e];
      ctx.moveTo(px[p[0]], py[p[0]]);
      ctx.lineTo(px[p[1]], py[p[1]]);
    }
    ctx.stroke();
  }
}

let last = 0;
function tick(now) {
  requestAnimationFrame(tick);
  if (document.hidden || !ctx) {
    last = now;
    return;
  }
  const dt = Math.min(0.05, (now - last) / 1000) || 0;
  last = now;

  easePalette(now);

  if (state.speed > 0) {
    for (let i = 0; i < field.boxes.length; i++) {
      const b = field.boxes[i];
      b.z -= 0.5 * state.speed * dt;
      b.rx += b.vx * dt;
      b.ry += b.vy * dt;
      b.rz += b.vz * dt;
      if (b.z <= NEAR) field.boxes[i] = makeBox(false);
    }
    field.dirty = true;
  }
  if (field.dirty) {
    draw();
    field.dirty = false;
  }
}

export function applyTheme(id, animate = true) {
  const th = themeById(id);
  state.theme = th.id;
  save("theme", th.id);
  const s = root.style;
  let vars = "";
  const set = (name, v) => {
    s.setProperty(`--${name}-l`, v[0]);
    s.setProperty(`--${name}-c`, v[1]);
    s.setProperty(`--${name}-h`, v[2]);
    vars += `--${name}-l:${v[0]};--${name}-c:${v[1]};--${name}-h:${v[2]};`;
  };
  set("base", th.base);
  set("contrast", th.contrast);
  set("accent", th.accent);
  const isLight = th.base[0] > 0.5;
  root.setAttribute("data-theme", th.id);
  root.style.colorScheme = isLight ? "light" : "dark";
  vars += `color-scheme:${isLight ? "light" : "dark"};`;
  save("vars", vars);

  target = { base: th.base, contrast: th.contrast, accent: th.accent };
  if (!current || !animate || reduced) {
    current = clonePalette(target);
    fadeAt = 0;
  } else {
    fadeAt = performance.now();
    fadeFrom = clonePalette(current);
  }

  for (const row of document.querySelectorAll(".theme-row[data-theme]")) {
    const active = row.dataset.theme === th.id;
    row.classList.toggle("active", active);
    row.setAttribute("aria-pressed", String(active));
  }
  field.dirty = true;
}

function makeSlider(id, labelText, min, max, step, value, onInput) {
  const frag = document.createDocumentFragment();
  const label = document.createElement("label");
  label.htmlFor = id;
  label.textContent = labelText;
  const input = document.createElement("input");
  input.type = "range";
  input.className = "ref-slider";
  input.id = id;
  input.min = min;
  input.max = max;
  input.step = step;
  input.value = value;
  input.addEventListener("input", () => onInput(parseFloat(input.value)));
  frag.append(label, input);
  return { frag, label };
}

function buildPanel() {
  panel = document.createElement("aside");
  panel.className = "panel";
  panel.hidden = true;
  panel.setAttribute("aria-label", t("themeTitle"));

  const bar = document.createElement("div");
  bar.className = "panel-bar";
  const close = document.createElement("button");
  close.className = "panel-close";
  close.type = "button";
  close.textContent = t("themeClose");
  close.addEventListener("click", () => toggleThemePanel(false));
  bar.appendChild(close);

  const body = document.createElement("div");
  body.className = "panel-body";

  const title = document.createElement("div");
  title.className = "panel-title";
  title.textContent = t("themePalette");
  body.appendChild(title);

  const rows = document.createElement("div");
  rows.className = "theme-rows";
  for (const th of THEMES) {
    const row = document.createElement("button");
    row.className = "theme-row";
    row.type = "button";
    row.dataset.theme = th.id;

    const name = document.createElement("span");
    name.className = "theme-name";
    name.textContent = th.name;
    row.append(name, document.createElement("hr"));

    for (const c of [th.base, th.contrast, th.accent]) {
      const sw = document.createElement("span");
      sw.className = "sw";
      const fill = document.createElement("i");
      fill.style.background = cssColor(c);
      sw.appendChild(fill);
      row.appendChild(sw);
    }

    row.addEventListener("click", () => applyTheme(th.id, true));
    rows.appendChild(row);
  }
  body.append(rows, document.createElement("hr"));

  const sliders = document.createElement("div");
  sliders.className = "theme-sliders";
  const sp = makeSlider("sl-bg-speed", t("themeSpeed"), 0, 3, 0.1, state.speed, (v) => {
    state.speed = v;
    save("speed", v);
    field.dirty = true;
  });
  const dn = makeSlider("sl-bg-density", t("themeDensity"), 0, 2, 0.1, state.density, (v) => {
    state.density = v;
    save("density", v);
    reflow();
    field.dirty = true;
  });
  sliders.append(sp.frag, dn.frag);
  body.appendChild(sliders);

  panel.append(bar, body);
  document.body.appendChild(panel);
  panelNodes = { close, title, speedLabel: sp.label, densityLabel: dn.label };
}

function updatePanelLabels() {
  if (!panel) return;
  panel.setAttribute("aria-label", t("themeTitle"));
  panelNodes.close.textContent = t("themeClose");
  panelNodes.title.textContent = t("themePalette");
  panelNodes.speedLabel.textContent = t("themeSpeed");
  panelNodes.densityLabel.textContent = t("themeDensity");
}

export function isThemePanelOpen() {
  return !!panel && !panel.hidden;
}

export function toggleThemePanel(open) {
  if (!panel) return;
  const show = open === undefined ? panel.hidden : open;
  panel.hidden = !show;
  const btn = document.querySelector("[data-theme-toggle]");
  if (btn) {
    btn.setAttribute("aria-expanded", String(show));
    if (!show) btn.focus();
  }
  if (show) {
    const first = panel.querySelector(".theme-row.active") || panel.querySelector(".theme-row");
    if (first) first.focus();
  }
}

export function initTheme() {
  canvas = document.getElementById("bg");
  ctx = canvas && canvas.getContext ? canvas.getContext("2d") : null;
  buildPanel();
  applyTheme(state.theme, false);
  onLangChange(updatePanelLabels);

  const btn = document.querySelector("[data-theme-toggle]");
  if (btn) btn.addEventListener("click", () => toggleThemePanel());

  document.addEventListener("click", (e) => {
    if (!panel || panel.hidden) return;
    if (panel.contains(e.target)) return;
    if (btn && btn.contains(e.target)) return;
    toggleThemePanel(false);
  });

  if (ctx) {
    resize();
    if (window.ResizeObserver) new ResizeObserver(resize).observe(canvas);
    else addEventListener("resize", resize, { passive: true });
    requestAnimationFrame(tick);
  }
}
