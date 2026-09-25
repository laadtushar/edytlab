/**
 * Switching sides of an A/B comparison crossfades, and does not stop
 * (#269 §2).
 *
 * The one mix player used to reload in place: the old side stopped, the
 * new one loaded, and playback came back with a gap and a hard cut —
 * the click the comparison plan promised would not be there. The switch
 * now plays both for a moment, the old side fading out as the new one
 * fades in from the same position.
 *
 * Read from the two `<audio>` elements the mix plays through, in real
 * Chromium media playback. jsdom cannot play anything.
 */

import { fixturePath } from "./audio-fixtures";
import { nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const A = nodeId(1);
const B = nodeId(2);

interface Sample {
  at: number;
  sides: { volume: number; paused: boolean; time: number }[];
}

test("switching to B mid-playback crossfades from the same moment", async ({ app }) => {
  const aPath = fixturePath("tone3s");
  const bPath = fixturePath("tone3sB");
  await app.boot({
    ...projectWith([toneTrack()], A),
    // `get_graph`: a load, and one edit after it.
    get_graph: ok({
      nodes: [
        { id: A, parent: null, label: "load", tool: "load", created_at: "2026-09-25T00:00:00Z" },
        { id: B, parent: A, label: "gain", tool: "gain", created_at: "2026-09-25T00:01:00Z" },
      ],
      head: A,
    }),
    // `prepare_compare` renders both sides and returns their paths.
    prepare_compare: ok({ a_path: aPath, b_path: bPath }),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // Compare the head with the edit after it, from the graph.
  await page.getByTestId("tab-graph").click();
  await page.getByTestId("graph-bubble").nth(1).click({ button: "right" });
  await page.getByTestId("graph-context-menu").getByText("Compare with…").click();
  await expect(page.getByTestId("ab-compare-bar")).toBeVisible();
  expect(await app.requestsFor("prepare_compare")).toEqual([{ a: A, b: B }]);

  // Back to the timeline, and play side A.
  await page.getByTestId("tab-timeline").click();
  const sides = page.locator("[data-testid=timeline-mix-player] audio");
  await expect(sides).toHaveCount(2);
  // Side A is loaded once one of the two has something to play.
  await expect
    .poll(() => sides.evaluateAll((els) => els.filter((e) => (e as HTMLAudioElement).src).length))
    .toBe(1);
  await page.keyboard.press("Space");
  await expect
    .poll(() =>
      sides.evaluateAll((els) =>
        Math.max(...els.map((e) => ((e as HTMLAudioElement).paused ? 0 : (e as HTMLAudioElement).currentTime))),
      ),
    )
    .toBeGreaterThan(0.5);

  // Watch both sides through the switch, a sample every couple of ms.
  const watch = sides.first().evaluate(async (first) => {
    const host = first.parentElement!;
    const out: Sample[] = [];
    const start = performance.now();
    while (performance.now() - start < 1200) {
      out.push({
        at: performance.now() - start,
        sides: [...host.querySelectorAll("audio")].map((e) => ({
          volume: e.volume,
          paused: e.paused,
          time: e.currentTime,
        })),
      });
      await new Promise((r) => setTimeout(r, 2));
    }
    return out;
  });
  // The sides are told apart by what they do, not by name — both play
  // from blob URLs. A is the one playing now; B is the other.
  const [aIndex, beforeSwitch] = await sides.evaluateAll((els) => {
    const i = els.findIndex((e) => !(e as HTMLAudioElement).paused);
    return [i, (els[i] as HTMLAudioElement).currentTime] as const;
  });
  expect(aIndex).toBeGreaterThanOrEqual(0);
  await page.getByTestId("ab-side-b").click();
  const samples = await watch;

  const sideA = (s: Sample) => s.sides[aIndex];
  const sideB = (s: Sample) => s.sides[1 - aIndex];

  // At some moment both sides play at once, each below full level: the
  // crossfade. A hard cut never has that.
  const crossing = samples.filter((s) => {
    const a = sideA(s);
    const b = sideB(s);
    return !a.paused && !b.paused && a.volume < 1 && b.volume > 0;
  });
  expect(crossing.length, "no moment where A faded out as B faded in").toBeGreaterThan(0);

  // And it ends on B alone, at full level, still playing, from where A
  // was — never back at the start.
  const last = samples[samples.length - 1];
  expect(sideB(last).paused).toBe(false);
  expect(sideB(last).volume).toBe(1);
  expect(sideB(last).time).toBeGreaterThan(beforeSwitch);
  expect(sideA(last).paused).toBe(true);

  // Stop, so teardown finds the app quiet.
  await page.keyboard.press("Space");
});
