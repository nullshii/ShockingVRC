import { rawFromHz } from "./proto.js";
import { t } from "./i18n.js";

const SETTINGS = {
  hz:    { labelKey: "lblHz",    min: 1, max: 100, def: 50,  unit: "Hz", steps: false },
  int:   { labelKey: "lblInt",   min: 0, max: 100, def: 100, unit: "%",  steps: false },
  power: { labelKey: "lblPower", min: 0, max: 100, def: 20,  unit: "%",  steps: true },
  secs:  { labelKey: "lblSecs",  min: 1, max: 15,  def: 3,   unit: "s",  steps: false },
};
const KEYS = Object.keys(SETTINGS);
const TIME_PRESETS = [1, 3, 5, 10, 15];
const NAMES = ["A", "B"];
const SAVE_KEY = "svrc-channels";

const channels = new Map();
let shown = [];
let layout = "";
let lastRoot = null;
let lastPolicy = null;
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
    for (const c of channels.values()) {
      all[c.key] = { hz: c.hz, int: c.int, power: c.power, secs: c.secs };
    }
    localStorage.setItem(SAVE_KEY, JSON.stringify(all));
  } catch {}
}

function channel(slot, index) {
  const key = `${slot}:${index}`;
  let c = channels.get(key);
  if (!c) {
    const saved = loadSaved()[key] || {};
    c = { key, slot, index, until: 0, holding: false, runMs: 1, allowed: true, cap: 0, out: 0, view: null };
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
  const row = el("label", s.steps ? "slider-row with-steps" : "slider-row");
  const title = el("span", "", t(s.labelKey));
  const input = el("input");
  input.type = "range";
  input.min = s.min;
  input.max = s.max;
  input.step = 1;
  input.value = c[key];
  const out = el("output", "", `${c[key]} ${s.unit}`);

  const sync = (val, persist = false) => {
    c[key] = settle(key, val);
    input.value = c[key];
    out.textContent = `${c[key]} ${s.unit}`;
    if (key === "secs") paintOne(c, performance.now());
    if (persist) save();
  };

  input.addEventListener("input", () => sync(input.value, false));
  input.addEventListener("change", save);

  const controls = [input];
  if (s.steps) {
    const minus = el("button", "step-btn", "−");
    minus.type = "button";
    minus.setAttribute("aria-label", `${t(s.labelKey)} -1`);
    minus.addEventListener("click", (e) => {
      e.preventDefault();
      sync(c[key] - 1, true);
    });
    const plus = el("button", "step-btn", "+");
    plus.type = "button";
    plus.setAttribute("aria-label", `${t(s.labelKey)} +1`);
    plus.addEventListener("click", (e) => {
      e.preventDefault();
      sync(c[key] + 1, true);
    });
    controls.push(minus, plus);
    row.append(title, minus, input, plus, out);
  } else {
    row.append(title, input, out);
  }

  return { key, row, title, input, out, controls };
}

function channelView(c) {
  const col = el("div", "channel");
  const head = el("header", "channel-head");
  const heading = el("h3", "", t("channelTitle", { name: NAMES[c.index] }));
  const meta = el("span", "meta");
  head.append(heading, meta);

  const meter = el("div", "meter");
  const meterFill = el("div", "meter-fill");
  meter.appendChild(meterFill);

  const fireGroup = el("div", "fire-group");
  const button = el("button", "fire");
  button.type = "button";
  const label = el("span", "fire-label");
  button.appendChild(label);
  button.addEventListener("click", () => toggle(c));

  const holdBtn = el("button", "hold-btn", t("btnHold"));
  holdBtn.type = "button";
  const releaseHold = () => {
    if (c.holding) {
      c.holding = false;
      paint();
    }
  };
  holdBtn.addEventListener("pointerdown", (e) => {
    if (!c.allowed) return;
    e.preventDefault();
    try { holdBtn.setPointerCapture(e.pointerId); } catch {}
    c.holding = true;
    paint();
  });
  holdBtn.addEventListener("pointerup", releaseHold);
  holdBtn.addEventListener("pointercancel", releaseHold);
  holdBtn.addEventListener("lostpointercapture", releaseHold);
  holdBtn.addEventListener("keydown", (e) => {
    if ((e.key === "Enter" || e.key === " ") && !e.repeat && c.allowed) {
      e.preventDefault();
      c.holding = true;
      paint();
    }
  });
  holdBtn.addEventListener("keyup", releaseHold);

  fireGroup.append(button, holdBtn);

  const presets = el("div", "presets");
  const chips = TIME_PRESETS.map((secs) => {
    const chip = el("button", "chip", `${secs}s`);
    chip.type = "button";
    chip.dataset.secs = String(secs);
    chip.addEventListener("click", () => {
      c.secs = secs;
      const secsRow = rows.find((r) => r.key === "secs");
      if (secsRow) {
        secsRow.input.value = secs;
        secsRow.out.textContent = `${secs} ${SETTINGS.secs.unit}`;
      }
      save();
      paintOne(c, performance.now());
    });
    presets.appendChild(chip);
    return chip;
  });

  const rows = KEYS.map((k) => sliderRow(c, k));
  col.append(head, meter, fireGroup, presets, ...rows.map((r) => r.row));

  c.view = {
    col,
    heading,
    meta,
    meterFill,
    button,
    label,
    holdBtn,
    chips,
    rows,
    controls: [...rows.flatMap((r) => r.controls), ...chips],
  };
  return col;
}

function emptyState() {
  const box = el("div", "sheet empty");
  const h = el("h2", "heading");
  h.append(el("span", "", t("noDevicesHeading")));
  box.append(h, el("p", "dim", t("noDevicesBody")));
  return box;
}

export function renderDevices(root, policy) {
  lastRoot = root;
  lastPolicy = policy;
  const list = (policy?.slots ?? []).filter((d) => Number.isInteger(d?.slot));
  const next = JSON.stringify(list.map((d) => [d.slot, d.name]));
  for (const d of list) {
    const rules = d.channels ?? policy?.channels ?? [];
    NAMES.forEach((_, i) => {
      const c = channel(d.slot, i);
      const rule = rules[i];
      c.allowed = rule ? !!rule.allowed : true;
      c.cap = Number.isFinite(rule?.cap) ? rule.cap : 0;
      if (!c.allowed) {
        c.until = 0;
        c.holding = false;
      }
    });
  }
  const dropped = shown.filter((slot) => !list.some((d) => d.slot === slot));
  for (const c of channels.values()) {
    if (dropped.includes(c.slot)) {
      c.until = 0;
      c.holding = false;
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
      const h = el("h2", "heading");
      h.append(el("span", "", d.name || t("deviceSlot", { n: d.slot + 1 })));
      if (d.name) h.append(el("em", "", t("deviceSlot", { n: d.slot + 1 })));
      const grid = el("div", "channels");
      NAMES.forEach((_, i) => grid.append(channelView(channel(d.slot, i))));
      card.append(h, grid);
      root.append(card);
    }
  }
  paint();
}

export function refreshLabels() {
  if (lastRoot && lastPolicy) {
    layout = "";
    renderDevices(lastRoot, lastPolicy);
  }
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
  const timedOn = left > 0;
  const on = timedOn || c.holding;

  v.col.classList.toggle("on", on);
  v.col.classList.toggle("locked", !c.allowed);
  v.heading.textContent = t("channelTitle", { name: NAMES[c.index] });

  v.button.disabled = !c.allowed;
  v.button.setAttribute("aria-pressed", String(timedOn));
  v.button.style.setProperty("--left", timedOn ? `${((left / c.runMs) * 100).toFixed(2)}%` : "0%");
  v.label.textContent = !c.allowed
    ? t("lockedByPartner")
    : timedOn
      ? t("btnStopLeft", { left: (left / 1000).toFixed(1) })
      : t("btnTurnOn", { s: c.secs });

  v.holdBtn.disabled = !c.allowed;
  v.holdBtn.classList.toggle("holding", c.holding && c.allowed);
  v.holdBtn.textContent = c.holding && c.allowed ? t("btnHolding") : t("btnHold");

  v.meta.textContent = c.allowed ? t("metaOut", { out: c.out, cap: c.cap }) : "";
  const pct = c.allowed && c.cap > 0 ? clamp((c.out / c.cap) * 100, 0, 100) : 0;
  v.meterFill.style.width = `${pct.toFixed(1)}%`;

  for (const chip of v.chips) {
    chip.classList.toggle("active", Number(chip.dataset.secs) === c.secs);
  }
  for (const r of v.rows) {
    r.title.textContent = t(SETTINGS[r.key].labelKey);
  }
  for (const ctrl of v.controls) ctrl.disabled = !c.allowed;
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
  const on = !!c && c.allowed && (c.holding || c.until > now);
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

export function releaseHolds() {
  let changed = false;
  for (const c of channels.values()) {
    if (c.holding) {
      c.holding = false;
      changed = true;
    }
  }
  if (changed) paint();
}

export function stopAll() {
  for (const c of channels.values()) {
    c.until = 0;
    c.holding = false;
  }
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
  lastRoot = null;
  lastPolicy = null;
  root.replaceChildren();
}
