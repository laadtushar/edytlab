/**
 * The in-page half of frame-stepped recording.
 *
 * A capture at the device scale of 2 (3200x2000) takes 0.3 to 0.4 s here, so
 * recording in real time would give 3 frames a second, and the number would
 * change with whatever else the machine is doing. So the recording does not
 * run in real time. The page's clock is stopped, and each frame is made by
 * advancing it by exactly 1/30 s and then taking a picture. Time on screen is
 * the page's time, however long the picture took.
 *
 * What "the page's clock" has to cover for the picture to be right:
 *
 *  - JavaScript time: `Date`, timers, `requestAnimationFrame` and
 *    `performance.now`. Playwright's fake clock (`page.clock`) does these.
 *  - CSS and Web Animations (the app's fades, spinners and settles, and the
 *    recorder's own click ring). They run on a clock of their own, so each
 *    tick `sync` pauses every animation it has not seen, and sets its time to
 *    the page time since it was first seen.
 *  - Media playback. The page's audio is an `<audio>` element, which plays in
 *    real time. In a stepped recording it must advance by the page's time, so
 *    the element is kept paused and its `currentTime`, `paused` and `ended`
 *    are computed from the page's clock, with the `play`, `playing`, `pause`
 *    and `ended` events raised as the real element raises them. The element
 *    still loads its file for real.
 *
 * Nothing in the app changes. The app's own code, styles and IPC calls run as
 * built; only the clocks it reads are the stepped ones.
 */

import type { App } from "../../../../apps/desktop/e2e/fixtures";

type Page = App["page"];

/** Call before `app.boot`. Installs the fake clock and the in-page kit. */
export async function installKit(page: Page): Promise<void> {
  await page.clock.install({ time: Date.now() });
  await page.addInitScript(() => {
    type Win = Window & {
      __promoSync: (nowMs: number) => void;
      __promoBusy: () => boolean;
      __PROMO_MEDIA__: { type: string; at: number; position: number }[];
    };
    const w = window as unknown as Win;

    // -- animations ----------------------------------------------------
    const firstSeen = new WeakMap<Animation, number>();
    w.__promoSync = (nowMs: number) => {
      for (const a of document.getAnimations()) {
        let start = firstSeen.get(a);
        if (start === undefined) {
          // Something already over (a one-shot that ended before the kit
          // looked) is left alone.
          if (a.playState === "finished" || a.playState === "idle") continue;
          start = nowMs;
          firstSeen.set(a, start);
          a.pause();
        }
        a.currentTime = nowMs - start;
      }
      // Media advances with the page clock.
      for (const el of playing) {
        const s = state.get(el)!;
        const pos = positionOf(el);
        const dur = Number.isFinite(el.duration) ? el.duration : Infinity;
        if (pos >= dur) {
          s.playing = false;
          s.pos = dur;
          playing.delete(el);
          note("pause", el);
          el.dispatchEvent(new Event("pause"));
          note("ended", el);
          el.dispatchEvent(new Event("ended"));
        } else {
          el.dispatchEvent(new Event("timeupdate"));
        }
      }
    };
    /** Whether anything is moving that a still page would not be doing. */
    w.__promoBusy = () => playing.size > 0 || document.getAnimations().some((a) => a.playState === "running");

    // -- media -----------------------------------------------------------
    w.__PROMO_MEDIA__ = [];
    const proto = HTMLMediaElement.prototype;
    const real = (name: string) => Object.getOwnPropertyDescriptor(proto, name)!;
    const realTime = real("currentTime");
    const state = new WeakMap<HTMLMediaElement, { playing: boolean; pos: number; since: number }>();
    const playing = new Set<HTMLMediaElement>();
    const now = () => performance.now();
    function positionOf(el: HTMLMediaElement): number {
      const s = state.get(el);
      if (!s) return realTime.get!.call(el) as number;
      if (!s.playing) return s.pos;
      return s.pos + ((now() - s.since) / 1000) * el.playbackRate;
    }
    const note = (type: string, el: HTMLMediaElement) =>
      w.__PROMO_MEDIA__.push({ type, at: performance.timeOrigin + now(), position: Math.min(positionOf(el), el.duration || Infinity) });

    Object.defineProperty(proto, "currentTime", {
      configurable: true,
      get(this: HTMLMediaElement) {
        const p = positionOf(this);
        return Number.isFinite(this.duration) ? Math.min(p, this.duration) : p;
      },
      set(this: HTMLMediaElement, v: number) {
        realTime.set!.call(this, v);
        const s = state.get(this);
        if (s) {
          s.pos = v;
          s.since = now();
        } else {
          state.set(this, { playing: false, pos: v, since: now() });
        }
      },
    });
    Object.defineProperty(proto, "paused", {
      configurable: true,
      get(this: HTMLMediaElement) {
        return !state.get(this)?.playing;
      },
    });
    Object.defineProperty(proto, "ended", {
      configurable: true,
      get(this: HTMLMediaElement) {
        const s = state.get(this);
        return !!s && !s.playing && Number.isFinite(this.duration) && s.pos >= this.duration;
      },
    });
    proto.play = function (this: HTMLMediaElement) {
      let s = state.get(this);
      if (!s) {
        s = { playing: false, pos: realTime.get!.call(this) as number, since: now() };
        state.set(this, s);
      }
      if (!s.playing) {
        // Playing again from the end starts over, as a real element does.
        if (Number.isFinite(this.duration) && s.pos >= this.duration) s.pos = 0;
        s.playing = true;
        s.since = now();
        playing.add(this);
        const el = this;
        queueMicrotask(() => {
          el.dispatchEvent(new Event("play"));
          note("playing", el);
          el.dispatchEvent(new Event("playing"));
        });
      }
      return Promise.resolve();
    };
    // A media element taken out of the document is paused, and says so, as the
    // HTML spec has it. The app's tab switch does this to the mix player.
    new MutationObserver((records) => {
      for (const r of records) {
        for (const node of r.removedNodes) {
          const els: HTMLMediaElement[] =
            node instanceof HTMLMediaElement
              ? [node]
              : node instanceof Element
                ? [...node.querySelectorAll<HTMLMediaElement>("audio, video")]
                : [];
          for (const el of els) {
            const s = state.get(el);
            if (!s?.playing) continue;
            s.pos = positionOf(el);
            s.playing = false;
            playing.delete(el);
            note("pause", el);
            el.dispatchEvent(new Event("pause"));
          }
        }
      }
    }).observe(document, { childList: true, subtree: true });
    proto.pause = function (this: HTMLMediaElement) {
      const s = state.get(this);
      if (s?.playing) {
        s.pos = positionOf(this);
        s.playing = false;
        playing.delete(this);
        note("pause", this);
        this.dispatchEvent(new Event("pause"));
      }
    };
  });
}

/** Ask the page to bring its animations and media to `nowMs` (its own fake clock). */
export async function syncPage(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const w = window as unknown as { __promoSync: (n: number) => void; __promoBusy: () => boolean };
    w.__promoSync(performance.now());
    return w.__promoBusy();
  });
}
