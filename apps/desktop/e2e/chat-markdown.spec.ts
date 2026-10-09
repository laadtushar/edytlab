/**
 * The assistant's markdown is drawn as formatting, inside the bubble (#440).
 *
 * The unit tests (`MessageBubble.markdown.test.tsx`) pin which elements
 * come out. What only a real browser can see is whether they *look* like
 * what they are, and whether anything pushes the chat wider than its
 * panel:
 *
 *   - Tailwind's preflight strips list markers, indents and heading
 *     weights, so a list is only a list if the stylesheet puts them back;
 *   - a code block or table wider than the bubble has to scroll by
 *     itself, or the whole transcript gets a horizontal scrollbar;
 *   - a long URL has to wrap.
 *
 * jsdom has no layout, so none of that exists there. The page also runs
 * under the shipped Content Security Policy, and the fixture fails the
 * test on any violation, so this is also where the markdown is shown to
 * need nothing the policy refuses.
 */

import { fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test, type App } from "./fixtures";

const HEAD = nodeId(2);

/** The panel and a turn that has been asked but not answered. */
async function ask(app: App, message: string) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    send_message: deferred("send"),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  const box = page.getByTestId("chat-form").locator("textarea");
  await box.fill(message);
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);
}

const LONG_URL = "https://example.com/" + "a-very-long-path-segment/".repeat(8) + "end";

const REPLY = [
  "Here is what I did:",
  "",
  "- **Fade:** smooths the start",
  "- **Trim:** ran `silence_finder` first",
  "",
  "```json",
  '{ "tool": "fade", "args": { "track": 0, "start_sec": 0.0, "end_sec": 0.5, "curve": "exponential-in" } }',
  "```",
  "",
  "| Tool | What it does to the selection | Typical use | Reversible | Notes about the sound | One more column |",
  "| --- | --- | --- | --- | --- | --- |",
  "| fade | ramps the gain down over the length of the region you picked | endings | yes | gentle | yes |",
  "",
  `Read [the guide](${LONG_URL}) for more.`,
].join("\n");

test("a reply is drawn as formatting and stays inside the chat", async ({ app }) => {
  await ask(app, "make it **not bold**");
  const page = app.page;

  // What the user typed is shown as typed.
  const mine = page.getByTestId("message-bubble").and(page.locator('[data-role="user"]'));
  await expect(mine).toHaveText("make it **not bold**");
  await expect(mine.locator("strong")).toHaveCount(0);

  await app.emit("agent://text-delta", { text: REPLY });
  await app.emit("agent://done", {});

  const reply = page.getByTestId("message-bubble").and(page.locator('[data-role="assistant"]'));
  await expect(reply).toBeVisible();
  await expect(page.getByTestId("caret")).toHaveCount(0);

  // The demo-video line: bold lead-in, bullets, and no symbols.
  await expect(reply.locator("li strong")).toHaveText(["Fade:", "Trim:"]);
  await expect(reply).not.toContainText("**");
  await expect(reply).not.toContainText("```");
  await expect(reply.locator("li")).toHaveCount(2);

  // Preflight strips markers and weights; the stylesheet puts them back.
  const style = await reply.evaluate((el) => {
    const css = (sel: string) => {
      const node = el.querySelector(sel);
      if (!node) throw new Error(`no ${sel}`);
      return getComputedStyle(node);
    };
    return {
      listStyle: css("ul").listStyleType,
      indent: parseFloat(css("ul").paddingLeft),
      strongWeight: Number(css("li strong").fontWeight),
      codeFont: css("li code").fontFamily,
      codeBackground: css("li code").backgroundColor,
      preWhiteSpace: css("pre").whiteSpace,
      preOverflowX: css("pre").overflowX,
      tableOverflowX: css(".chat-md-table").overflowX,
    };
  });
  expect(style.listStyle).toBe("disc");
  expect(style.indent).toBeGreaterThan(10);
  expect(style.strongWeight).toBeGreaterThanOrEqual(600);
  expect(style.codeFont).toMatch(/mono/i);
  expect(style.codeBackground).not.toBe("rgba(0, 0, 0, 0)");
  expect(style.preWhiteSpace).toBe("pre");
  expect(style.preOverflowX).toBe("auto");
  expect(style.tableOverflowX).toBe("auto");

  // Nothing is wider than its place. The code block and the table are
  // wider than the bubble and scroll inside it; the chat does not.
  const widths = await page.evaluate(() => {
    const scroller = document.querySelector('[data-testid="chat-scroller"]') as HTMLElement;
    const bubble = document.querySelector(
      '[data-role="assistant"] .chat-md',
    ) as HTMLElement;
    const pre = bubble.querySelector("pre") as HTMLElement;
    const wrap = bubble.querySelector(".chat-md-table") as HTMLElement;
    const sc = scroller.getBoundingClientRect();
    const bb = bubble.getBoundingClientRect();
    return {
      scrollerOverflows: scroller.scrollWidth > scroller.clientWidth,
      bubbleInside: bb.left >= sc.left - 0.5 && bb.right <= sc.right + 0.5,
      bubbleOverflows: bubble.scrollWidth > bubble.clientWidth + 1,
      preScrolls: pre.scrollWidth > pre.clientWidth,
      preInside: pre.getBoundingClientRect().right <= bb.right + 0.5,
      tableScrolls: wrap.scrollWidth > wrap.clientWidth,
      tableInside: wrap.getBoundingClientRect().right <= bb.right + 0.5,
    };
  });
  expect(widths.scrollerOverflows, "the chat has a horizontal scrollbar").toBe(false);
  expect(widths.bubbleInside).toBe(true);
  expect(widths.bubbleOverflows, "something spills out of the bubble").toBe(false);
  expect(widths.preScrolls, "the test's code block is not wider than the bubble").toBe(true);
  expect(widths.preInside).toBe(true);
  expect(widths.tableScrolls, "the test's table is not wider than the bubble").toBe(true);
  expect(widths.tableInside).toBe(true);

  // A link shows where it goes, as text, and the long address wraps
  // rather than widening the bubble (checked above).
  await expect(reply).toContainText(`the guide (${LONG_URL})`);
  await expect(page.locator('[data-testid="chat-scroller"] a')).toHaveCount(0);

  await app.release("send", null);
});

test("a reply that is still typing keeps its caret on the last line", async ({ app }) => {
  await ask(app, "fade the intro");
  const page = app.page;

  // Cut off mid-syntax: the second `**` has not arrived.
  await app.emit("agent://text-delta", { text: "Applying a **fad" });
  const caret = page.getByTestId("caret");
  await expect(caret).toHaveCount(1);
  const inLine = () => caret.evaluate((el) => el.parentElement?.tagName);
  expect(await inLine()).toBe("P");

  // It closes, then a list starts. The caret follows the text each time
  // rather than dropping beneath it.
  await app.emit("agent://text-delta", { text: "e** to the intro:\n\n- fa" });
  await expect(page.locator('[data-role="assistant"] strong')).toHaveText("fade");
  await expect(caret).toHaveCount(1);
  await expect(caret.locator("xpath=ancestor::li")).toHaveCount(1);

  await app.emit("agent://done", {});
  await expect(caret).toHaveCount(0);
  await expect(page.locator('[data-role="assistant"] li')).toHaveText("fa");

  await app.release("send", null);
});
