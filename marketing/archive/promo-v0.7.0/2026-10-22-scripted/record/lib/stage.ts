/**
 * The stage a shot is recorded on: the real app in Chromium at 1600x1000 CSS
 * pixels and a device scale of 2, the user's hands, and the camera.
 *
 * Frame-stepped recording (see `kit.ts` for why): after the app has booted
 * the page's clock is stopped. Everything a shot does then takes page time,
 * and page time only passes in `tick()`, one 1/30 s frame at a time:
 *
 *   tick = advance the page's clock, animations and media by 1/30 s,
 *          let the app react, take a picture (3200x2000 JPEG, quality 95).
 *
 * `wait(ms)` is a number of ticks; typing, moving the pointer and scrolling
 * are loops of an action and a tick. The frames are 30 fps by construction, so
 * the footage needs no timestamps and no duplicated or dropped frames.
 *
 * A picture of a page that did not change costs a full 2x capture for
 * nothing, so after a frame that repeated the one before, the next ticks take
 * a cheap 1x look first and take the 2x picture only if the page differs.
 *
 * The encode is H.264 High, yuv420p, CRF 16 by default, 30 fps, no audio.
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import type { App } from "../../../../apps/desktop/e2e/fixtures";

import { installCursor, sleep } from "./human";
import { installKit, syncPage } from "./kit";
import { playedSpans, readMedia, type PlayedSpan } from "./media";
import { FOOTAGE_DIR, RECORD_DIR } from "./paths";

export const FPS = 30;
export const TICK_MS = 1000 / FPS;
export const VIEWPORT = { width: 1600, height: 1000 } as const;
export const SCALE = 2;

type Page = App["page"];
type Locator = ReturnType<Page["locator"]>;
interface CdpSession {
  send(method: string, params?: object): Promise<unknown>;
  detach(): Promise<void>;
}

export interface Mark {
  label: string;
  /** Seconds into the footage. */
  t: number;
}

export interface Finished {
  file: string;
  seconds: number;
  frames: number;
  /** How many of the frames were a 2x capture of a page that had changed. */
  captured: number;
  marks: Mark[];
  /** Spans of sound the app played, on the footage's clock. */
  spans: PlayedSpan[];
}

const easeInOut = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

export class Stage {
  readonly page: Page;
  readonly app: App;

  private cdp!: CdpSession;
  private name = "";
  private work = "";
  /** The picture for each frame: a file name, repeated while the page held still. */
  private frames: string[] = [];
  private lastFile = "";
  private lastShotHash = "";
  private lastProbeHash = "";
  private probeMode = false;
  private captured = 0;
  private targetMs = 0;
  /** Page time moved since the first frame, in whole ms. */
  private clockMs = 0;
  private marks: Mark[] = [];
  private pointer = { x: 1100, y: 700 };
  private originEpochMs = 0;
  private recording = false;

  private constructor(app: App) {
    this.app = app;
    this.page = app.page;
  }

  /** Install the clock, the kit and the pointer. Call before `app.boot`. */
  static async prepare(app: App): Promise<Stage> {
    const stage = new Stage(app);
    await installKit(app.page);
    await installCursor(app.page);
    return stage;
  }

  /**
   * Stop the page's clock and take the first frame. The app should be booted
   * and showing the state the shot starts from.
   */
  async start(name: string): Promise<void> {
    this.name = name;
    this.work = join(RECORD_DIR, ".out", `frames-${name}`);
    rmSync(this.work, { recursive: true, force: true });
    mkdirSync(this.work, { recursive: true });
    mkdirSync(FOOTAGE_DIR, { recursive: true });

    const now = await this.page.evaluate(() => Date.now());
    await this.page.clock.pauseAt(now + 20);
    await syncPage(this.page);
    this.originEpochMs = await this.page.evaluate(() => performance.timeOrigin + performance.now());
    this.cdp = (await this.page.context().newCDPSession(this.page)) as unknown as CdpSession;
    this.recording = true;
    await this.capture(true);
  }

  /** Seconds of footage so far. */
  get elapsed(): number {
    return this.frames.length / FPS;
  }

  /** Note a moment ("play pressed") for the README and the edit. */
  mark(label: string): void {
    this.marks.push({ label, t: Number(this.elapsed.toFixed(2)) });
  }

