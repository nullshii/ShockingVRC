import * as P from "./proto.js";
import * as Panel from "./panel.js";
import { t, applyDomI18n, toggleLang, onLangChange } from "./i18n.js";
import { initTheme, toggleThemePanel, isThemePanelOpen } from "./theme.js";

const $ = (s, el = document) => el.querySelector(s);
const TICK_MS = 25;
const PING_MS = 250;
const DEAD_MS = 6000;

const ui = {
  promptCmd: $("#prompt-cmd"),
  hero: $("#hero"),
  connectView: $("#connect-view"),
  code: $("#code"),
  codeMode: $("#code-mode"),
  pasteBtn: $("#paste-btn"),
  pass: $("#pass"),
  passRow: $("#pass-row"),
  passToggle: $("#pass-toggle"),
  name: $("#name"),
  go: $("#go"),
  cancelBtn: $("#cancel-btn"),
  connSteps: $("#conn-steps"),
  status: $("#status"),
  control: $("#control"),
  stop: $("#stop"),
  badge: $("#badge"),
  partner: $("#partner"),
  latency: $("#latency"),
  latencyDot: $("#latency-dot"),
  verify: $("#verify"),
  pauseAll: $("#pause-all"),
  feedbackBanner: $("#feedback-banner"),
  toast: $("#toast"),
  floatingToast: $("#floating-toast"),
  devices: $("#devices"),
};

