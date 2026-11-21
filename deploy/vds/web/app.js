import * as P from "./proto.js";
import * as Panel from "./panel.js";

const $ = (s, el = document) => el.querySelector(s);
const TICK_MS = 25;
const PING_MS = 250;
const DEAD_MS = 6000;

const ui = {
  code: $("#code"), pass: $("#pass"), passRow: $("#pass-row"), name: $("#name"), go: $("#go"),
  status: $("#status"), connect: $("#connect"), control: $("#control"), stop: $("#stop"),
  badge: $("#badge"), partner: $("#partner"), latency: $("#latency"),
  verify: $("#verify"), toast: $("#toast"), devices: $("#devices"),
};

const state = { session: null };

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
    pc, dc, pipe, partner,
    seq: 0, ping: 0, t0: performance.now(),
    ticker: null,
  };
  state.session = s;
  ui.connect.classList.add("hidden");
  ui.control.classList.remove("hidden");
  ui.stop.classList.remove("hidden");
  ui.badge.classList.remove("hidden");
  ui.badge.textContent = `REMOTE · ${partner}`;
  ui.partner.textContent = partner;
  ui.verify.textContent = linkNote || "Connected with a control code.";
  Panel.renderDevices(ui.devices, policy);
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
      if (state.session !== s) return;
      if (m.t === "ping") pipe.send(P.encode({ t: "pong", seq: m.seq, ts: m.ts }));
      else if (m.t === "pong") ui.latency.textContent = `${now() - m.ts} ms`;
      else if (m.t === "policy") Panel.renderDevices(ui.devices, m);
      else if (m.t === "output") {
        const [a, b] = Panel.showOutput(m.slots);
        ui.badge.textContent = `REMOTE · ${partner} · A ${a} B ${b}`;
      } else if (m.t === "feedback") {
        toast(`${partner} asks for ${m.kind === "more" ? "more" : "less"}`, "ok");
      } else if (m.t === "stop" || m.t === "bye") {
        end(m.t === "stop" ? `stopped by ${partner}: ${m.reason}` : `${partner} left`);
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
  Panel.stopAll();
  if (notify) s.pipe.send(P.encode({ t: "frame", seq: ++s.seq, ts: Math.round(performance.now() - s.t0), slots: Panel.frameSlots() }));
  state.session = null;
  s.ticker?.stop();
  if (notify) s.pipe.send(P.encode({ t: "stop", reason }));
  setTimeout(() => {
    try {
      s.dc.close();
      s.pc.close();
    } catch {}
  }, 150);
  Panel.reset(ui.devices);
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
