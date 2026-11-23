import { rawFromHz } from "./proto.js";

const SETTINGS = {
  hz: { label: "Frequency", min: 1, max: 100, def: 50, unit: "Hz" },
  int: { label: "Intensity", min: 0, max: 100, def: 100, unit: "%" },
  power: { label: "Power", min: 0, max: 100, def: 20, unit: "%" },
  secs: { label: "Time", min: 1, max: 15, def: 3, unit: "s" },
};
const KEYS = Object.keys(SETTINGS);
const NAMES = ["A", "B"];
const SAVE_KEY = "svrc-channels";

const channels = new Map();
let shown = [];
let layout = "";
let raf = 0;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

function settle(key, v) {
  const s = SETTINGS[key];
  const n = Number(v);
  return clamp(Number.isFinite(n) ? Math.round(n) : s.def, s.min, s.max);
}

function loadSaved() {
  try {
    const v = JSON.parse(localStorage.getItem(SAVE_KEY) || "{}");
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

function save() {
  try {
    const all = loadSaved();
    for (const c of channels.values()) all[c.key] = { hz: c.hz, int: c.int, power: c.power, secs: c.secs };
    localStorage.setItem(SAVE_KEY, JSON.stringify(all));
  } catch {}
}

function channel(slot, index) {
  const key = `${slot}:${index}`;
  let c = channels.get(key);
  if (!c) {
    const saved = loadSaved()[key] || {};
    c = { key, slot, index, until: 0, runMs: 1, allowed: true, cap: 0, out: 0, view: null };
    for (const k of KEYS) c[k] = settle(k, saved[k] ?? SETTINGS[k].def);
    channels.set(key, c);
  }
  return c;
}

function el(tag, cls = "", text = "") {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text) node.textContent = text;
  return node;
}

function sliderRow(c, key) {
  const s = SETTINGS[key];
  const row = el("label", "slider");
  const input = el("input");
  input.type = "range";
  input.min = s.min;
  input.max = s.max;
  input.step = 1;
  input.value = c[key];
  const out = el("output", "", `${c[key]} ${s.unit}`);
  input.addEventListener("input", () => {
    c[key] = settle(key, input.value);
    out.textContent = `${c[key]} ${s.unit}`;
    if (key === "secs") paintOne(c, performance.now());
  });
  input.addEventListener("change", save);
  row.append(el("span", "", s.label), input, out);
  return { row, input };
}

function channelView(c) {
  const col = el("div", "channel");
  const head = el("header", "channel-head");
  const meta = el("span", "meta");
  head.append(el("h3", "", `Channel ${NAMES[c.index]}`), meta);
  const button = el("button", "fire");
  button.type = "button";
  const label = el("span", "fire-label");
  button.append(label);
  button.addEventListener("click", () => toggle(c));
  const rows = KEYS.map((k) => sliderRow(c, k));
  col.append(head, button, ...rows.map((r) => r.row));
  c.view = { col, meta, button, label, inputs: rows.map((r) => r.input) };
  return col;
}

function emptyState() {
  const box = el("div", "sheet empty");
  const h = el("h2");
  h.append(el("span", "", "No devices yet"));
  box.append(
    h,
    el("p", "dim", "Your partner hasn't shared a device. Its channels appear here as soon as they connect one."),
  );
  return box;
}

export function renderDevices(root, policy) {
  const list = (policy?.slots ?? []).filter((d) => Number.isInteger(d?.slot));
  const next = JSON.stringify(list.map((d) => [d.slot, d.name]));
  for (const d of list) {
    const rules = d.channels ?? policy?.channels ?? [];
    NAMES.forEach((_, i) => {
      const c = channel(d.slot, i);
      const rule = rules[i];
      c.allowed = rule ? !!rule.allowed : true;
      c.cap = Number.isFinite(rule?.cap) ? rule.cap : 0;
      if (!c.allowed) c.until = 0;
    });
  }
  const dropped = shown.filter((slot) => !list.some((d) => d.slot === slot));
  for (const c of channels.values()) {
    if (dropped.includes(c.slot)) {
      c.until = 0;
      c.view = null;
    }
  }
  shown = list.map((d) => d.slot);
  if (next !== layout || !root.firstChild) {
    layout = next;
    root.replaceChildren();
    if (!list.length) root.append(emptyState());
    for (const d of list) {
      const card = el("section", "sheet device");
      const h = el("h2");
      h.append(el("span", "", d.name || `Device ${d.slot + 1}`));
      if (d.name) h.append(el("em", "", `device ${d.slot + 1}`));
      const grid = el("div", "channels");
      NAMES.forEach((_, i) => grid.append(channelView(channel(d.slot, i))));
      card.append(h, grid);
      root.append(card);
    }
  }
  paint();
}

function toggle(c) {
  if (!c.allowed) return;
  const now = performance.now();
  if (c.until > now) {
    c.until = 0;
  } else {
    c.runMs = c.secs * 1000;
    c.until = now + c.runMs;
  }
  paint();
}

function paintOne(c, now) {
  const v = c.view;
  if (!v) return;
  const left = Math.max(0, c.until - now);
  const on = left > 0;
  v.col.classList.toggle("on", on);
  v.col.classList.toggle("locked", !c.allowed);
  v.button.disabled = !c.allowed;
  v.button.setAttribute("aria-pressed", String(on));
  v.button.style.setProperty("--left", on ? `${((left / c.runMs) * 100).toFixed(2)}%` : "0%");
  v.label.textContent = !c.allowed
    ? "Locked by partner"
    : on
      ? `Stop · ${(left / 1000).toFixed(1)} s left`
      : `Turn on for ${c.secs} s`;
  v.meta.textContent = c.allowed ? `out ${c.out} · cap ${c.cap}` : "";
  for (const input of v.inputs) input.disabled = !c.allowed;
}

function paint() {
  const now = performance.now();
  let running = false;
  for (const c of channels.values()) {
    paintOne(c, now);
    running ||= c.until > now;
  }
  if (running && !raf) {
    raf = requestAnimationFrame(() => {
      raf = 0;
      paint();
    });
  }
}

function wire(c, now) {
  const on = !!c && c.allowed && c.until > now;
  const p = on ? Math.round(c.power * 10) / 1000 : 0;
  const f = rawFromHz(c ? c.hz : SETTINGS.hz.def);
  const i = on ? c.int : 0;
  return { p: [p, p, p, p], f: [f, f, f, f], i: [i, i, i, i], m: 200 };
}

export function frameSlots(now = performance.now()) {
  const slots = shown.length ? shown : [0];
  return slots.map((slot) => ({
    slot,
    a: wire(channels.get(`${slot}:0`), now),
    b: wire(channels.get(`${slot}:1`), now),
  }));
}

export function showOutput(report) {
  const peak = [0, 0];
  for (const entry of report ?? []) {
    const [slot, out] = entry ?? [];
    NAMES.forEach((_, i) => {
      const v = Number.isFinite(out?.[i]) ? out[i] : 0;
      peak[i] = Math.max(peak[i], v);
      const c = channels.get(`${slot}:${i}`);
      if (c) c.out = v;
    });
  }
  paint();
  return peak;
}

export function stopAll() {
  for (const c of channels.values()) c.until = 0;
  paint();
}

export function reset(root) {
  stopAll();
  for (const c of channels.values()) {
    c.view = null;
    c.out = 0;
  }
  shown = [];
  layout = "";
  root.replaceChildren();
}
