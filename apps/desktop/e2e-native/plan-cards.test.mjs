// How the stories meet a plan-approval card and a chat error, against a
// stand-in page (no display, driver, app or model needed):
//
//   node --test apps/desktop/e2e-native/plan-cards.test.mjs
//
// A turn the app gates on a plan parks at the card until someone presses
// Run. waitForReply used to run out its whole timeout on that, saying only
// "last: false"; it now names the card at once. ask() (askThrough) presses
// Run, as a person would, and leaves a note that a card appeared.
import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { Driver } from "./webdriver.mjs";
import { waitForReply } from "./stories/agent.mjs";
import { ask, endsInQuestion } from "./stories/claude.mjs";

const CARD = "[data-testid='plan-approval-card']";
const BUBBLE = "[data-testid='message-bubble'][data-role='assistant']";

/** What the page holds; each test sets what it needs. */
let page;
beforeEach(() => {
  page = { card: null, error: null, bubbles: [], busy: false, thinking: 0, caret: 0 };
});

// The functions the stories run in the page read `document`, so give them one.
globalThis.document = {
  querySelector(css) {
    if (css === CARD) {
      if (!page.card) return null;
      const { steps, text } = page.card;
      return { innerText: text, querySelector: (s) => (s === "ol" && steps != null ? { innerText: steps } : null) };
    }
    if (css === "[data-testid='chat-form'] textarea") return { disabled: page.busy };
    return null;
  },
  querySelectorAll(css) {
    return css === BUBBLE ? page.bubbles.map((textContent) => ({ textContent })) : [];
  },
};

/** The real Driver's polling (`until`), over the stand-in page. */
function fakeCtx() {
  const d = Object.create(Driver.prototype);
  d.exec = async (fn, ...args) => fn(...args);
  d.count = async (css) => {
    if (css === CARD) return page.card ? 1 : 0;
    if (css === "[data-testid='chat-error']") return page.error ? 1 : 0;
    if (css === "[data-testid='thinking-indicator']") return page.thinking;
    if (css === "[data-testid='caret']") return page.caret;
    if (css === BUBBLE) return page.bubbles.length;
    return 0;
  };
  d.text = async (css) => {
    if (css === "[data-testid='chat-error']") return page.error;
    throw new Error(`no such element: ${css}`);
  };
  const notes = [];
  return { d, notes, note: (t) => notes.push(t), shot: async () => {} };
}

const plan = (steps, text = "Plan 2 STEPS Discard Run") => ({ steps, text });
const timed = async (fn) => {
  const t0 = Date.now();
  const r = await fn().then(
    () => ({ ok: true }),
    (error) => ({ error }),
  );
  return { ...r, ms: Date.now() - t0 };
};

test("a plan card with no reply: waitForReply throws at once, naming the steps", async () => {
  page.card = plan("1. Raise track 0 by 6 dB\n2. Render a preview");
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.equal(r.error?.message, "a plan card is waiting for approval: 1. Raise track 0 by 6 dB 2. Render a preview");
  assert.ok(r.ms < 5000, `reported in ${r.ms} ms, not at the timeout`);
});

test("blank steps (#481) still name the card", async () => {
  page.card = plan("", "Plan 1 STEP Discard Run");
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.match(r.error?.message, /^a plan card is waiting for approval: .*blank.*"Plan 1 STEP Discard Run"/);
});

test("a card is reported even when an earlier turn left a reply on screen", async () => {
  page.bubbles = ["Done: +3 dB."];
  page.card = plan("1. Fade out the last second");
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.match(r.error?.message, /^a plan card is waiting for approval: 1\. Fade out/);
  assert.ok(r.ms < 5000);
});

test("a card that was just answered and goes away is not reported", async () => {
  page.card = plan("1. Raise track 0 by 6 dB");
  setTimeout(() => {
    page.card = null;
    page.bubbles = ["Raised track 0 by 6 dB."];
  }, 400);
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.equal(r.error, undefined, r.error?.message);
});

test("a finished reply with no card returns", async () => {
  page.bubbles = ["Hello."];
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.equal(r.error, undefined, r.error?.message);
});

test("a chat error throws at once, with its text (until would otherwise swallow it for the whole timeout)", async () => {
  page.error = "Your credit balance is too low to access the Anthropic API.";
  const r = await timed(() => waitForReply(fakeCtx(), { timeout: 60000 }));
  assert.equal(r.error?.message, "the chat showed an error: Your credit balance is too low to access the Anthropic API.");
  assert.ok(r.ms < 5000);
});

test("ask() presses Run on a plan card, notes it, and returns once the turn ends", async () => {
  const ctx = fakeCtx();
  const clicked = [];
  // `say` types into the box and presses Enter; the turn then starts (the
  // box is disabled) and, a moment later, the app puts up its plan.
  ctx.d.type = async () => {
    page.busy = true;
    setTimeout(() => (page.card = plan("1. Reverse track 0")), 300);
  };
  ctx.d.keys = async () => {};
  ctx.d.click = async (css) => {
    clicked.push(css);
    page.card = null;
    page.busy = false;
  };
  const badges = await ask(ctx, "Reverse the whole of track 0.");
  assert.deepEqual(clicked, ["[data-testid='plan-run-button']"]);
  assert.deepEqual(badges, []);
  assert.equal(ctx.notes.length, 1);
  assert.equal(ctx.notes[0], "Reverse the whole of track 0.: a plan card appeared and was approved (Run): 1. Reverse track 0");
});

test("ask() on a turn with no plan card presses nothing and notes nothing", async () => {
  const ctx = fakeCtx();
  ctx.d.type = async () => {
    page.busy = true;
    setTimeout(() => ((page.busy = false), (page.bubbles = ["Done."])), 800);
  };
  ctx.d.keys = async () => {};
  ctx.d.click = async (css) => assert.fail(`clicked ${css}`);
  await ask(ctx, "Make track 0 louder by 6 dB.");
  assert.deepEqual(ctx.notes, []);
});

test("endsInQuestion: a closing question, even in emphasis or quotes", () => {
  for (const q of ["Shall I add a limiter?", "Shall I add a limiter? ", "Add a limiter (yes/no)?", 'Do you mean "louder"?', "Want a limiter?**"]) {
    assert.ok(endsInQuestion(q), q);
  }
  for (const s of ["Exported to /tmp/a.wav.", "Is that OK? I went ahead.", "", "Done!"]) {
    assert.ok(!endsInQuestion(s), s);
  }
});
