const enc = new TextEncoder();
const dec = new TextDecoder();
const subtle = globalThis.crypto.subtle;

export const PROTOCOL_VERSION = 2;

export function b64(bytes) {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function unb64(text) {
  const s = text.trim().replace(/-/g, "+").replace(/_/g, "/");
  const padded = s + "===".slice((s.length + 3) % 4);
  const bin = atob(padded);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function concat(...parts) {
  const len = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(len);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

export function random(n) {
  return globalThis.crypto.getRandomValues(new Uint8Array(n));
}

function u32be(n) {
  return new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
}

export async function sha256(...parts) {
  return new Uint8Array(await subtle.digest("SHA-256", concat(...parts)));
}

export async function hmac(key, ...parts) {
  const k = await subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await subtle.sign("HMAC", k, concat(...parts)));
}

export function ctEqual(a, b) {
  if (a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a[i] ^ b[i];
  return d === 0;
}

export async function verifyEd25519(pk, data, sig) {
  try {
    const key = await subtle.importKey("raw", pk, { name: "Ed25519" }, false, ["verify"]);
    return await subtle.verify({ name: "Ed25519" }, key, sig, data);
  } catch (e) {
    return null;
  }
}

export function encode(msg) {
  const body = enc.encode(JSON.stringify(msg));
  return concat(new Uint8Array([0x53, 0x56, PROTOCOL_VERSION, 0]), u32be(body.length), body);
}

export function decode(bytes) {
  const b = new Uint8Array(bytes);
  if (b.length < 8 || b[0] !== 0x53 || b[1] !== 0x56) throw new Error("not a ShockingVRC message");
  if (b[2] !== PROTOCOL_VERSION) throw new Error(`protocol version ${b[2]} is not supported`);
  const len = ((b[4] << 24) | (b[5] << 16) | (b[6] << 8) | b[7]) >>> 0;
  if (len !== b.length - 8) throw new Error("length mismatch");
  return JSON.parse(dec.decode(b.subarray(8)));
}

const IV32 = new Uint32Array([
  0xf3bcc908, 0x6a09e667, 0x84caa73b, 0xbb67ae85, 0xfe94f82b, 0x3c6ef372, 0x5f1d36f1, 0xa54ff53a,
  0xade682d1, 0x510e527f, 0x2b3e6c1f, 0x9b05688c, 0xfb41bd6b, 0x1f83d9ab, 0x137e2179, 0x5be0cd19,
]);
const SIGMA = [
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
  11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4, 7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8,
  9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13, 2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9,
  12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11, 13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10,
  6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5, 10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0,
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
].map((x) => x * 2);

const bv = new Uint32Array(32);
const bm = new Uint32Array(32);

function add64aa(v, a, b) {
  const o0 = v[a] + v[b];
  let o1 = v[a + 1] + v[b + 1];
  if (o0 >= 0x100000000) o1++;
  v[a] = o0;
  v[a + 1] = o1;
}

function add64ac(v, a, b0, b1) {
  let o0 = v[a] + b0;
  if (b0 < 0) o0 += 0x100000000;
  let o1 = v[a + 1] + b1;
  if (o0 >= 0x100000000) o1++;
  v[a] = o0;
  v[a + 1] = o1;
}

function bG(a, b, c, d, ix, iy) {
  const x0 = bm[ix], x1 = bm[ix + 1], y0 = bm[iy], y1 = bm[iy + 1];
  add64aa(bv, a, b);
  add64ac(bv, a, x0, x1);
  let x = bv[d] ^ bv[a], y = bv[d + 1] ^ bv[a + 1];
  bv[d] = y;
  bv[d + 1] = x;
  add64aa(bv, c, d);
  x = bv[b] ^ bv[c];
  y = bv[b + 1] ^ bv[c + 1];
  bv[b] = (x >>> 24) ^ (y << 8);
  bv[b + 1] = (y >>> 24) ^ (x << 8);
  add64aa(bv, a, b);
  add64ac(bv, a, y0, y1);
  x = bv[d] ^ bv[a];
  y = bv[d + 1] ^ bv[a + 1];
  bv[d] = (x >>> 16) ^ (y << 16);
  bv[d + 1] = (y >>> 16) ^ (x << 16);
  add64aa(bv, c, d);
  x = bv[b] ^ bv[c];
  y = bv[b + 1] ^ bv[c + 1];
  bv[b] = (y >>> 31) ^ (x << 1);
  bv[b + 1] = (x >>> 31) ^ (y << 1);
}

function compress(ctx, last) {
  for (let i = 0; i < 16; i++) {
    bv[i] = ctx.h[i];
    bv[i + 16] = IV32[i];
  }
  bv[24] ^= ctx.t;
  bv[25] ^= ctx.t / 0x100000000;
  if (last) {
    bv[28] = ~bv[28];
    bv[29] = ~bv[29];
  }
  for (let i = 0; i < 32; i++) {
    const o = 4 * i;
    bm[i] = ctx.b[o] ^ (ctx.b[o + 1] << 8) ^ (ctx.b[o + 2] << 16) ^ (ctx.b[o + 3] << 24);
  }
  for (let i = 0; i < 12; i++) {
    const s = i * 16;
    bG(0, 8, 16, 24, SIGMA[s + 0], SIGMA[s + 1]);
    bG(2, 10, 18, 26, SIGMA[s + 2], SIGMA[s + 3]);
    bG(4, 12, 20, 28, SIGMA[s + 4], SIGMA[s + 5]);
    bG(6, 14, 22, 30, SIGMA[s + 6], SIGMA[s + 7]);
    bG(0, 10, 20, 30, SIGMA[s + 8], SIGMA[s + 9]);
    bG(2, 12, 22, 24, SIGMA[s + 10], SIGMA[s + 11]);
    bG(4, 14, 16, 26, SIGMA[s + 12], SIGMA[s + 13]);
    bG(6, 8, 18, 28, SIGMA[s + 14], SIGMA[s + 15]);
  }
  for (let i = 0; i < 16; i++) ctx.h[i] = ctx.h[i] ^ bv[i] ^ bv[i + 16];
}

export function blake2b(input, outlen = 64) {
  const ctx = { b: new Uint8Array(128), h: new Uint32Array(16), t: 0, c: 0 };
  for (let i = 0; i < 16; i++) ctx.h[i] = IV32[i];
  ctx.h[0] ^= 0x01010000 ^ outlen;
  for (let i = 0; i < input.length; i++) {
    if (ctx.c === 128) {
      ctx.t += ctx.c;
      compress(ctx, false);
      ctx.c = 0;
    }
    ctx.b[ctx.c++] = input[i];
  }
  ctx.t += ctx.c;
  while (ctx.c < 128) ctx.b[ctx.c++] = 0;
  compress(ctx, true);
  const out = new Uint8Array(outlen);
  for (let i = 0; i < outlen; i++) out[i] = ctx.h[i >> 2] >> (8 * (i & 3));
  return out;
}

function le32(n) {
  return new Uint8Array([n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255]);
}

function hPrime(len, input) {
  const x = concat(le32(len), input);
  if (len <= 64) return blake2b(x, len);
  const out = new Uint8Array(len);
  const r = Math.ceil(len / 32) - 2;
  let v = blake2b(x, 64);
  out.set(v.subarray(0, 32), 0);
  for (let i = 1; i < r; i++) {
    v = blake2b(v, 64);
    out.set(v.subarray(0, 32), i * 32);
  }
  v = blake2b(v, len - 32 * r);
  out.set(v, r * 32);
  return out;
}

function mul32(a, b) {
  const al = a & 0xffff, ah = a >>> 16, bl = b & 0xffff, bh = b >>> 16;
  const ll = al * bl, lh = al * bh, hl = ah * bl, hh = ah * bh;
  const mid = (ll >>> 16) + (lh & 0xffff) + (hl & 0xffff);
  const lo = ((mid & 0xffff) << 16) | (ll & 0xffff);
  const hi = hh + (lh >>> 16) + (hl >>> 16) + (mid >>> 16);
  return [lo >>> 0, hi >>> 0];
}

function blamka(v, a, b) {
  const [pl, ph] = mul32(v[a], v[b]);
  const dl = (pl << 1) >>> 0;
  const dh = ((ph << 1) | (pl >>> 31)) >>> 0;
  let lo = v[a] + v[b];
  let hi = v[a + 1] + v[b + 1] + (lo >= 0x100000000 ? 1 : 0);
  lo >>>= 0;
  const lo2 = lo + dl;
  hi = (hi + dh + (lo2 >= 0x100000000 ? 1 : 0)) >>> 0;
  v[a] = lo2 >>> 0;
  v[a + 1] = hi;
}

function rotr(v, i, n) {
  const lo = v[i], hi = v[i + 1];
  if (n === 32) {
    v[i] = hi;
    v[i + 1] = lo;
  } else if (n < 32) {
    v[i] = (lo >>> n) | (hi << (32 - n));
    v[i + 1] = (hi >>> n) | (lo << (32 - n));
  } else {
    const m = n - 32;
    v[i] = (hi >>> m) | (lo << (32 - m));
    v[i + 1] = (lo >>> m) | (hi << (32 - m));
  }
}

function gb(v, a, b, c, d) {
  blamka(v, a, b);
  v[d] ^= v[a];
  v[d + 1] ^= v[a + 1];
  rotr(v, d, 32);
  blamka(v, c, d);
  v[b] ^= v[c];
  v[b + 1] ^= v[c + 1];
  rotr(v, b, 24);
  blamka(v, a, b);
  v[d] ^= v[a];
  v[d + 1] ^= v[a + 1];
  rotr(v, d, 16);
  blamka(v, c, d);
  v[b] ^= v[c];
  v[b + 1] ^= v[c + 1];
  rotr(v, b, 63);
}

const R = new Uint32Array(256);
const Q = new Uint32Array(256);
const P16 = new Uint32Array(32);

function permute(q, idx) {
  for (let k = 0; k < 16; k++) {
    P16[2 * k] = q[2 * idx[k]];
    P16[2 * k + 1] = q[2 * idx[k] + 1];
  }
  gb(P16, 0, 8, 16, 24);
  gb(P16, 2, 10, 18, 26);
  gb(P16, 4, 12, 20, 28);
  gb(P16, 6, 14, 22, 30);
  gb(P16, 0, 10, 20, 30);
  gb(P16, 2, 12, 22, 24);
  gb(P16, 4, 14, 16, 26);
  gb(P16, 6, 8, 18, 28);
  for (let k = 0; k < 16; k++) {
    q[2 * idx[k]] = P16[2 * k];
    q[2 * idx[k] + 1] = P16[2 * k + 1];
  }
}

const ROWS = [];
const COLS = [];
for (let i = 0; i < 8; i++) {
  ROWS.push(Array.from({ length: 16 }, (_, k) => 16 * i + k));
  const col = [];
  for (let r = 0; r < 8; r++) col.push(2 * (8 * r + i), 2 * (8 * r + i) + 1);
  COLS.push(col);
}

function compressG(mem, x, y, out, xor) {
  for (let i = 0; i < 256; i++) R[i] = mem[x + i] ^ mem[y + i];
  Q.set(R);
  for (const row of ROWS) permute(Q, row);
  for (const col of COLS) permute(Q, col);
  if (xor) {
    for (let i = 0; i < 256; i++) mem[out + i] ^= Q[i] ^ R[i];
  } else {
    for (let i = 0; i < 256; i++) mem[out + i] = Q[i] ^ R[i];
  }
}

function blockToWords(bytes, mem, at) {
  for (let i = 0; i < 256; i++) {
    const o = 4 * i;
    mem[at + i] = (bytes[o] | (bytes[o + 1] << 8) | (bytes[o + 2] << 16) | (bytes[o + 3] << 24)) >>> 0;
  }
}

export function argon2id(password, salt, { m, t, p, len = 32, secret = new Uint8Array(0), ad = new Uint8Array(0) }) {
  const h0 = blake2b(
    concat(
      le32(p), le32(len), le32(m), le32(t), le32(0x13), le32(2),
      le32(password.length), password, le32(salt.length), salt,
      le32(secret.length), secret, le32(ad.length), ad,
    ),
    64,
  );
  const mBlocks = 4 * p * Math.floor(m / (4 * p));
  const q = mBlocks / p;
  const sl = q / 4;
  const mem = new Uint32Array(mBlocks * 256);
  const at = (lane, col) => (lane * q + col) * 256;
  for (let l = 0; l < p; l++) {
    blockToWords(hPrime(1024, concat(h0, le32(0), le32(l))), mem, at(l, 0));
    blockToWords(hPrime(1024, concat(h0, le32(1), le32(l))), mem, at(l, 1));
  }
  const zero = new Uint32Array(256);
  const addr = new Uint32Array(256 * 3);
  for (let pass = 0; pass < t; pass++) {
    for (let slice = 0; slice < 4; slice++) {
      for (let lane = 0; lane < p; lane++) {
        const independent = pass === 0 && slice < 2;
        let counter = 0;
        const nextAddresses = () => {
          counter++;
          addr.fill(0);
          addr.set(zero, 0);
          const inp = 256;
          addr[inp + 0] = pass;
          addr[inp + 2] = lane;
          addr[inp + 4] = slice;
          addr[inp + 6] = mBlocks;
          addr[inp + 8] = t;
          addr[inp + 10] = 2;
          addr[inp + 12] = counter;
          compressG(addr, 0, inp, 512, false);
          compressG(addr, 0, 512, 512, false);
        };
        const start = pass === 0 && slice === 0 ? 2 : 0;
        if (independent) nextAddresses();
        for (let idx = start; idx < sl; idx++) {
          const col = slice * sl + idx;
          const prevCol = col === 0 ? q - 1 : col - 1;
          let j1, j2;
          if (independent) {
            if (idx % 128 === 0 && idx !== 0) nextAddresses();
            const w = 512 + 2 * (idx % 128);
            j1 = addr[w];
            j2 = addr[w + 1];
          } else {
            const w = at(lane, prevCol);
            j1 = mem[w];
            j2 = mem[w + 1];
          }
          const refLane = pass === 0 && slice === 0 ? lane : j2 % p;
          let area;
          if (pass === 0) {
            area = refLane === lane ? slice * sl + idx - 1 : slice * sl - (idx === 0 ? 1 : 0);
          } else {
            area = refLane === lane ? q - sl + idx - 1 : q - sl - (idx === 0 ? 1 : 0);
          }
          const J1 = BigInt(j1);
          const x = (J1 * J1) >> 32n;
          const y = (BigInt(area) * x) >> 32n;
          const rel = area - 1 - Number(y);
          const startPos = pass === 0 || slice === 3 ? 0 : (slice + 1) * sl;
          const refCol = (startPos + rel) % q;
          compressG(mem, at(lane, prevCol), at(refLane, refCol), at(lane, col), pass > 0);
        }
      }
    }
  }
  const final = new Uint32Array(256);
  final.set(mem.subarray(at(0, q - 1), at(0, q - 1) + 256));
  for (let l = 1; l < p; l++) {
    const b = at(l, q - 1);
    for (let i = 0; i < 256; i++) final[i] ^= mem[b + i];
  }
  const bytes = new Uint8Array(1024);
  for (let i = 0; i < 256; i++) {
    bytes[4 * i] = final[i] & 255;
    bytes[4 * i + 1] = (final[i] >>> 8) & 255;
    bytes[4 * i + 2] = (final[i] >>> 16) & 255;
    bytes[4 * i + 3] = (final[i] >>> 24) & 255;
  }
  return hPrime(len, bytes);
}

export const SECRET_LABEL = enc.encode("shockingvrc/2/secret");
const LINK_SIG_LABEL = enc.encode("shockingvrc/2/link");
const CODE_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function normalizeCode(text) {
  const s = [...text]
    .filter((c) => !/\s|-/.test(c))
    .map((c) => {
      const u = c.toUpperCase();
      return u === "O" ? "0" : u === "I" || u === "L" ? "1" : u === "U" ? "V" : u;
    })
    .join("");
  return s.length === 16 && [...s].every((c) => CODE_ALPHABET.includes(c)) ? s : null;
}

function linkBlob(text) {
  let s = text.trim();
  if (!/^shockingvrc:/i.test(s)) return null;
  s = s.replace(/^shockingvrc:\/*/i, "");
  const [ver, data] = s.split("/");
  if (ver !== "2" || !data) throw new Error("this link needs a newer version");
  return unb64(data);
}

export function isLink(text) {
  return /^\s*shockingvrc:/i.test(text);
}

export function linkNeedsPassphrase(text) {
  try {
    const blob = linkBlob(text);
    return !!blob && blob[1] === 1;
  } catch {
    return false;
  }
}

export async function decodeLink(text, passphrase, onProgress) {
  const blob = linkBlob(text);
  if (!blob || blob[0] !== 2) throw new Error("not a ShockingVRC link");
  let key, headerLen;
  if (blob[1] === 0) {
    key = blob.subarray(2, 34);
    headerLen = 34;
  } else if (blob[1] === 1) {
    if (!passphrase) throw new Error("this link needs its passphrase");
    const m = ((blob[2] << 24) | (blob[3] << 16) | (blob[4] << 8) | blob[5]) >>> 0;
    const t = blob[6], p = blob[7];
    if (m > 262144 || t > 16 || p === 0 || p > 8) throw new Error("the link is damaged");
    onProgress?.("deriving the key from your passphrase…");
    await new Promise((r) => setTimeout(r, 30));
    key = argon2id(enc.encode(passphrase), blob.subarray(8, 24), { m, t, p, len: 32 });
    headerLen = 24;
  } else {
    throw new Error("the link is damaged");
  }
  const total = headerLen + 12;
  const aesKey = await subtle.importKey("raw", key, "AES-GCM", false, ["decrypt"]);
  let plain;
  try {
    plain = await subtle.decrypt(
      { name: "AES-GCM", iv: blob.subarray(headerLen, total), additionalData: blob.subarray(0, total), tagLength: 128 },
      aesKey,
      blob.subarray(total),
    );
  } catch {
    throw new Error("wrong passphrase, or the link was changed");
  }
  const signed = JSON.parse(dec.decode(plain));
  const body = JSON.parse(signed.body);
  const verified = await verifyEd25519(unb64(body.pk), concat(LINK_SIG_LABEL, enc.encode(signed.body)), unb64(signed.sig));
  if (verified === false) throw new Error("the link's signature does not match its sender");
  if (body.exp && Date.now() / 1000 > body.exp) throw new Error("this link has expired");
  return { body, signatureChecked: verified === true };
}

export function sdpFingerprints(sdp) {
  const out = sdp
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.startsWith("a=fingerprint:"))
    .map((l) => l.slice("a=fingerprint:".length).trim().toUpperCase());
  return [...new Set(out)].sort();
}

export async function dtlsBinding(localSdp, remoteSdp) {
  const all = [...new Set([...sdpFingerprints(localSdp), ...sdpFingerprints(remoteSdp)])].sort((a, b) =>
    a < b ? -1 : a > b ? 1 : 0,
  );
  return sha256(enc.encode(all.join("|")));
}

function put(parts, bytes) {
  parts.push(u32be(bytes.length), bytes);
}

export async function transcript({ binding, joinerNonce, listenerNonce, listenerKey, joinerKey, grant }) {
  const parts = [];
  put(parts, enc.encode("shockingvrc/2/handshake"));
  for (const p of [binding, joinerNonce, listenerNonce, listenerKey, joinerKey, enc.encode(grant)]) put(parts, p);
  return sha256(concat(...parts));
}

export async function join(pipe, { name, binding, grant, onPending }) {
  const nonce = random(32);
  const hello = { t: "hello", name, client: "web", invite: grant.id, nonce: b64(nonce) };
  pipe.send(encode(hello));
  const ch = await pipe.next();
  if (ch.t === "reject") throw new Error(`refused: ${ch.reason}`);
  if (ch.t !== "challenge") throw new Error(`unexpected ${ch.t}`);
  const listenerKey = unb64(ch.identity);
  if (grant.expect && !ctEqual(listenerKey, grant.expect)) throw new Error("the partner is not who the link says");
  const t = await transcript({
    binding,
    joinerNonce: nonce,
    listenerNonce: unb64(ch.nonce),
    listenerKey,
    joinerKey: new Uint8Array(0),
    grant: `invite:${grant.id}`,
  });
  const key = await sha256(SECRET_LABEL, grant.secret);
  const theirMac = ch.mac ? unb64(ch.mac) : new Uint8Array(0);
  if (!ctEqual(theirMac, await hmac(key, enc.encode("L"), t))) throw new Error("the partner's proof did not check out");
  const sigOk = await verifyEd25519(listenerKey, concat(enc.encode("L"), t), unb64(ch.sig));
  if (sigOk === false) throw new Error("the partner's signature did not check out");
  pipe.send(encode({ t: "proof", mac: b64(await hmac(key, enc.encode("D"), t)) }));
  for (;;) {
    const m = await pipe.next();
    if (m.t === "welcome") return { welcome: m, partner: ch.name, fingerprintChecked: sigOk === true };
    if (m.t === "pending") {
      onPending?.(m.reason);
      continue;
    }
    if (m.t === "ping" || m.t === "pong") continue;
    if (m.t === "reject") throw new Error(`refused: ${m.reason}`);
    throw new Error(`unexpected ${m.t}`);
  }
}

export function rawFromHz(hz) {
  const h = Number.isFinite(hz) ? Math.min(100, Math.max(1, hz)) : 100;
  const ms = 1000 / h;
  let raw;
  if (ms <= 100) raw = ms;
  else if (ms <= 600) raw = (ms - 100) / 5 + 100;
  else raw = (ms - 600) / 10 + 200;
  return Math.round(Math.min(240, Math.max(10, raw)));
}
