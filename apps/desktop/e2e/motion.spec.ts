/**
 * The app's motion, in a real browser (docs/motion-audit.md, phase 3).
 *
 * jsdom computes no animation, so the unit tests can only say that a class
 * is on an element. This says that the browser, given the shipped
 * stylesheet, plays the animation the class names, over the time the
 * vocabulary gives it, and that under `prefers-reduced-motion: reduce` it
 * plays none of them. It is also where the one rule that matters most is
 * enforced against computed style: nothing WaveSurfer draws, and nothing
 * above it, ever moves. An animated ancestor of a lane's canvases is what
 * the freeze on Linux (#455) looked like from the outside.
 *
 * The file opts in to motion. Every other spec runs under a global
 * `reducedMotion: "reduce"` (see `playwright.config.ts`), so nothing there
 * waits on an entrance.
 *
 * No sleeping. An animation is found by its name: on `getAnimations()`
 * while it is running or waiting out a delay, and from the
 * `animationstart` events a listener recorded from the first script of
 * the page once it has ended (the classes fill `backwards`, so a finished
 * animation is no longer listed, and one that is 120ms long is over by
 * the time a test asks). A state that changes over a transition is polled
 * until it has.
 */

import type { Locator, Page } from "@playwright/test";

import { fixturePath } from "./audio-fixtures";
import {
  deferred,
  firstRun,
  nodeId,
  ok,
  projectWith,
  readyToLoad,
  selecting,
  sessionOnlyKeychain,
  settingsTabs,
  toneTrack,
  type Backend,
} from "./backend";
import { expect, test, type App } from "./fixtures";
import { dragSelect } from "./timeline";

test.use({
  contextOptions: { reducedMotion: "no-preference" },
  viewport: { width: 1280, height: 800 },
});

const HEAD = nodeId(2);

declare global {
  interface Window {
    /**
     * Every CSS animation that has started on the page, in order, with
     * the properties its keyframes set (read as it starts: once it has
     * ended the browser no longer lists it).
     */
    __STARTED__: { el: Element; name: string; props: string[] }[];
  }
}

// From the first script of every page, so an animation that starts and
// ends between two of a test's calls is still seen.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__STARTED__ = [];
    document.addEventListener(
      "animationstart",
      (e) => {
        const el = e.target as Element;
        const own = el.getAnimations().find((a) => (a as CSSAnimation).animationName === e.animationName);
        const props = new Set<string>();
        for (const k of (own?.effect as KeyframeEffect | undefined)?.getKeyframes() ?? []) {
          for (const key of Object.keys(k)) {
            if (!["offset", "easing", "composite", "computedOffset"].includes(key)) props.add(key);
          }
        }
        window.__STARTED__.push({ el, name: e.animationName, props: [...props] });
      },
      true,
    );
  });
});

// -- reading the page ------------------------------------------------------

/**
 * The names of the CSS animations that are on, or have been on, one
 * element: the ones playing now (a delayed one is listed from the start)
 * and the ones that have started since the page began.
 */
const animationsOn = (loc: Locator) =>
  loc.evaluate((el) => {
    const names = el.getAnimations().map((a) => (a as CSSAnimation).animationName);
    for (const s of window.__STARTED__) if (s.el === el) names.push(s.name);
    return [...new Set(names)];
  });

/** The properties the keyframes of the animations an element has played set. */
const propertiesAnimatedOn = (loc: Locator) =>
  loc.evaluate((el) => {
    const props = new Set<string>();
    for (const s of window.__STARTED__) if (s.el === el) for (const p of s.props) props.add(p);
    return [...props].sort();
  });

/** A computed property of an element, or of one of its pseudo-elements. */
const computed = (loc: Locator, prop: string, pseudo: string | null = null) =>
  loc.evaluate(
    (el, [p, ps]) => (getComputedStyle(el, ps) as unknown as Record<string, string>)[p!],
    [prop, pseudo] as const,
  );

/** Seconds, from a computed `0.2s` (or the first of a list). */
const seconds = (value: string) => parseFloat(value.split(",")[0]);

