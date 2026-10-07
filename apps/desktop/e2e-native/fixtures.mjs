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
