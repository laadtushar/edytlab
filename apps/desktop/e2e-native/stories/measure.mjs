// Measuring audio files from node, so a story can check what an edit did to
// the sound itself (its loudness, pitch, brightness, level swing, direction)
// instead of what the model said it did. Pure functions over WAV files and
// the headers of FLAC and MP3 files; nothing here talks to the app.
import { readFileSync } from "node:fs";

/** A WAV file as one Float32Array per channel. Reads 16/24/32-bit integer
 * and 32/64-bit float PCM, plain or WAVE_FORMAT_EXTENSIBLE. */
export function readWav(file) {
  const b = readFileSync(file);
  if (b.length < 44 || b.toString("latin1", 0, 4) !== "RIFF" || b.toString("latin1", 8, 12) !== "WAVE") {
    throw new Error(`${file} is not a WAV file`);
  }
  let tag = 0;
  let channels = 0;
  let rate = 0;
  let bits = 0;
  let dataAt = -1;
  let dataLen = 0;
  for (let at = 12; at + 8 <= b.length; ) {
    const id = b.toString("latin1", at, at + 4);
    const size = b.readUInt32LE(at + 4);
    if (id === "fmt ") {
      tag = b.readUInt16LE(at + 8);
      channels = b.readUInt16LE(at + 10);
      rate = b.readUInt32LE(at + 12);
      bits = b.readUInt16LE(at + 22);
      if (tag === 0xfffe && size >= 26) tag = b.readUInt16LE(at + 8 + 24);
    } else if (id === "data") {
      dataAt = at + 8;
      // A streamed file may carry a size that is not the real one.
      dataLen = Math.min(size, b.length - dataAt);
      break;
    }
    at += 8 + size + (size & 1);
  }
  if (dataAt < 0 || !channels || !rate) throw new Error(`${file}: no fmt or data chunk`);
  const bytes = bits / 8;
  const frames = Math.floor(dataLen / (bytes * channels));
  const out = Array.from({ length: channels }, () => new Float32Array(frames));
  for (let i = 0; i < frames; i++) {
    for (let c = 0; c < channels; c++) {
      const p = dataAt + (i * channels + c) * bytes;
      let v;
      if (tag === 3) v = bits === 64 ? b.readDoubleLE(p) : b.readFloatLE(p);
      else if (bits === 16) v = b.readInt16LE(p) / 32768;
      else if (bits === 24) v = b.readIntLE(p, 3) / 8388608;
      else if (bits === 32) v = b.readInt32LE(p) / 2147483648;
      else throw new Error(`${file}: ${bits}-bit PCM is not handled`);
      out[c][i] = v;
    }
  }
  return { rate, channels: out, frames, seconds: frames / rate };
}

/** The channels averaged into one. */
export function mono(wav) {
  if (wav.channels.length === 1) return wav.channels[0];
  const out = new Float32Array(wav.frames);
  for (const ch of wav.channels) for (let i = 0; i < wav.frames; i++) out[i] += ch[i] / wav.channels.length;
  return out;
}

function biquad(x, [b0, b1, b2], [a1, a2]) {
  const y = new Float64Array(x.length);
  let x1 = 0;
  let x2 = 0;
  let y1 = 0;
  let y2 = 0;
  for (let i = 0; i < x.length; i++) {
    const v = b0 * x[i] + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    x2 = x1;
    x1 = x[i];
    y2 = y1;
    y1 = v;
    y[i] = v;
  }
  return y;
}

/** ITU-R BS.1770-4 K-weighting: a high shelf, then a high-pass. */
function kWeighted(x, fs) {
  let f0 = 1681.974450955533;
  let G = 3.999843853973347;
  let Q = 0.7071752369554196;
  let K = Math.tan((Math.PI * f0) / fs);
  const Vh = 10 ** (G / 20);
  const Vb = Vh ** 0.4996667741545416;
  let a0 = 1 + K / Q + K * K;
  const shelf = biquad(
    x,
    [(Vh + (Vb * K) / Q + K * K) / a0, (2 * (K * K - Vh)) / a0, (Vh - (Vb * K) / Q + K * K) / a0],
    [(2 * (K * K - 1)) / a0, (1 - K / Q + K * K) / a0],
  );
  f0 = 38.13547087602444;
  Q = 0.5003270373238773;
  K = Math.tan((Math.PI * f0) / fs);
  a0 = 1 + K / Q + K * K;
  return biquad(shelf, [1, -2, 1], [(2 * (K * K - 1)) / a0, (1 - K / Q + K * K) / a0]);
}