/** Two frames, so a style change has been applied before it is read. */
const settleFrames = (page: Page) =>
  page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));

/**
 * Until everything that is entering has entered. A box measured while the
 * Settings card is still scaling in from .98, or a list still travelling
 * its 3px, is not the box it will be. (Nothing infinite is running where
 * this is used.)
 */
const entered = (page: Page) =>
  page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished)).then(() => undefined));

/**
 * The number of animations that have started on the page, once it has
 * stopped starting them for a dozen frames: a count taken the instant
 * something mounts misses the ones that start a frame later.
 */
const startedWhenQuiet = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<number>((resolve) => {
        let last = window.__STARTED__.length;
        let quiet = 0;
        const tick = () => {
          const n = window.__STARTED__.length;
          if (n !== last) {
            last = n;
            quiet = 0;
          } else if (++quiet >= 12) {
            resolve(n);
            return;
          }
          requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      }),
  );

/** The project the timeline tests share: one track of tone, a head. */
async function loaded(app: App, extra: Backend = {}): Promise<void> {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    ...selecting(),
    ...extra,
  });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
  await expect(app.page.getByTestId("timeline-lane")).toHaveCount(1);
}

// -- Settings --------------------------------------------------------------

test.describe("Settings", () => {
  test("opens like the other overlays: the card scales in, the scrim fades, nothing blurs", async ({ app }) => {
    await app.boot(readyToLoad());
    await app.page.getByTestId("open-settings-button").click();
    const card = app.page.getByTestId("settings-card");
    const scrim = app.page.getByTestId("settings");

    await expect.poll(() => animationsOn(card)).toContain("overlay-in");
    expect(await animationsOn(scrim)).toContain("backdrop-in");
    expect(seconds(await computed(card, "animationDuration"))).toBeCloseTo(0.2, 5);
    expect(seconds(await computed(scrim, "animationDuration"))).toBeCloseTo(0.2, 5);
    // The scrim's opacity animates, so it must not blur what is behind it
    // (WebKitGTK re-blurs the whole window every frame), and it is no
    // longer the old translating fade.
    expect(await computed(scrim, "backdropFilter")).toBe("none");
    expect(await animationsOn(scrim)).not.toContain("app-fade-in");
    // The card enters by opacity and scale only.
    expect(await propertiesAnimatedOn(card)).toEqual(["opacity", "transform"]);
  });

  test("the first-run dialog enters the same way", async ({ app }) => {
    await app.boot(firstRun());
    const dialog = app.page.getByRole("heading", { name: "Welcome to edytlab" });
    await expect(dialog).toBeVisible();
    await expect.poll(() => animationsOn(app.page.getByTestId("settings-card"))).toContain("overlay-in");
    expect(await animationsOn(app.page.getByTestId("settings"))).toContain("backdrop-in");
  });

  test("a tab's content is a fresh element that fades in over --dur-1", async ({ app }) => {
    await app.boot({ ...readyToLoad(), ...settingsTabs() });
    await app.page.getByTestId("open-settings-button").click();
    const content = app.page.getByTestId("settings-body").locator(":scope > div");
    await expect(content).toHaveCount(1);
    // Remembered on the element, to see that the next tab is not it.
    await content.evaluate((el) => ((el as unknown as { __was: string }).__was = "account"));

    await app.page.getByTestId("settings-tab-plugins").click();
    await expect(app.page.getByTestId("plugins-panel")).toBeVisible();
    expect(await content.evaluate((el) => (el as unknown as { __was?: string }).__was)).toBeUndefined();
    expect(await animationsOn(content)).toContain("fade-in");
    expect(seconds(await computed(content, "animationDuration"))).toBeCloseTo(0.12, 5);
    // Opacity only: no box moves while it fades.
    expect(await propertiesAnimatedOn(content)).toEqual(["opacity"]);
  });

  test("the tab indicator slides in from the left, and the tab does not resize", async ({ app }) => {
    await app.boot({ ...readyToLoad(), ...settingsTabs() });
    await app.page.getByTestId("open-settings-button").click();
    const account = app.page.getByTestId("settings-tab-account");
    const plugins = app.page.getByTestId("settings-tab-plugins");
    const scaleX = async (loc: Locator) => parseFloat((await computed(loc, "scale", "::after")).split(" ")[0]);

    await expect.poll(() => scaleX(account)).toBe(1);
    expect(await scaleX(plugins)).toBe(0);
    expect(await computed(plugins, "transitionProperty", "::after")).toBe("scale");
    expect(seconds(await computed(plugins, "transitionDuration", "::after"))).toBeCloseTo(0.2, 5);
    expect(await computed(plugins, "transformOrigin", "::after")).toMatch(/^0px /);

    await entered(app.page);
    const before = await plugins.boundingBox();
    await plugins.click();
    await expect.poll(() => scaleX(plugins)).toBe(1);
    await expect.poll(() => scaleX(account)).toBe(0);
    // A pseudo-element's scale: the tab is the box it was.
    const after = await plugins.boundingBox();
    expect(after!.width).toBeCloseTo(before!.width, 1);
    expect(after!.height).toBeCloseTo(before!.height, 1);
  });

  test("a link's underline sweeps in on hover", async ({ app }) => {
    await app.boot(readyToLoad());
    await app.page.getByTestId("open-settings-button").click();
    const link = app.page.getByRole("link", { name: /how to get a key/i });
    const scaleX = async () => parseFloat((await computed(link, "scale", "::after")).split(" ")[0]);

    expect(await scaleX()).toBe(0);
    expect(seconds(await computed(link, "transitionDuration", "::after"))).toBeCloseTo(0.2, 5);
    await link.hover();
    await expect.poll(scaleX).toBe(1);
    expect(await computed(link, "transformOrigin", "::after")).toMatch(/^0px /);
    // Out to the right: the origin flips as the pointer leaves.
    await app.page.mouse.move(2, 2);
    await expect.poll(scaleX).toBe(0);
    const box = await link.boundingBox();
    expect(parseFloat(await computed(link, "transformOrigin", "::after"))).toBeGreaterThan(box!.width - 1);
  });
});