  // -- time ---------------------------------------------------------------

  /**
   * One frame: advance the page by 1/30 s, then take its picture. The clock
   * moves in whole milliseconds (33, 33, 34, ...), so that after k frames it
   * has moved by exactly k/30 s, rounded; advancing by 33.33 each time would
   * be rounded up to 34 by the fake clock and drift 2% fast.
   */
  async tick(): Promise<void> {
    const to = Math.round((this.frames.length * 1000) / FPS);
    await this.page.clock.runFor(to - this.clockMs);
    this.clockMs = to;
    await syncPage(this.page);
    // Let what the app scheduled for itself (React's own work, a decode) run.
    await sleep(14);
    await this.capture(false);
    // A frame made on its own (a pan, a drag) is time that has passed, so a
    // `wait` after it counts from here and not from before it.
    this.targetMs = Math.max(this.targetMs, this.frames.length * TICK_MS);
  }

  /** Let `ms` of page time pass. */
  async wait(ms: number): Promise<void> {
    this.targetMs += ms;
    while (this.frames.length * TICK_MS < this.targetMs - TICK_MS / 2) await this.tick();
  }

  /** Tick until `done()` is true, or `maxMs` of page time has passed. */
  async until(done: () => Promise<boolean>, maxMs = 8000): Promise<void> {
    const limit = this.targetMs + maxMs;
    while (!(await done())) {
      if (this.frames.length * TICK_MS > limit) throw new Error("the page did not get there in time");
      await this.tick();
    }
  }

  // -- the camera -----------------------------------------------------------

  private async shot(scale: number, quality: number): Promise<string> {
    const { data } = (await this.cdp.send("Page.captureScreenshot", {
      format: "jpeg",
      quality,
      optimizeForSpeed: true,
      fromSurface: true,
      clip: { x: 0, y: 0, width: VIEWPORT.width, height: VIEWPORT.height, scale },
    })) as { data: string };
    return data;
  }

  private async capture(first: boolean): Promise<void> {
    if (this.probeMode && !first) {
      const probe = await this.shot(1, 70);
      const h = createHash("md5").update(probe).digest("hex");
      if (h === this.lastProbeHash) {
        this.frames.push(this.lastFile);
        return;
      }
      this.lastProbeHash = h;
    }
    const data = await this.shot(SCALE, 95);
    const h = createHash("md5").update(data).digest("hex");
    if (h === this.lastShotHash && this.lastFile) {
      this.frames.push(this.lastFile);
      // Still: look cheaply from now on.
      if (!this.probeMode) {
        this.probeMode = true;
        this.lastProbeHash = createHash("md5").update(await this.shot(1, 70)).digest("hex");
      }
      return;
    }
    this.lastShotHash = h;
    this.probeMode = false;
    const file = `f${String(this.captured).padStart(6, "0")}.jpg`;
    this.captured++;
    writeFileSync(join(this.work, file), Buffer.from(data, "base64"));
    this.lastFile = file;
    this.frames.push(file);
  }

  // -- the user's hands ---------------------------------------------------------

  /** Move the pointer to (x, y) over `ms` of page time, easing in and out. */
  async moveTo(x: number, y: number, ms?: number): Promise<void> {
    const from = this.pointer;
    const dist = Math.hypot(x - from.x, y - from.y);
    const duration = ms ?? Math.min(900, 260 + dist * 0.9);
    const steps = Math.max(1, Math.round(duration / TICK_MS));
    for (let i = 1; i <= steps; i++) {
      const e = easeInOut(i / steps);
      await this.page.mouse.move(from.x + (x - from.x) * e, from.y + (y - from.y) * e);
      await this.advanceOne();
    }
    this.pointer = { x, y };
  }

  /** One frame, counted as time passed. */
  private async advanceOne(): Promise<void> {
    await this.tick();
  }

  private async aim(target: Locator, at?: { x?: number; y?: number }) {
    const box = await target.boundingBox();
    if (!box) throw new Error("the element to point at has no box");
    return { x: box.x + box.width * (at?.x ?? 0.5), y: box.y + box.height * (at?.y ?? 0.5) };
  }