/** Integrated loudness in LUFS (EBU R128 / BS.1770-4): 400 ms blocks every
 * 100 ms, an absolute gate at -70 LUFS and a relative gate 10 LU below. For
 * mono and stereo, where every channel has weight 1. */
export function lufs(wav) {
  const fs = wav.rate;
  const sq = wav.channels.map((ch) => {
    const k = kWeighted(ch, fs);
    const sums = new Float64Array(k.length + 1);
    for (let i = 0; i < k.length; i++) sums[i + 1] = sums[i] + k[i] * k[i];
    return sums;
  });
  const block = Math.round(0.4 * fs);
  const step = Math.round(0.1 * fs);
  const lk = (z) => -0.691 + 10 * Math.log10(z);
  const blocks = [];
  for (let s = 0; s + block <= wav.frames; s += step) {
    let z = 0;
    for (const sums of sq) z += (sums[s + block] - sums[s]) / block;
    blocks.push(z);
  }
  const loud = blocks.filter((z) => z > 0 && lk(z) > -70);
  if (!loud.length) return -Infinity;
  const mean = (a) => a.reduce((t, z) => t + z, 0) / a.length;
  const gate = lk(mean(loud)) - 10;
  const kept = loud.filter((z) => lk(z) > gate);
  return lk(mean(kept));
}

/** The frequency in [lo, hi] Hz with the most energy, from a one-second
 * Hann-windowed slice of the middle of `x`, to a fraction of a Hz. */
export function peakHz(x, rate, lo, hi, step = 0.5) {
  const n = Math.min(x.length, rate);
  const from = Math.floor((x.length - n) / 2);
  const w = new Float64Array(n);
  for (let i = 0; i < n; i++) w[i] = x[from + i] * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (n - 1)));
  const freqs = [];
  const mags = [];
  for (let f = lo; f <= hi; f += step) {
    const dr = Math.cos((2 * Math.PI * f) / rate);
    const di = Math.sin((2 * Math.PI * f) / rate);
    let cr = 1;
    let ci = 0;
    let re = 0;
    let im = 0;
    for (let i = 0; i < n; i++) {
      re += w[i] * cr;
      im -= w[i] * ci;
      const t = cr * dr - ci * di;
      ci = cr * di + ci * dr;
      cr = t;
    }
    freqs.push(f);
    mags.push(Math.hypot(re, im));
  }
  let k = 0;
  for (let i = 1; i < mags.length; i++) if (mags[i] > mags[k]) k = i;
  if (k === 0 || k === mags.length - 1) return freqs[k];
  // A parabola through the peak and its two neighbours, on a dB scale.
  const [a, b, c] = [mags[k - 1], mags[k], mags[k + 1]].map((m) => 20 * Math.log10(m + 1e-12));
  const denom = a - 2 * b + c;
  return freqs[k] + (denom === 0 ? 0 : (0.5 * (a - c)) / denom) * step;
}

/** In-place radix-2 FFT. */
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang);
    const wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k;
        const b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci;
        const ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr;
        im[b] = im[a] - ti;
        re[a] += tr;
        im[a] += ti;
        const t = cr * wr - ci * wi;
        ci = cr * wi + ci * wr;
        cr = t;
      }
    }
  }
}

/** How bright the sound is: the level of the 2.8 to 11.2 kHz octaves (4 and
 * 8 kHz) minus the level of the 0.7 to 2.8 kHz ones (1 and 2 kHz), in dB, from
 * the average spectrum of the parts that are not silence. Higher is brighter.
 * A boost anywhere in the top two octaves raises it, a gain change or a
 * compressor does not move it. */