// -- the chat --------------------------------------------------------------

const steps = (n: number) =>
  Array.from({ length: n }, (_, i) => ({ step: i + 1, tool: "reverse", description: `track: ${i}` }));

/** The turn up to the plan card: the user asks, a call starts, a plan arrives. */
async function askForAPlan(app: App, n: number): Promise<void> {
  await loaded(app, {
    get_plan_first: ok(true),
    send_message: deferred("send"),
    approve_plan: ok(null),
    reject_plan: ok(null),
  });
  const box = app.page.getByTestId("chat-form").locator("textarea");
  await box.fill("Reverse every track");
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);
  await app.emit("agent://tool-call", { name: "reverse", id: "t1" });
  await app.emit("agent://plan", { steps: steps(n) });
  await expect(app.page.getByTestId("plan-approval-card")).toBeVisible();
}

test.describe("the chat", () => {
  test("the plan card enters, and its steps follow in reading order, capped", async ({ app }) => {
    await askForAPlan(app, 8);
    const card = app.page.getByTestId("plan-approval-card");

    expect(await animationsOn(card)).toContain("plan-in");
    expect(seconds(await computed(card, "animationDuration"))).toBeCloseTo(0.2, 5);

    const rows = app.page.getByTestId("plan-step");
    await expect(rows).toHaveCount(8);
    const delays: number[] = [];
    for (let i = 0; i < 8; i++) {
      const row = rows.nth(i);
      expect(await animationsOn(row)).toContain("stagger-in");
      expect(seconds(await computed(row, "animationDuration"))).toBeCloseTo(0.12, 5);
      delays.push(seconds(await computed(row, "animationDelay")));
    }
    // 30ms apart, then flat at five steps: the last row waits no longer
    // than the sixth, however long the plan is.
    [0, 0.03, 0.06, 0.09, 0.12, 0.15, 0.15, 0.15].forEach((want, i) => expect(delays[i]).toBeCloseTo(want, 5));
    // Card and steps together: under the 300ms enter budget.
    expect(0.15 + 0.12).toBeLessThan(0.3);

    await app.release("send", null);
  });

  test("a tool badge settles when its call lands, and spins only while it runs", async ({ app }) => {
    await askForAPlan(app, 1);
    const badge = app.page.getByTestId("tool-badge");
    const glyph = badge.locator("[aria-hidden]");

    await expect(badge).toHaveAttribute("data-status", "running");
    expect(await animationsOn(glyph)).toContain("spin");
    expect(await animationsOn(glyph)).not.toContain("badge-settle");
    expect(await animationsOn(badge)).toContain("app-fade-in");

    await app.emit("agent://tool-call-end", { id: "t1", ok: true, result: "Reversed" });
    await expect(badge).toHaveAttribute("data-status", "ok");
    expect(await animationsOn(glyph)).toContain("badge-settle");
    expect(await animationsOn(glyph)).not.toContain("spin");
    expect(seconds(await computed(glyph, "animationDuration"))).toBeCloseTo(0.2, 5);
    // The badge's own tint eases between statuses, over --dur-2.
    expect(seconds(await computed(badge, "transitionDuration"))).toBeCloseTo(0.2, 5);
    expect(await computed(badge, "transitionProperty")).toContain("border-color");

    await app.release("send", null);
  });

  test("nothing animates per streamed token", async ({ app }) => {
    await askForAPlan(app, 1);
    await app.emit("agent://text-delta", { text: "Reversing" });
    // The assistant's streaming bubble (the user's own is above it).
    const bubble = app.page.locator('[data-testid="message-bubble"][data-role="assistant"]');
    await expect(bubble).toBeVisible();
    // Its own entrance and its caret's pulse are the bubble arriving, not
    // tokens: let both begin before counting.
    await expect.poll(() => animationsOn(bubble)).toContain("app-fade-in");
    // Every animation that has started on the page, ever: a delta that
    // started one would add to it.
    const settled = await startedWhenQuiet(app.page);

    for (const word of [" the", " first", " track", " and", " then", " the", " rest."]) {
      await app.emit("agent://text-delta", { text: word });
    }
    await expect(bubble).toContainText("the rest.");
    // Every delta re-rendered the bubble; none of them started an animation
    // or remounted it.
    expect(await startedWhenQuiet(app.page)).toBe(settled);
    await expect(bubble).toHaveCount(1);

    await app.release("send", null);
  });

  test("a banner in the chat drops in from the edge above", async ({ app }) => {
    await askForAPlan(app, 1);
    await app.emit("agent://plan-unavailable", { reason: "the model did not return a plan", first_edit_held: true });
    const notice = app.page.getByTestId("chat-notice");
    await expect(notice).toBeVisible();
    expect(await animationsOn(notice)).toContain("banner-in");
    await app.release("send", null);
  });
});