  /** Move to `target` and click it, with the hold a finger makes. */
  async click(target: Locator, opts: { at?: { x?: number; y?: number }; moveMs?: number; hold?: number } = {}): Promise<void> {
    await target.scrollIntoViewIfNeeded();
    const p = await this.aim(target, opts.at);
    await this.moveTo(p.x, p.y, opts.moveMs);
    await this.wait(120);
    await this.page.mouse.down();
    await this.wait(opts.hold ?? 90);
    await this.page.mouse.up();
  }

  /** Move to `target` and rest there. */
  async hover(target: Locator, opts: { at?: { x?: number; y?: number }; moveMs?: number } = {}): Promise<void> {
    const p = await this.aim(target, opts.at);
    await this.moveTo(p.x, p.y, opts.moveMs);
  }

  /**
   * Type `text` with the key-to-key times of someone typing a sentence they
   * know: about 70 ms a key with jitter, a little longer after a space and a
   * breath after a comma. `perKey` scales it (1 is that pace).
   */
  async type(text: string, perKey = 1): Promise<void> {
    let seed = 12345;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    for (const ch of text) {
      await this.page.keyboard.type(ch);
      let d = 52 + rnd() * 46;
      if (ch === " ") d += 20;
      if (ch === "," || ch === ".") d += 130;
      await this.wait(d * perKey);
    }
  }

  /** Press a key (down, a short hold, up). */
  async press(key: string, hold = 90): Promise<void> {
    await this.page.keyboard.down(key);
    await this.wait(hold);
    await this.page.keyboard.up(key);
  }

  /** Scroll with the wheel at `over` by `dy` pixels (and `dx`) across `ms`, easing in and out. */
  async wheel(over: { x: number; y: number }, dy: number, ms: number, dx = 0): Promise<void> {
    await this.moveTo(over.x, over.y);
    const steps = Math.max(1, Math.round(ms / TICK_MS));
    let sentY = 0;
    let sentX = 0;
    for (let i = 1; i <= steps; i++) {
      const e = easeInOut(i / steps);
      const sy = dy * e - sentY;
      const sx = dx * e - sentX;
      if (Math.abs(sy) >= 0.5 || Math.abs(sx) >= 0.5) {
        await this.page.mouse.wheel(sx, sy);
        sentY += sy;
        sentX += sx;
      }
      await this.advanceOne();
    }
  }

  // -- the end ------------------------------------------------------------------------

  /** Stop, encode `footage/<name>.mp4`, and write `<name>.marks.json`. */
  async finish(): Promise<Finished> {
    this.recording = false;
    const events = await readMedia(this.page);
    const seconds = this.frames.length / FPS;
    const spans = playedSpans(events, this.originEpochMs, this.originEpochMs + seconds * 1000);
    await this.cdp.detach().catch(() => {});
    await this.page.clock.resume();

    const list: string[] = [];
    for (const f of this.frames) list.push(`file '${join(this.work, f)}'`, `duration ${(1 / FPS).toFixed(6)}`);
    list.push(`file '${join(this.work, this.frames[this.frames.length - 1])}'`);
    const listFile = join(this.work, "frames.txt");
    writeFileSync(listFile, list.join("\n") + "\n");

    const out = join(FOOTAGE_DIR, `${this.name}.mp4`);
    const r = spawnSync(
      "ffmpeg",
      [
        "-y", "-v", "error",
        "-f", "concat", "-safe", "0", "-i", listFile,
        "-vf", `fps=${FPS},format=yuv420p`,
        "-r", String(FPS),
        "-c:v", "libx264", "-profile:v", "high", "-preset", process.env.PROMO_PRESET ?? "slow",
        "-crf", process.env.PROMO_CRF ?? "16",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        "-an",
        out,
      ],
      { encoding: "utf8" },
    );
    if (r.status !== 0) throw new Error(`ffmpeg failed for ${this.name}: ${r.stderr}`);
    writeFileSync(join(FOOTAGE_DIR, `${this.name}.marks.json`), JSON.stringify(this.marks, null, 2) + "\n");
    rmSync(this.work, { recursive: true, force: true });
    return { file: out, seconds, frames: this.frames.length, captured: this.captured, marks: this.marks, spans };
  }

  /** Throw the frames away (a shot that failed). */
  async discard(): Promise<void> {
    this.recording = false;
    rmSync(this.work, { recursive: true, force: true });
  }

  get isRecording(): boolean {
    return this.recording;
  }
}
