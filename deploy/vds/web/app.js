import * as P from "./proto.js";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const TICK_MS = 25;
const PING_MS = 250;
const DEAD_MS = 6000;

const ui = {
  code: $("#code"), pass: $("#pass"), passRow: $("#pass-row"), name: $("#name"), go: $("#go"),
  status: $("#status"), connect: $("#connect"), control: $("#control"), stop: $("#stop"),
  badge: $("#badge"), partner: $("#partner"), latency: $("#latency"), output: $("#output"),
  verify: $("#verify"), toast: $("#toast"), xy: $("#xy"), xyOn: $("#xy-on"),
};

const state = {
  channels: {
    a: { power: 0, max: 200, freq: [0.5, 0.5, 0.5, 0.5], int: [1, 1, 1, 1] },
    b: { power: 0, max: 200, freq: [0.5, 0.5, 0.5, 0.5], int: [1, 1, 1, 1] },
  },
  xy: { x: 0.5, y: 0, held: false },
  preset: "off",
  walk: 0.5,
  session: null,
};

function store(key, value) {
  try {
    if (value === undefined) return localStorage.getItem(key) ?? "";
    localStorage.setItem(key, value);
  } catch {
    return "";
  }
  return "";
}

function toast(text, kind = "") {
  ui.toast.textContent = text;
  ui.toast.className = kind;
}

function setStatus(text) {
  ui.status.textContent = text;
}

function buildBars(el, values, onChange) {
  el.innerHTML = "";
  values.forEach((v, i) => {
    const cell = document.createElement("div");
    const fill = document.createElement("i");
    cell.appendChild(fill);
    el.appendChild(cell);
    const paint = () => (fill.style.height = `calc(${(values[i] * 100).toFixed(1)}% - 4px)`);
    paint();
    const set = (ev) => {
      const r = cell.getBoundingClientRect();
      values[i] = Math.min(1, Math.max(0, 1 - (ev.clientY - r.top) / r.height));
      paint();
      onChange?.();
    };
    cell.addEventListener("pointerdown", (ev) => {
      cell.setPointerCapture(ev.pointerId);
      set(ev);
    });
    cell.addEventListener("pointermove", (ev) => {
      if (ev.buttons) set(ev);
    });
  });
}

for (const sheet of $$("[data-ch]")) {
  const ch = state.channels[sheet.dataset.ch];
  const power = $("input.power", sheet);
  const max = $("input.max", sheet);
  power.addEventListener("input", () => {
    ch.power = Number(power.value);
    power.nextElementSibling.textContent = `${Math.round(ch.power * 100)} %`;
  });
  max.addEventListener("input", () => {
    ch.max = Number(max.value);
    max.nextElementSibling.textContent = String(ch.max);
  });
  buildBars($(".bars.freq", sheet), ch.freq);
  buildBars($(".bars.int", sheet), ch.int);
}

function xyFromEvent(ev) {
  const r = ui.xy.getBoundingClientRect();
  state.xy.x = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width));
  state.xy.y = Math.min(1, Math.max(0, 1 - (ev.clientY - r.top) / r.height));
  const dot = $("i", ui.xy);
  dot.style.left = `${state.xy.x * 100}%`;
  dot.style.top = `${(1 - state.xy.y) * 100}%`;
}
ui.xy.addEventListener("pointerdown", (ev) => {
  ui.xy.setPointerCapture(ev.pointerId);
  state.xy.held = true;
  ui.xy.classList.add("held");
  xyFromEvent(ev);
});
ui.xy.addEventListener("pointermove", (ev) => {
  if (state.xy.held) xyFromEvent(ev);
});
const release = () => {
  state.xy.held = false;
  ui.xy.classList.remove("held");
};
ui.xy.addEventListener("pointerup", release);
ui.xy.addEventListener("pointercancel", release);

for (const b of $$("#presets button")) {
  b.addEventListener("click", () => {
    state.preset = b.dataset.p;
    $$("#presets button").forEach((x) => x.classList.toggle("on", x === b));
  });
}