test("the left pane's banners drop in", async ({ app }) => {
  await app.boot({ ...readyToLoad(), ...sessionOnlyKeychain() });
  const banner = app.page.getByTestId("keychain-session-only");
  await expect(banner).toBeVisible();
  expect(await animationsOn(banner)).toContain("banner-in");
  expect(seconds(await computed(banner, "animationDuration"))).toBeCloseTo(0.2, 5);
});

// -- the empty state -------------------------------------------------------

test.describe("the empty state", () => {
  test("builds the logo once, then reveals its groups in order", async ({ app }) => {
    await app.boot(readyToLoad());
    const root = app.page.getByTestId("empty-state");
    await expect(root).toBeVisible();

    const bars = root.locator("[data-logo-bar]");
    await expect(bars).toHaveCount(3);
    for (let i = 0; i < 3; i++) {
      expect(await animationsOn(bars.nth(i))).toContain("logo-bar-in");
      expect(seconds(await computed(bars.nth(i), "animationDelay"))).toBeCloseTo(i * 0.04, 5);
    }
    const lines = root.locator("[data-logo-line]");
    await expect(lines).toHaveCount(2);
    for (let i = 0; i < 2; i++) {
      expect(await animationsOn(lines.nth(i))).toContain("logo-line-draw");
      expect(seconds(await computed(lines.nth(i), "animationDelay"))).toBeCloseTo(0.08, 5);
    }

    const groups = root.locator(".reveal-in");
    const delays = await groups.evaluateAll((els) => els.map((el) => parseFloat(getComputedStyle(el).animationDelay)));
    expect(delays.length).toBeGreaterThan(3);
    expect([...delays].sort((a, b) => a - b)).toEqual(delays);
    expect(Math.max(...delays)).toBeCloseTo(0.09, 5);
    // The whole reveal is over inside 300ms: the latest group's delay plus its duration.
    expect(Math.max(...delays) + 0.2).toBeLessThanOrEqual(0.3);

    // The root does not fade as well, or the two would stack.
    expect(await animationsOn(root)).toEqual([]);
  });

  test("and then it is still: nothing loops on an idle screen", async ({ app }) => {
    await app.boot(readyToLoad());
    await expect(app.page.getByTestId("empty-state")).toBeVisible();
    const endless = await app.page.evaluate(() =>
      document
        .getAnimations()
        .filter((a) => a.effect?.getComputedTiming().iterations === Infinity)
        .map((a) => (a as CSSAnimation).animationName),
    );
    expect(endless).toEqual([]);
  });

  test("the primary button glows wider under the pointer, and dips when pressed", async ({ app }) => {
    await app.boot(readyToLoad());
    const open = app.page.getByTestId("empty-state-open-button");
    const rest = await computed(open, "boxShadow");

    await open.hover();
    await expect.poll(() => computed(open, "boxShadow")).not.toBe(rest);
    expect(seconds(await computed(open, "transitionDuration"))).toBeCloseTo(0.12, 5);

    await app.page.mouse.down();
    await expect.poll(() => computed(open, "scale")).toBe("0.96");
    // Released somewhere else, so it is a press and not a click.
    await app.page.mouse.move(2, 2);
    await app.page.mouse.up();
    await expect.poll(() => computed(open, "scale")).toBe("none");
  });
});

