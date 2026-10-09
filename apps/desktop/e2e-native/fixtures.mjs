// Audio the stories load, written as 16-bit PCM WAV at run time.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

function wav({ seconds, rate = 44100, channels = 1, sample }) {
  const frames = Math.round(seconds * rate);
  const data = Buffer.alloc(frames * channels * 2);
  for (let i = 0; i < frames; i++) {
    for (let c = 0; c < channels; c++) {
      const v = Math.max(-1, Math.min(1, sample(i / rate, c)));
      data.writeInt16LE(Math.round(v * 32767), (i * channels + c) * 2);
    }
  }
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(channels, 22);
  h.writeUInt32LE(rate, 24);
  h.writeUInt32LE(rate * channels * 2, 28);
  h.writeUInt16LE(channels * 2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
}

/** A deterministic loop at `bpm`: kick on every beat, a hat on every
 * off-beat, and an eighth-note bass at `bassHz`. */
function loop({ bpm, bassHz }) {
  const beat = 60 / bpm;
  const noise = (t) => {
    const x = Math.sin(t * 12_989.8 + 78.233) * 43_758.5453;
    return (x - Math.floor(x)) * 2 - 1;
  };
  return (t, c) => {
    const inBeat = t % beat;
    const kick = Math.sin(2 * Math.PI * (50 + 90 * Math.exp(-inBeat * 30)) * inBeat) * Math.exp(-inBeat * 9) * 0.9;
    const off = (t + beat / 2) % beat;
    const hat = noise(t) * Math.exp(-off * 60) * 0.25;
    const eighth = t % (beat / 2);
    const bass = Math.sin(2 * Math.PI * bassHz * t) * Math.exp(-eighth * 6) * 0.35;
    return (kick + hat * (c ? 1 : 0.8) + bass) * 0.7;
  };
}

/** A short original dance track at `bpm` in `rootHz`: kick, clap, hats,
 * a bassline, a chord pad and a lead riff, with an 8-bar intro of drums
 * only so a DJ has something to mix over. */
function song({ bpm, rootHz }) {
  const beat = 60 / bpm;
  const bar = beat * 4;
  const noise = (t) => {
    const x = Math.sin(t * 12_989.8 + 78.233) * 43_758.5453;
    return (x - Math.floor(x)) * 2 - 1;
  };
  const chord = [1, 1.2599, 1.4983]; // a minor triad over the root
  const riff = [0, 3, 7, 10, 7, 3, 0, -2];
  return (t, c) => {
    const inBeat = t % beat;
    const barNo = Math.floor(t / bar);
    const kick = Math.sin(2 * Math.PI * (48 + 100 * Math.exp(-inBeat * 32)) * inBeat) * Math.exp(-inBeat * 8) * 0.9;
    const beatNo = Math.floor(t / beat) % 4;
    const clap = beatNo % 2 === 1 ? noise(t * 1.7) * Math.exp(-inBeat * 25) * 0.35 : 0;
    const off = (t + beat / 2) % beat;
    const hat = noise(t) * Math.exp(-off * 70) * 0.18;
    const intro = barNo < 8;
    const eighth = t % (beat / 2);
    const bass = intro ? 0 : Math.sin(2 * Math.PI * (rootHz / 2) * t) * Math.exp(-eighth * 5) * 0.35;
    const pad = intro
      ? 0
      : chord.reduce((a, r) => a + Math.sin(2 * Math.PI * rootHz * 2 * r * t + c * 0.3), 0) * 0.06 * (0.6 + 0.4 * Math.sin((2 * Math.PI * t) / bar));
    const step = Math.floor(t / (beat / 2)) % riff.length;
    const leadHz = rootHz * 4 * 2 ** (riff[step] / 12);
    const lead = barNo >= 12 ? Math.sin(2 * Math.PI * leadHz * t) * Math.exp(-eighth * 9) * 0.12 : 0;
    return (kick + clap + hat * (c ? 1 : 0.85) + bass + pad + lead) * 0.75;
  };
}

export function writeFixtures(dir) {
  mkdirSync(dir, { recursive: true });
  const tone = (hz, amp) => (t) => Math.sin(2 * Math.PI * hz * t) * amp;
  const files = {
    // A plain take.
    tone: ["tone-6s.wav", wav({ seconds: 6, sample: tone(440, 0.5) })],
    // Stereo, with a near-silent gap from 2 to 3 s, for selection and
    // silence stories; the channels differ so a swap would show.
    music: [
      "music-8s-stereo.wav",
      wav({
        seconds: 8,
        channels: 2,
        sample: (t, c) => (t > 2 && t < 3 ? 0.001 : Math.sin(2 * Math.PI * (c ? 330 : 220) * t) * 0.6 * Math.exp(-((t % 1) * 1.5))),
      }),
    ],
    // A second take for multi-track stories.
    take2: ["take2-4s.wav", wav({ seconds: 4, sample: tone(660, 0.4) })],
    // Two dance loops for the DJ story: four-on-the-floor kick, off-beat
    // hats and a bassline, at different tempos and in different keys.
    djA: ["dj-a-120bpm.wav", wav({ seconds: 16, channels: 2, sample: loop({ bpm: 120, bassHz: 110 }) })],
    djB: ["dj-b-128bpm.wav", wav({ seconds: 16, channels: 2, sample: loop({ bpm: 128, bassHz: 98 }) })],
    // Three original tracks for the DJ demos, 32 s (16 bars at 120 BPM).
    midnightDrive: ["midnight-drive-120bpm.wav", wav({ seconds: 32, channels: 2, sample: song({ bpm: 120, rootHz: 110 }) })],
    solarFlare: ["solar-flare-124bpm.wav", wav({ seconds: 32, channels: 2, sample: song({ bpm: 124, rootHz: 98 }) })],
    neonRush: ["neon-rush-128bpm.wav", wav({ seconds: 32, channels: 2, sample: song({ bpm: 128, rootHz: 123.47 }) })],
    // Not audio, for the refusal story.
    notAudio: ["notes.wav", Buffer.from("this is not a wav file\n")],
  };
  const out = {};
  for (const [k, [name, buf]] of Object.entries(files)) {
    const p = join(dir, name);
    writeFileSync(p, buf);
    out[k] = p;
  }
  return out;
}