function presetFactor(t) {
  switch (state.preset) {
    case "wave":
      return 0.5 - 0.5 * Math.cos((2 * Math.PI * t) / 4);
    case "ladder": {
      const u = (t / 4) % 1;
      const step = Math.floor(u * 4);
      return u * 4 - step < 0.5 ? (step + 1) / 4 : 0;
    }
    case "heart": {
      const u = (t / 0.8) % 1;
      if (u < 0.08) return 1;
      if (u > 0.16 && u < 0.24) return 0.6;
      return 0;
    }
    case "storm":
      state.walk = Math.min(1, Math.max(0.1, state.walk + (Math.random() - 0.5) * 0.15));
      return state.walk;
    default:
      return 1;
  }
}

const finite = (v, fallback) => (Number.isFinite(v) ? v : fallback);

function wireChannel(ch, factor, xyOnA) {
  const p = Math.min(1, Math.max(0, finite(ch.power * factor, 0)));
  const freq = xyOnA ? [0, 0, 0, 0].map(() => state.xy.x) : ch.freq;
  const int = xyOnA ? [0, 0, 0, 0].map(() => state.xy.y) : ch.int;
  return {
    p: [p, p, p, p].map((v) => Math.round(v * 1000) / 1000),
    f: freq.map((v) => P.rawFrequency(finite(v, 0.5))),
    i: int.map((v) => Math.round(finite(v, 0) * 100)),
    m: finite(ch.max, 200),
  };
}

function showPolicy(policy) {
  if (!policy) return;
  ["a", "b"].forEach((key, i) => {
    const c = policy.channels?.[i];
    const sheet = $(`[data-ch="${key}"]`);
    const cap = c && c.allowed ? c.cap : 0;
    $(".cap", sheet).textContent = c && c.allowed ? `cap ${cap}` : "locked by partner";
    $("label.max", sheet).style.setProperty("--cap", `${(cap / 200) * 100}%`);
  });
}

function messagePipe(dc) {
  const queue = [];
  const waiters = [];
  let closed = false;
  dc.onmessage = (ev) => {
    let msg;
    try {
      msg = P.decode(ev.data);
    } catch (e) {
      return;
    }
    const w = waiters.shift();
    if (w) w.resolve(msg);
    else queue.push(msg);
  };
  const fail = () => {
    closed = true;
    while (waiters.length) waiters.shift().reject(new Error("the connection closed"));
  };
  dc.onclose = fail;
  return {
    send: (bytes) => dc.readyState === "open" && dc.send(bytes),
    next: (timeoutMs = 90000) =>
      new Promise((resolve, reject) => {
        if (queue.length) return resolve(queue.shift());
        if (closed) return reject(new Error("the connection closed"));
        const w = { resolve, reject };
        waiters.push(w);
        setTimeout(() => {
          const i = waiters.indexOf(w);
          if (i >= 0) {
            waiters.splice(i, 1);
            reject(new Error("the partner took too long"));
          }
        }, timeoutMs);
      }),
    isClosed: () => closed,
  };
}