test("a card lights its edge under the pointer, and does not lift", async ({ app }) => {
  await app.boot({
    ...readyToLoad(),
    list_recent_projects: ok([{ path: "/p/one", name: "one", last_opened_at: null }]),
  });
  const row = app.page.getByTestId("recent-project-row");
  await expect(row).toBeVisible();
  // Past the empty state's own entrance (the list is one of its groups,
  // which still has its 3px to travel), so a move measured below is the
  // hover's and not that.
  await entered(app.page);
  const rest ={ border: await computed(row, "borderTopColor"), shadow: await computed(row, "boxShadow") };
  expect(rest.shadow).toBe("none");
  const before = await row.boundingBox();

  await row.hover();
  await expect.poll(() => computed(row, "boxShadow")).not.toBe("none");
  await expect.poll(() => computed(row, "borderTopColor")).not.toBe(rest.border);
  expect(seconds(await computed(row, "transitionDuration"))).toBeCloseTo(0.12, 5);
  // No lift: a target that moves under the pointer is worse in a tool.
  const after = await row.boundingBox();
  expect(after!.y).toBe(before!.y);
  expect(await computed(row, "translate")).toBe("none");
  expect(await computed(row, "transform")).toBe("none");
});

// -- the press, in the timeline --------------------------------------------