const state = {
  session: null,
  connecting: null,
  toastTimer: 0,
  feedbackTimer: 0,
  verifyInfo: null,
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

function toast(text, kind = "", sticky = false) {
  ui.toast.textContent = text;
  ui.toast.className = kind;
  clearTimeout(state.toastTimer);
  if (!text) {
    ui.floatingToast.classList.add("hidden");
    return;
  }
  ui.floatingToast.textContent = text;
  ui.floatingToast.className = `floating-toast ${kind}`.trim();
  if (!sticky) {
    state.toastTimer = setTimeout(() => ui.floatingToast.classList.add("hidden"), 4200);
  }
}

function setStatus(text) {
  ui.status.textContent = text;
}

function setStep(activeStep) {
  if (!activeStep) {
    ui.connSteps.classList.add("hidden");
    return;
  }
  ui.connSteps.classList.remove("hidden");
  for (const el of ui.connSteps.querySelectorAll(".step")) {
    const n = Number(el.dataset.step);
    el.classList.toggle("done", n < activeStep);
    el.classList.toggle("active", n === activeStep);
  }
}

function updateCodeHints() {
  const val = ui.code.value.trim();
  const needsPass = P.linkNeedsPassphrase(val);
  ui.passRow.classList.toggle("hidden", !needsPass);
  if (!val) {
    ui.codeMode.classList.add("hidden");
    return;
  }
  ui.codeMode.classList.remove("hidden");
  if (P.isLink(val)) {
    ui.codeMode.textContent = needsPass ? t("modeLinkPass") : t("modeLink");
  } else if (P.normalizeCode(val)) {
    ui.codeMode.textContent = t("modeCode");
  } else {
    ui.codeMode.textContent = t("modeCodePartial");
  }
}

function renderVerifyText() {
  if (!state.verifyInfo) {
    ui.verify.textContent = "";
    return;
  }
  if (state.verifyInfo.type === "link") {
    ui.verify.textContent = state.verifyInfo.signatureChecked
      ? t("verifyLink", { fp: state.verifyInfo.fp })
      : t("verifyLinkNoSig", { fp: state.verifyInfo.fp });
  } else {
    ui.verify.textContent = t("verifyCode");
  }
}

function showFeedbackBanner(partner, kind) {
  clearTimeout(state.feedbackTimer);
  const text = kind === "more" ? t("feedbackMore", { partner }) : t("feedbackLess", { partner });
  ui.feedbackBanner.textContent = text;
  ui.feedbackBanner.classList.remove("hidden");
  toast(text, "ok");
  state.feedbackTimer = setTimeout(() => ui.feedbackBanner.classList.add("hidden"), 6000);
}

function messagePipe(dc) {
  const queue = [];
  const waiters = [];
  let closed = false;
  dc.onmessage = (ev) => {
    let msg;
    try {
      msg = P.decode(ev.data);
    } catch {
      return;
    }
    const w = waiters.shift();
    if (w) w.resolve(msg);
    else queue.push(msg);
  };
  const fail = () => {
    closed = true;
    while (waiters.length) waiters.shift().reject(new Error(t("errClosed")));
  };
  dc.onclose = fail;
  return {
    send: (bytes) => dc.readyState === "open" && dc.send(bytes),
    next: (timeoutMs = 90000) =>
      new Promise((resolve, reject) => {
        if (queue.length) return resolve(queue.shift());
        if (closed) return reject(new Error(t("errClosed")));
        const w = { resolve, reject };
        waiters.push(w);
        setTimeout(() => {
          const i = waiters.indexOf(w);
          if (i >= 0) {
            waiters.splice(i, 1);
            reject(new Error(t("errTimeout")));
          }
        }, timeoutMs);
      }),
    isClosed: () => closed,
  };
}

function wsUrl() {
  return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`;
}

function signaling(room, attempt) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl());
    attempt.ws = ws;
    const handlers = new Set();
    let joined = false;
    ws.onopen = () => {
      if (attempt.aborted) {
        ws.close();
        return;
      }
      ws.send(JSON.stringify({ op: "join", room, v: 2 }));
    };
    ws.onerror = () => reject(new Error(t("errReachServer")));
    ws.onclose = () => {
      if (!joined && !attempt.aborted) reject(new Error(t("errReachServer")));
    };
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
            no_room: t("errNoRoom"),
            rate_limited: t("errRateLimited"),
            busy: t("errBusy"),
          }[m.code];
          reject(new Error(why ?? m.message ?? m.code));
        }
        return;
      }
      if (m.op === "signal") handlers.forEach((f) => f(m.data));
    };
  });
}

function cancelConnect() {
  const a = state.connecting;
  if (!a) return;
  a.aborted = true;
  try { a.ws?.close(); } catch {}
  try { a.dc?.close(); } catch {}
  try { a.pc?.close(); } catch {}
  state.connecting = null;
  ui.go.disabled = false;
  ui.cancelBtn.classList.add("hidden");
  setStep(0);
  setStatus("");
  toast(t("toastCancelled"), "");
}

async function connect() {
  const text = ui.code.value.trim();
  if (!text || state.connecting) return;

  const attempt = { aborted: false, ws: null, pc: null, dc: null };
  state.connecting = attempt;
  ui.go.disabled = true;
  ui.cancelBtn.classList.remove("hidden");
  toast("");

  try {
    let room, grant, verifyInfo = { type: "code" };
    if (P.isLink(text)) {
      const { body, signatureChecked } = await P.decodeLink(text, ui.pass.value, () => setStatus(t("statusDeriving")));
      if (attempt.aborted) return;
      if (body.role === "controller") throw new Error(t("errWrongRole"));
      room = body.routes.room;
      grant = { id: body.invite, secret: P.unb64(body.secret), expect: P.unb64(body.pk) };
      verifyInfo = { type: "link", fp: body.fp, signatureChecked };
    } else {
      const code = P.normalizeCode(text);
      if (!code) throw new Error(t("errCodeLen"));
      room = `c-${code.slice(0, 4)}`;
      grant = { id: `code-${code.slice(0, 4)}`, secret: new TextEncoder().encode(code) };
    }

    setStep(1);
    setStatus(t("statusFinding"));
    const sig = await signaling(room, attempt);
    if (attempt.aborted) return;

    setStep(2);
    const pc = new RTCPeerConnection({ iceServers: sig.ice });
    const dc = pc.createDataChannel("svrc", { ordered: true });
    dc.binaryType = "arraybuffer";
    attempt.pc = pc;
    attempt.dc = dc;

    let offered = false;
    pc.onicecandidate = (ev) => {
      if (offered && ev.candidate) {
        sig.send({
          kind: "candidate",
          candidate: ev.candidate.candidate,
          sdpMid: ev.candidate.sdpMid,
          sdpMLineIndex: ev.candidate.sdpMLineIndex,
        });
      }
    };
    sig.on(async (data) => {
      try {
        if (data.kind === "answer") await pc.setRemoteDescription({ type: "answer", sdp: data.sdp });
        else if (data.kind === "candidate" && data.candidate) {
          await pc.addIceCandidate({ candidate: data.candidate, sdpMid: data.sdpMid, sdpMLineIndex: data.sdpMLineIndex });
        }
      } catch (e) {
        console.warn(e);
      }
    });

    await pc.setLocalDescription(await pc.createOffer());
    await new Promise((r) => {
      if (pc.iceGatheringState === "complete") return r();
      const timer = setTimeout(r, 3000);
      pc.onicegatheringstatechange = () => pc.iceGatheringState === "complete" && (clearTimeout(timer), r());
    });
    if (attempt.aborted) return;

    sig.send({ kind: "offer", sdp: pc.localDescription.sdp });
    offered = true;
    setStatus(t("statusP2P"));
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(t("errP2PTimeout"))), 25000);
      dc.onopen = () => (clearTimeout(timer), resolve());
      pc.onconnectionstatechange = () =>
        pc.connectionState === "failed" && (clearTimeout(timer), reject(new Error(t("errConnFailed"))));
    });
    sig.ws.close();
    if (attempt.aborted) return;

    setStep(3);
    setStatus(t("statusKeys"));
    const binding = await P.dtlsBinding(pc.localDescription.sdp, pc.remoteDescription.sdp);
    const pipe = messagePipe(dc);
    const name = ui.name.value.trim() || "Web panel";
    store("svrc-name", name);
    const { welcome, partner } = await P.join(pipe, {
      name,
      binding,
      grant,
      onPending: () => setStatus(t("statusPending")),
    });
    if (attempt.aborted) return;
    if (!welcome.you_control) throw new Error(t("errNoControl"));

    state.connecting = null;
    ui.cancelBtn.classList.add("hidden");
    setStep(0);
    start({ pc, dc, pipe, partner: welcome.name || partner, policy: welcome.policy, verifyInfo });
  } catch (e) {
    if (!attempt.aborted) {
      toast(e.message || String(e), "err");
      setStatus("");
    }
    try { attempt.ws?.close(); } catch {}
    try { attempt.dc?.close(); } catch {}
    try { attempt.pc?.close(); } catch {}
  } finally {
    if (state.connecting === attempt) state.connecting = null;
    ui.go.disabled = false;
    ui.cancelBtn.classList.add("hidden");
    setStep(0);
  }
}

function updateLatency(ms) {
  ui.latency.textContent = `${ms} ms`;
  ui.latencyDot.className = "dot " + (ms < 120 ? "good" : ms < 260 ? "warn" : "bad");
}

function start({ pc, dc, pipe, partner, policy, verifyInfo }) {
  const s = {
    pc, dc, pipe, partner,
    seq: 0, ping: 0, t0: performance.now(),
    ticker: null,
  };
  state.session = s;
  state.verifyInfo = verifyInfo;

  ui.hero.classList.add("hidden");
  ui.connectView.classList.add("hidden");
  ui.control.classList.remove("hidden");
  ui.stop.classList.remove("hidden");
  ui.badge.classList.remove("hidden");
  ui.promptCmd.textContent = "remote --live";
  ui.badge.textContent = `LIVE · ${partner}`;
  ui.partner.textContent = partner;
  ui.latency.textContent = "—";
  ui.latencyDot.className = "dot";
  renderVerifyText();
  Panel.renderDevices(ui.devices, policy);
  toast(t("toastConnected"), "ok");

  const now = () => Math.round(performance.now() - s.t0);
  (async () => {
    while (state.session === s) {
      let m;
      try {
        m = await pipe.next(DEAD_MS);
      } catch {
        if (state.session === s) end(t("errPartnerSilent"));
        return;
      }
      if (state.session !== s) return;
      if (m.t === "ping") pipe.send(P.encode({ t: "pong", seq: m.seq, ts: m.ts }));
      else if (m.t === "pong") updateLatency(Math.max(0, now() - m.ts));
      else if (m.t === "policy") Panel.renderDevices(ui.devices, m);
      else if (m.t === "output") {
        const [a, b] = Panel.showOutput(m.slots);
        ui.badge.textContent = `LIVE · ${partner} · A ${a} B ${b}`;
      } else if (m.t === "feedback") {
        showFeedbackBanner(partner, m.kind);
      } else if (m.t === "stop" || m.t === "bye") {
        end(
          m.t === "stop"
            ? t("errStoppedBy", { partner, reason: m.reason })
            : t("errPartnerLeft", { partner }),
        );
        return;
      }
    }
  })();

  const sendFrame = () => {
    if (state.session !== s) return;
    pipe.send(P.encode({ t: "frame", seq: ++s.seq, ts: now(), slots: Panel.frameSlots() }));
  };
  const sendPing = () => state.session === s && pipe.send(P.encode({ t: "ping", seq: ++s.ping, ts: now() }));
  s.ticker = startTicker(TICK_MS, PING_MS, sendFrame, sendPing);
  pc.onconnectionstatechange = () => {
    if (["failed", "closed"].includes(pc.connectionState) && state.session === s) end(t("errConnLost"));
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
  Panel.stopAll();
  if (notify) {
    s.pipe.send(P.encode({ t: "frame", seq: ++s.seq, ts: Math.round(performance.now() - s.t0), slots: Panel.frameSlots() }));
  }
  state.session = null;
  state.verifyInfo = null;
  s.ticker?.stop();
  if (notify) s.pipe.send(P.encode({ t: "stop", reason }));
  setTimeout(() => {
    try {
      s.dc.close();
      s.pc.close();
    } catch {}
  }, 150);

  Panel.reset(ui.devices);
  clearTimeout(state.feedbackTimer);
  ui.feedbackBanner.classList.add("hidden");
  ui.control.classList.add("hidden");
  ui.hero.classList.remove("hidden");
  ui.connectView.classList.remove("hidden");
  ui.stop.classList.add("hidden");
  ui.badge.classList.add("hidden");
  ui.promptCmd.textContent = t("promptCmd");
  ui.latency.textContent = "—";
  ui.latencyDot.className = "dot";
  setStatus("");
  toast(reason, notify ? "ok" : "err");
}

function typing(el) {
  return el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

applyDomI18n();
initTheme();

onLangChange(() => {
  updateCodeHints();
  renderVerifyText();
  Panel.refreshLabels();
  ui.passToggle.textContent = ui.pass.type === "password" ? t("btnShow") : t("btnHide");
  if (state.session) ui.promptCmd.textContent = "remote --live";
});

$("[data-lang-toggle]")?.addEventListener("click", toggleLang);

ui.pasteBtn.addEventListener("click", async () => {
  try {
    const clip = (await navigator.clipboard.readText()).trim();
    if (clip) {
      ui.code.value = clip;
      updateCodeHints();
      toast(t("toastPasted"), "ok");
    }
  } catch {
    toast(t("toastPasteFail"), "err");
  }
});

ui.passToggle.addEventListener("click", () => {
  const hidden = ui.pass.type === "password";
  ui.pass.type = hidden ? "text" : "password";
  ui.passToggle.textContent = hidden ? t("btnHide") : t("btnShow");
});

ui.pauseAll.addEventListener("click", () => {
  if (!state.session) return;
  Panel.stopAll();
  toast(t("toastPausedAll"), "ok");
});

ui.stop.addEventListener("click", () => end(t("toastStopped"), true));
ui.cancelBtn.addEventListener("click", cancelConnect);
ui.go.addEventListener("click", connect);
ui.code.addEventListener("keydown", (ev) => ev.key === "Enter" && connect());
ui.pass.addEventListener("keydown", (ev) => ev.key === "Enter" && connect());
ui.code.addEventListener("input", updateCodeHints);

// Keyboard shortcuts (safety-first: Esc ALWAYS stops an active session)
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") {
    if (state.session) {
      ev.preventDefault();
      end(t("toastStopped"), true);
      return;
    }
    if (isThemePanelOpen()) {
      ev.preventDefault();
      toggleThemePanel(false);
      return;
    }
    if (state.connecting) {
      ev.preventDefault();
      cancelConnect();
    }
    return;
  }

  if (ev.ctrlKey || ev.metaKey || ev.altKey || typing(ev.target)) return;

  if (ev.code === "Space" && state.session) {
    ev.preventDefault();
    Panel.stopAll();
    toast(t("toastPausedAll"), "ok");
    return;
  }

  const k = ev.key.toLowerCase();
  if (k === "t") {
    ev.preventDefault();
    toggleThemePanel();
  } else if (k === "r") {
    ev.preventDefault();
    toggleLang();
  }
});

window.addEventListener("blur", () => Panel.releaseHolds());
window.addEventListener("pagehide", () => state.session && end("page closed", true));
document.addEventListener("visibilitychange", () => {
  if (document.hidden) Panel.releaseHolds();
  if (state.session && document.hidden) toast(t("toastHiddenTab"), "", true);
  else if (state.session) toast(t("toastConnected"), "ok");
});

ui.name.value = store("svrc-name") || "";
if (location.hash.length > 1) {
  ui.code.value = decodeURIComponent(location.hash.slice(1));
  history.replaceState(null, "", location.pathname);
}
updateCodeHints();
