// Driving the app the way a person does on film: the X pointer travels to
// a control before it is pressed, and presses are real X clicks, so the
// recording shows the cursor, the hover and the press.
//
// WebDriver clicks never move the X pointer, which is right for a test and
// wrong for a demo: on film, buttons would press themselves.
import { x } from "../native.mjs";
import { sleep } from "./helpers.mjs";

/** Where the page's (0, 0) is on the screen, measured: the pointer is put
 * on a known screen point and the page says where the mouse event landed.
 * The window's own geometry does not say it, since GTK draws a title bar
 * inside the window, above the page. Measured once per app session. */
const origins = new WeakMap();
async function origin(ctx) {
  if (origins.has(ctx.d)) return origins.get(ctx.d);
  await ctx.d.exec(() => {
    window.__pointer = null;
    if (!window.__pointerProbe) {
      window.__pointerProbe = true;
      window.addEventListener("mousemove", (e) => (window.__pointer = [e.clientX, e.clientY]), { capture: true });
    }
  });
  for (const [sx, sy] of [[700, 420], [720, 450], [740, 470]]) {
    x("mousemove", String(sx), String(sy));
    await sleep(200);
    const at = await ctx.d.exec(() => window.__pointer);
    if (at) {
      const o = { x: sx - at[0], y: sy - at[1] };
      origins.set(ctx.d, o);
      return o;
    }
  }
  throw new Error("the page saw no mouse movement, so the pointer cannot be placed");
}

function pointer() {
  const m = Object.fromEntries(
    x("getmouselocation", "--shell")
      .split("\n")
      .map((l) => l.split("="))
      .map(([k, v]) => [k, Number(v)]),
  );
  return { x: m.X, y: m.Y };
}

/** Glide the pointer to a screen point, easing in and out. */
async function glide(to) {
  const from = pointer();
  const dist = Math.hypot(to.x - from.x, to.y - from.y);
  if (dist < 2) return;
  const ms = Math.min(900, 260 + dist * 0.7);
  const steps = Math.max(12, Math.min(45, Math.round(dist / 18)));
  for (let i = 1; i <= steps; i++) {
    const p = i / steps;
    const e = p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2;
    x("mousemove", String(Math.round(from.x + (to.x - from.x) * e)), String(Math.round(from.y + (to.y - from.y) * e)));
    await sleep(ms / steps);
  }
}

/** Move the pointer onto `selector`, at a fraction of its box (centre by
 * default). Returns the screen point. */
export async function pointAt(ctx, selector, { fx = 0.5, fy = 0.5 } = {}) {
  await ctx.d.waitFor(selector);
  const r = await ctx.d.exec((sel) => {
    const el = document.querySelector(sel);
    el.scrollIntoView({ block: "nearest", inline: "nearest" });
    const b = el.getBoundingClientRect();
    return { left: b.left, top: b.top, width: b.width, height: b.height };
  }, selector);
  const o = await origin(ctx);
  const to = { x: Math.round(o.x + r.left + r.width * fx), y: Math.round(o.y + r.top + r.height * fy) };
  await glide(to);
  await sleep(140);
  return to;
}

/** Point at `selector` and press it with a real click. */
export async function press(ctx, selector, at) {
  await pointAt(ctx, selector, at);
  x("click", "1");
  await sleep(250);
}

/** Rest the pointer somewhere out of the way (over the chat log), so it
 * does not sit on a control while Claude works. */
export async function rest(ctx) {
  await pointAt(ctx, "[data-testid='chat-form']", { fx: 0.5, fy: -1.2 }).catch(() => {});
}