test.describe("the press", () => {
  test("a mute toggle dips, a fader does not", async ({ app }) => {
    await loaded(app, { set_track_gain: ok(nodeId(3)) });
    const mute = app.page.getByTestId("timeline-lane-mute");

    // The inline transition lists `scale`, or the dip would snap.
    expect(await computed(mute, "transitionProperty")).toContain("scale");
    await mute.hover();
    await app.page.mouse.down();
    await expect.poll(() => computed(mute, "scale")).toBe("0.96");
    await app.page.mouse.move(2, 2);
    await app.page.mouse.up();
    await expect.poll(() => computed(mute, "scale")).toBe("none");

    // A slider is a drag handle: it must not shrink under the pointer.
    const gain = app.page.getByTestId("timeline-lane-gain");
    await gain.hover();
    await app.page.mouse.down();
    await settleFrames(app.page);
    expect(await computed(gain, "scale")).toBe("none");
    await app.page.mouse.up();
  });

  test("a fader's readout lights while its slider is held", async ({ app }) => {
    await loaded(app, { set_track_gain: ok(nodeId(3)) });
    const readout = app.page.getByTestId("timeline-lane-gain-readout");
    const accent = await app.page.evaluate(() => {
      const probe = document.createElement("i");
      probe.style.color = "var(--accent-text)";
      document.body.append(probe);
      const rgb = getComputedStyle(probe).color;
      probe.remove();
      return rgb;
    });
    const rest = await computed(readout, "color");
    expect(rest).not.toBe(accent);
    expect(seconds(await computed(readout, "transitionDuration"))).toBeCloseTo(0.12, 5);

    const gain = app.page.getByTestId("timeline-lane-gain");
    const box = (await gain.boundingBox())!;
    await app.page.mouse.move(box.x + box.width * 0.4, box.y + box.height / 2);
    await app.page.mouse.down();
    await expect(readout).toHaveAttribute("data-held", "true");
    await expect.poll(() => computed(readout, "color")).toBe(accent);

    await app.page.mouse.up();
    await expect(readout).not.toHaveAttribute("data-held", "true");
    await expect.poll(() => computed(readout, "color")).toBe(rest);
    await expect.poll(() => app.requestsFor("set_track_gain")).toHaveLength(1);
  });
});

// -- the left pane ---------------------------------------------------------

test.describe("the left pane", () => {
  const graph = () => {
    const at = (n: number) => `2026-10-09T00:0${n}:00Z`;
    return {
      get_graph: ok({
        nodes: [
          { id: nodeId(1), parent: null, label: "load", tool: "load", created_at: at(0) },
          { id: HEAD, parent: nodeId(1), label: "load", tool: "load", created_at: at(1) },
        ],
        head: HEAD,
      }),
    };
  };

  test("the Graph and the Transcript fade in when chosen; the Timeline does not", async ({ app }) => {
    await loaded(app, graph());
    const timeline = app.page.getByTestId("timeline-root");
    expect(await animationsOn(timeline)).toEqual([]);

    await app.page.getByTestId("tab-graph").click();
    const view = app.page.getByTestId("graph-view-root");
    await expect(view).toBeVisible();
    expect(await animationsOn(view)).toContain("fade-in");
    expect(seconds(await computed(view, "animationDuration"))).toBeCloseTo(0.2, 5);

    await app.page.getByTestId("tab-transcript").click();
    const empty = app.page.getByTestId("transcript-empty").locator("..");
    await expect(empty).toBeVisible();
    expect(await animationsOn(empty)).toContain("fade-in");

    // Coming back, the timeline is a plain remount of what was always
    // there: no fade over the canvases.
    await app.page.getByTestId("tab-timeline").click();
    await expect(app.page.getByTestId("timeline-root")).toBeVisible();
    expect(await animationsOn(app.page.getByTestId("timeline-root"))).toEqual([]);
  });

  test("the view tabs' indicator slides in", async ({ app }) => {
    await loaded(app, graph());
    const timelineTab = app.page.getByTestId("tab-timeline");
    const graphTab = app.page.getByTestId("tab-graph");
    const scaleX = async (loc: Locator) => parseFloat((await computed(loc, "scale", "::after")).split(" ")[0]);

    expect(await scaleX(timelineTab)).toBe(1);
    expect(await scaleX(graphTab)).toBe(0);
    await graphTab.click();
    await expect.poll(() => scaleX(graphTab)).toBe(1);
    await expect.poll(() => scaleX(timelineTab)).toBe(0);
  });

  test("a selection fades in when it mounts, and only fades", async ({ app }) => {
    await loaded(app);
    await dragSelect(app, 0.25, 0.6);
    const overlay = app.page.getByTestId("timeline-selection-overlay");
    await expect(overlay).toBeVisible();

    expect(await animationsOn(overlay)).toContain("fade-in");
    expect(seconds(await computed(overlay, "animationDuration"))).toBeCloseTo(0.12, 5);
    // Its position is the pointer's, not an easing's: it has no transition.
    expect(seconds(await computed(overlay, "transitionDuration"))).toBe(0);
    expect(await propertiesAnimatedOn(overlay)).toEqual(["opacity"]);
  });
});