function wsUrl() {
  return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`;
}

function signaling(room) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl());
    const handlers = new Set();
    let joined = false;
    ws.onopen = () => ws.send(JSON.stringify({ op: "join", room, v: 2 }));
    ws.onerror = () => reject(new Error("cannot reach this server"));
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (!joined) {
        if (m.op === "joined") {
          joined = true;
          resolve({
            ws,
            host: m.peer,
            ice: m.ice?.servers ?? [],
            on: (f) => handlers.add(f),
            send: (data) => ws.readyState === 1 && ws.send(JSON.stringify({ op: "signal", to: m.peer, data })),
          });
        } else if (m.op === "error") {
          const why = {
            no_room: "nobody is waiting under that code",
            rate_limited: "too many attempts — wait a little",
            busy: "the partner is busy",
          }[m.code];
          reject(new Error(why ?? m.message ?? m.code));
        }
        return;
      }
      if (m.op === "signal") handlers.forEach((f) => f(m.data));
    };
  });
}

async function connect() {
  const text = ui.code.value.trim();
  if (!text) return;
  ui.go.disabled = true;
  toast("");
  try {
    let room, grant, linkNote = "";
    if (P.isLink(text)) {
      const { body, signatureChecked } = await P.decodeLink(text, ui.pass.value, setStatus);
      if (body.role === "controller") throw new Error("this link lets them control you — open it in the ShockingVRC app");
      room = body.routes.room;
      grant = { id: body.invite, secret: P.unb64(body.secret), expect: P.unb64(body.pk) };
      linkNote = `partner fingerprint ${body.fp}${signatureChecked ? "" : " (this browser cannot check signatures)"}`;
    } else {
      const code = P.normalizeCode(text);
      if (!code) throw new Error("a control code has 16 characters");
      room = `c-${code.slice(0, 4)}`;
      grant = { id: `code-${code.slice(0, 4)}`, secret: new TextEncoder().encode(code) };
    }
    setStatus("finding your partner…");
    const sig = await signaling(room);
    const pc = new RTCPeerConnection({ iceServers: sig.ice });
    const dc = pc.createDataChannel("svrc", { ordered: true });
    dc.binaryType = "arraybuffer";
    let offered = false;
    pc.onicecandidate = (ev) => {
      if (offered && ev.candidate) {
        sig.send({ kind: "candidate", candidate: ev.candidate.candidate, sdpMid: ev.candidate.sdpMid, sdpMLineIndex: ev.candidate.sdpMLineIndex });
      }
    };
    sig.on(async (data) => {
      try {
        if (data.kind === "answer") await pc.setRemoteDescription({ type: "answer", sdp: data.sdp });
        else if (data.kind === "candidate" && data.candidate) await pc.addIceCandidate({ candidate: data.candidate, sdpMid: data.sdpMid, sdpMLineIndex: data.sdpMLineIndex });
      } catch (e) {
        console.warn(e);
      }
    });
    await pc.setLocalDescription(await pc.createOffer());
    await new Promise((r) => {
      if (pc.iceGatheringState === "complete") return r();
      const t = setTimeout(r, 3000);
      pc.onicegatheringstatechange = () => pc.iceGatheringState === "complete" && (clearTimeout(t), r());
    });
    sig.send({ kind: "offer", sdp: pc.localDescription.sdp });
    offered = true;
    setStatus("connecting peer to peer…");
    await new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("could not reach your partner directly or by relay")), 25000);
      dc.onopen = () => (clearTimeout(t), resolve());
      pc.onconnectionstatechange = () => pc.connectionState === "failed" && (clearTimeout(t), reject(new Error("the connection failed")));
    });
    sig.ws.close();
    setStatus("checking each other's keys…");
    const binding = await P.dtlsBinding(pc.localDescription.sdp, pc.remoteDescription.sdp);
    const pipe = messagePipe(dc);
    const name = ui.name.value.trim() || "Web panel";
    store("svrc-name", name);
    const { welcome, partner } = await P.join(pipe, {
      name,
      binding,
      grant,
      onPending: () => setStatus("waiting for your partner to accept…"),
    });
    if (!welcome.you_control) throw new Error("this invite does not let you control them");
    start({ pc, dc, pipe, partner: welcome.name || partner, policy: welcome.policy, linkNote });
  } catch (e) {
    toast(e.message || String(e), "err");
    setStatus("");
  } finally {
    ui.go.disabled = false;
  }
}

function start({ pc, dc, pipe, partner, policy, linkNote }) {
  const s = {
    pc, dc, pipe, partner, policy,
    seq: 0, ping: 0, t0: performance.now(), lastRx: performance.now(),
    ticker: null,
  };
  state.session = s;
  ui.connect.classList.add("hidden");
  ui.control.classList.remove("hidden");
  ui.stop.classList.remove("hidden");
  ui.badge.classList.remove("hidden");
  ui.partner.textContent = partner;
  ui.verify.textContent = linkNote || "Connected with a control code.";
  showPolicy(policy);
  toast("connected", "ok");

  const now = () => Math.round(performance.now() - s.t0);
  (async () => {
    while (state.session === s) {
      let m;
      try {
        m = await pipe.next(DEAD_MS);
      } catch {
        if (state.session === s) end("the partner stopped answering");
        return;
      }
      s.lastRx = performance.now();
      if (m.t === "ping") pipe.send(P.encode({ t: "pong", seq: m.seq, ts: m.ts }));
      else if (m.t === "pong") ui.latency.textContent = `${now() - m.ts} ms`;
      else if (m.t === "policy") {
        s.policy = m;
        showPolicy(m);
      } else if (m.t === "output") {
        const out = (m.slots || []).reduce((acc, [, o]) => [Math.max(acc[0], o[0]), Math.max(acc[1], o[1])], [0, 0]);
        ui.output.textContent = `A ${out[0]} · B ${out[1]}`;
        ui.badge.textContent = `REMOTE · ${partner} · A ${out[0]} B ${out[1]}`;
      } else if (m.t === "feedback") {
        toast(`${partner} asks for ${m.kind === "more" ? "more ▲" : "less ▼"}`, "ok");
      } else if (m.t === "stop" || m.t === "bye") {
        end(m.t === "stop" ? `stopped by ${partner}: ${m.reason}` : `${partner} left`);
        return;
      }
    }
  })();

  const sendFrame = () => {
    if (state.session !== s) return;
    const t = (performance.now() - s.t0) / 1000;
    const factor = presetFactor(t);
    const xyOnA = ui.xyOn.checked && state.xy.held;
    const slots = (s.policy?.slots?.length ? s.policy.slots.map((x) => x.slot) : [0]).map((slot) => ({
      slot,
      a: wireChannel(state.channels.a, factor, xyOnA),
      b: wireChannel(state.channels.b, factor, false),
    }));
    pipe.send(P.encode({ t: "frame", seq: ++s.seq, ts: now(), slots }));
  };
  const sendPing = () => state.session === s && pipe.send(P.encode({ t: "ping", seq: ++s.ping, ts: now() }));
  s.ticker = startTicker(TICK_MS, PING_MS, sendFrame, sendPing);
  pc.onconnectionstatechange = () => {
    if (["failed", "closed"].includes(pc.connectionState) && state.session === s) end("connection lost");
  };
}

function startTicker(frameMs, pingMs, onFrame, onPing) {
  try {
    const src = `let n=0;setInterval(()=>{n++;postMessage(n%${Math.max(1, Math.round(pingMs / frameMs))}===0)},${frameMs});`;
    const url = URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
    const worker = new Worker(url);
    URL.revokeObjectURL(url);
    worker.onmessage = (ev) => {
      onFrame();
      if (ev.data) onPing();
    };
    return { stop: () => worker.terminate() };
  } catch {
    const a = setInterval(onFrame, frameMs);
    const b = setInterval(onPing, pingMs);
    return { stop: () => (clearInterval(a), clearInterval(b)) };
  }
}

function end(reason, notify = false) {
  const s = state.session;
  if (!s) return;
  state.session = null;
  s.ticker?.stop();
  if (notify) s.pipe.send(P.encode({ t: "stop", reason }));
  setTimeout(() => {
    try {
      s.dc.close();
      s.pc.close();
    } catch {}
  }, 150);
  for (const ch of Object.values(state.channels)) ch.power = 0;
  $$("input.power").forEach((el) => {
    el.value = 0;
    el.nextElementSibling.textContent = "0 %";
  });
  ui.control.classList.add("hidden");
  ui.connect.classList.remove("hidden");
  ui.stop.classList.add("hidden");
  ui.badge.classList.add("hidden");
  ui.latency.textContent = "—";
  setStatus("");
  toast(reason, notify ? "ok" : "err");
}

ui.stop.addEventListener("click", () => end("stopped", true));
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && state.session) end("stopped", true);
});
window.addEventListener("pagehide", () => state.session && end("page closed", true));
document.addEventListener("visibilitychange", () => {
  if (state.session && document.hidden) toast("keep this tab open — some browsers slow hidden tabs", "");
  else if (state.session) toast("connected", "ok");
});

ui.go.addEventListener("click", connect);
ui.code.addEventListener("keydown", (ev) => ev.key === "Enter" && connect());
ui.code.addEventListener("input", () => ui.passRow.classList.toggle("hidden", !P.linkNeedsPassphrase(ui.code.value)));
ui.name.value = store("svrc-name") || "";
if (location.hash.length > 1) {
  ui.code.value = decodeURIComponent(location.hash.slice(1));
  history.replaceState(null, "", location.pathname);
  ui.passRow.classList.toggle("hidden", !P.linkNeedsPassphrase(ui.code.value));
}