export function tiltDb(x, rate) {
  const size = 4096;
  const hop = size / 2;
  const win = Float64Array.from({ length: size }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / size));
  const frames = [];
  for (let s = 0; s + size <= x.length; s += hop) {
    let e = 0;
    for (let i = 0; i < size; i++) e += x[s + i] * x[s + i];
    frames.push({ s, e });
  }
  const top = Math.max(...frames.map((f) => f.e), 1e-20);
  const power = new Float64Array(size / 2);
  let used = 0;
  for (const { s, e } of frames) {
    // Silence and the tails of syllables would only add the noise floor.
    if (e < top * 1e-3) continue;
    const re = Float64Array.from(win, (w, i) => w * x[s + i]);
    const im = new Float64Array(size);
    fft(re, im);
    for (let k = 0; k < size / 2; k++) power[k] += re[k] * re[k] + im[k] * im[k];
    used++;
  }
  if (!used) return -Infinity;
  const band = (lo, hi) => {
    let sum = 0;
    for (let k = Math.ceil((lo * size) / rate); k < Math.min(size / 2, (hi * size) / rate); k++) sum += power[k];
    return 10 * Math.log10(sum / used + 1e-20);
  };
  return (band(2800, 5600) + band(5600, 11200)) / 2 - (band(700, 1400) + band(1400, 2800)) / 2;
}

/** How far the level swings, in dB: the 90th minus the 10th percentile of
 * the 50 ms RMS of the parts that are not silence. A compressor or a
 * leveler lowers it; a gain change does not move it. */
export function levelSpreadDb(x, rate, windowSec = 0.05) {
  const win = Math.round(windowSec * rate);
  const levels = [];
  for (let s = 0; s + win <= x.length; s += win) {
    let t = 0;
    for (let i = s; i < s + win; i++) t += x[i] * x[i];
    levels.push(10 * Math.log10(t / win + 1e-20));
  }
  const top = Math.max(...levels);
  const live = levels.filter((l) => l > top - 35).sort((a, b) => a - b);
  const at = (q) => live[Math.floor(q * (live.length - 1))];
  return at(0.9) - at(0.1);
}

/** The greatest sample difference between `b` and `a` read backwards, over
 * every channel; 0 when `b` is `a` reversed. Infinity when they are not the
 * same length, because then no sample lines up. */
export function reversedError(a, b) {
  if (a.frames !== b.frames) return Infinity;
  const n = a.frames;
  let worst = 0;
  const channels = Math.min(a.channels.length, b.channels.length);
  for (let c = 0; c < channels; c++) {
    for (let i = 0; i < n; i++) {
      worst = Math.max(worst, Math.abs(b.channels[c][i] - a.channels[c][a.frames - 1 - i]));
    }
  }
  return worst;
}

/** The RMS level of `wav` between two times, over all its channels. */
export function rmsBetween(wav, fromSec, toSec) {
  const from = Math.max(0, Math.round(fromSec * wav.rate));
  const to = Math.min(wav.frames, Math.round(toSec * wav.rate));
  let sum = 0;
  for (const ch of wav.channels) for (let i = from; i < to; i++) sum += ch[i] * ch[i];
  const n = (to - from) * wav.channels.length;
  return n > 0 ? Math.sqrt(sum / n) : 0;
}

/** What a FLAC file's STREAMINFO block says, or null if it is not FLAC.
 * `samples` is 0 when the encoder did not record a length. */
export function flacInfo(file) {
  const b = readFileSync(file);
  if (b.length < 42 || b.toString("latin1", 0, 4) !== "fLaC") return null;
  // After "fLaC": a 4-byte block header, then STREAMINFO. The rate (20 bits),
  // channels (3), bits per sample (5) and total samples (36) follow 10 bytes
  // of block and frame sizes.
  const at = 8 + 10;
  const rate = (b[at] << 12) | (b[at + 1] << 4) | (b[at + 2] >> 4);
  const channels = ((b[at + 2] >> 1) & 7) + 1;
  const samples = (b[at + 3] & 15) * 2 ** 32 + b.readUInt32BE(at + 4);
  return { rate, channels, samples, seconds: rate ? samples / rate : 0, bytes: b.length };
}

/** Whether a file starts like an MP3: an ID3v2 tag, or a frame sync. */
export function looksLikeMp3(file) {
  const b = readFileSync(file);
  if (b.length < 4) return false;
  if (b.toString("latin1", 0, 3) === "ID3") return true;
  return b[0] === 0xff && (b[1] & 0xe0) === 0xe0;
}