// -- the rule that matters most --------------------------------------------

/**
 * Everything above WaveSurfer's container that could be moving it, or
 * painting it under a moving layer. Empty is the only good answer.
 *
 * Checked on the element, on its ancestors up to `<html>`, and against
 * every animation on the document (an animation on a pseudo-element
 * reports its originating element as its target).
 */
function whatMovesTheWaveform(page: Page): Promise<string[]> {
  return page.getByTestId("timeline-lane-waveform").first().evaluate((waveform) => {
    const label = (el: Element) => el.getAttribute("data-testid") ?? el.tagName.toLowerCase();
    const MOVES = new Set(["all", "opacity", "transform", "translate", "scale", "rotate", "filter", "backdrop-filter"]);
    const found: string[] = [];

    for (let el: Element | null = waveform; el; el = el.parentElement) {
      for (const a of el.getAnimations()) {
        found.push(`${label(el)} has the animation ${(a as CSSAnimation).animationName ?? a.id}`);
      }
      const style = getComputedStyle(el);
      // Whether or not it has played yet, or has finished: the element is
      // not given one. (Independent of timing, so it cannot miss a short
      // animation that ended before the check.)
      if (style.animationName !== "none") found.push(`${label(el)} is given animation ${style.animationName}`);
      const properties = style.transitionProperty.split(",").map((s) => s.trim());
      const durations = style.transitionDuration.split(",").map((s) => parseFloat(s));
      properties.forEach((p, i) => {
        // `all 0s` is the browser's default and moves nothing.
        if (MOVES.has(p) && durations[i % durations.length] > 0) {
          found.push(`${label(el)} transitions ${p} over ${durations[i % durations.length]}s`);
        }
      });
      if (/opacity|transform/.test(style.willChange)) found.push(`${label(el)} has will-change ${style.willChange}`);
    }
    for (const a of waveform.ownerDocument.getAnimations()) {
      const target = (a.effect as KeyframeEffect | null)?.target;
      if (target && target.contains(waveform)) {
        found.push(`${label(target)} is animated (${(a as CSSAnimation).animationName})`);
      }
    }
    // And any that started on it since the page began, however short.
    for (const s of window.__STARTED__) {
      if (s.el.contains(waveform)) found.push(`${label(s.el)} ran ${s.name}`);
    }
    return found;
  });
}

test.describe("WaveSurfer is never animated, and neither is anything above it", () => {
  test("after load", async ({ app }) => {
    await loaded(app);
    expect(await whatMovesTheWaveform(app.page)).toEqual([]);
  });

  test("with a selection, a plan card and Settings open over it", async ({ app }) => {
    await loaded(app, {
      get_plan_first: ok(true),
      send_message: deferred("send"),
      approve_plan: ok(null),
    });
    await dragSelect(app, 0.2, 0.7);
    const box = app.page.getByTestId("chat-form").locator("textarea");
    await box.fill("Reverse it");
    await box.press("Enter");
    await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);
    await app.emit("agent://plan", { steps: steps(3) });
    await expect(app.page.getByTestId("plan-approval-card")).toBeVisible();
    await app.page.getByTestId("open-settings-button").click();
    await expect.poll(() => animationsOn(app.page.getByTestId("settings-card"))).toContain("overlay-in");

    // Animations are running on the page, just not above the waveform.
    expect((await app.page.evaluate(() => document.getAnimations().length)) > 0).toBe(true);
    expect(await whatMovesTheWaveform(app.page)).toEqual([]);

    await app.page.getByTestId("settings-close").click();
    await app.release("send", null);
  });

  test("while the mix plays", async ({ app }) => {
    await loaded(app);
    await app.page.getByTestId("render-preview-button").click();
    await expect
      .poll(() =>
        app.page
          .locator("[data-testid=timeline-mix-player] audio")
          .evaluateAll((els) => (els as HTMLAudioElement[]).filter((a) => a.src).length),
      )
      .toBe(1);
    await app.page.getByTestId("play-pause-button").click();
    await expect
      .poll(() =>
        app.page
          .locator("[data-testid=timeline-mix-player] audio")
          .evaluateAll((els) => (els as HTMLAudioElement[]).some((a) => !a.paused && a.currentTime > 0)),
      )
      .toBe(true);

    expect(await whatMovesTheWaveform(app.page)).toEqual([]);
    await app.page.getByTestId("play-pause-button").click();
  });
});

// -- reduced motion --------------------------------------------------------

test.describe("under prefers-reduced-motion: reduce", () => {
  test.use({ contextOptions: { reducedMotion: "reduce" } });

  const FLAT = 1e-5;

  test("Settings, its tabs and the empty state play nothing", async ({ app }) => {
    await app.boot({ ...readyToLoad(), ...settingsTabs() });
    // The empty state, first.
    const bars = app.page.getByTestId("empty-state").locator("[data-logo-bar]");
    await expect(bars).toHaveCount(3);
    for (let i = 0; i < 3; i++) {
      expect(seconds(await computed(bars.nth(i), "animationDuration"))).toBeCloseTo(FLAT, 9);
      expect(seconds(await computed(bars.nth(i), "animationDelay"))).toBe(0);
    }
    const groups = app.page.getByTestId("empty-state").locator(".reveal-in");
    await expect(groups).not.toHaveCount(0);
    for (const delay of await groups.evaluateAll((els) => els.map((el) => getComputedStyle(el).animationDelay))) {
      expect(seconds(delay)).toBe(0);
    }

    await app.page.getByTestId("open-settings-button").click();
    const card = app.page.getByTestId("settings-card");
    await expect(card).toBeVisible();
    expect(seconds(await computed(card, "animationDuration"))).toBeCloseTo(FLAT, 9);
    expect(seconds(await computed(app.page.getByTestId("settings"), "animationDuration"))).toBeCloseTo(FLAT, 9);

    await app.page.getByTestId("settings-tab-plugins").click();
    const content = app.page.getByTestId("settings-body").locator(":scope > div");
    expect(seconds(await computed(content, "animationDuration"))).toBeCloseTo(FLAT, 9);
    // The tab indicator still shows the state; it just does not travel,
    // and nothing on the page is given a transition to wait out.
    const plugins = app.page.getByTestId("settings-tab-plugins");
    expect(await computed(plugins, "transitionProperty", "::after")).toBe("none");
    expect(await computed(plugins, "transitionProperty")).toBe("none");
    await expect
      .poll(async () => parseFloat((await computed(plugins, "scale", "::after")).split(" ")[0]))
      .toBe(1);
  });

  test("the plan card, its steps and a badge play nothing", async ({ app }) => {
    await askForAPlan(app, 6);
    const card = app.page.getByTestId("plan-approval-card");
    expect(seconds(await computed(card, "animationDuration"))).toBeCloseTo(FLAT, 9);
    const rows = app.page.getByTestId("plan-step");
    for (let i = 0; i < 6; i++) {
      expect(seconds(await computed(rows.nth(i), "animationDuration"))).toBeCloseTo(FLAT, 9);
      // No delay either: a staggered reveal would otherwise pop in row by row.
      expect(seconds(await computed(rows.nth(i), "animationDelay"))).toBe(0);
    }
    await app.emit("agent://tool-call-end", { id: "t1", ok: true });
    const glyph = app.page.getByTestId("tool-badge").locator("[aria-hidden]");
    await expect(app.page.getByTestId("tool-badge")).toHaveAttribute("data-status", "ok");
    expect(seconds(await computed(glyph, "animationDuration"))).toBeCloseTo(FLAT, 9);
    await app.release("send", null);
  });

  test("a held button does not dip", async ({ app }) => {
    await app.boot(readyToLoad());
    const open = app.page.getByTestId("empty-state-open-button");
    await open.hover();
    await app.page.mouse.down();
    await settleFrames(app.page);
    // The press is declared only where motion is welcome.
    expect(await computed(open, "scale")).toBe("none");
    await app.page.mouse.move(2, 2);
    await app.page.mouse.up();
  });
});
