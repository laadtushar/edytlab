// A minimal W3C WebDriver client for tauri-driver: just what these
// stories need, over fetch, so the native suite adds no dependencies.
import { writeFileSync } from "node:fs";

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

/** WebDriver key codes, for `keys(...)`. */
export const K = {
  ctrl: "\uE009",
  shift: "\uE008",
  enter: "\uE007",
  escape: "\uE00C",
  space: "\uE00D",
  end: "\uE010",
  home: "\uE011",
  left: "\uE012",
  right: "\uE014",
  backspace: "\uE003",
  del: "\uE017",
};

// Every request closes its connection. Node's fetch pools keep-alive
// sockets per origin, and a story's driver lives on the same port as the
// last story's dead one: the first request of the next story reused the
// dead socket and failed with "fetch failed" — which made every other
// story fail, with the new driver up and its log empty.
export class Driver {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async start({ application, args = [], base = "http://127.0.0.1:4444" }) {
    const res = await fetch(`${base}/session`, {
      method: "POST",
      headers: { "content-type": "application/json", connection: "close" },
      body: JSON.stringify({
        capabilities: {
          alwaysMatch: { "tauri:options": { application, args } },
        },
      }),
    });
    const body = await res.json();
    if (!res.ok) throw new Error(`session: ${JSON.stringify(body)}`);
    return new Driver(base, body.value.sessionId);
  }

  async cmd(method, path, payload) {
    const res = await fetch(`${this.base}/session/${this.id}${path}`, {
      method,
      headers: { "content-type": "application/json", connection: "close" },
      body: payload === undefined ? undefined : JSON.stringify(payload),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      throw new Error(`${method} ${path}: ${JSON.stringify(body.value ?? body).slice(0, 400)}`);
    }
    return body.value;
  }

  async quit() {
    await fetch(`${this.base}/session/${this.id}`, { method: "DELETE", headers: { connection: "close" } }).catch(() => {});
  }

  /** Run `fn` in the page; it receives `args` and may return a promise. */
  async exec(fn, ...args) {
    return this.cmd("POST", "/execute/async", {
      script: `const done = arguments[arguments.length - 1];
        Promise.resolve((${fn.toString()})(...Array.from(arguments).slice(0, -1)))
          .then((v) => done({ ok: v }), (e) => done({ err: String(e && e.message || e) }));`,
      args,
    }).then((r) => {
      if (r && "err" in r) throw new Error(r.err);
      return r?.ok;
    });
  }

  async find(css) {
    const v = await this.cmd("POST", "/element", { using: "css selector", value: css });
    return v[ELEMENT];
  }

  async findAll(css) {
    const v = await this.cmd("POST", "/elements", { using: "css selector", value: css });
    return v.map((e) => e[ELEMENT]);
  }

  /**
   * Click `css` as a person would. A visually hidden control (an sr-only
   * radio inside its <label>) is clicked through its label; nothing falls
   * back to a script click, which would hide a control nobody can reach.
   */
  async click(css) {
    const el = await this.waitFor(css);
    try {
      await this.cmd("POST", `/element/${el}/click`, {});
    } catch (e) {
      if (!/not interactable/.test(e.message)) throw e;
      const label = await this.cmd("POST", "/execute/sync", {
        script: "return arguments[0].closest('label');",
        args: [{ [ELEMENT]: el }],
      });
      if (!label) throw e;
      await this.cmd("POST", `/element/${label[ELEMENT]}/click`, {});
    }
  }

  async type(css, text) {
    const el = await this.waitFor(css);
    await this.cmd("POST", `/element/${el}/value`, { text });
  }

  async text(css) {
    const el = await this.waitFor(css);
    return this.cmd("GET", `/element/${el}/text`);
  }

  async keys(...keys) {
    await this.cmd("POST", "/actions", {
      actions: [
        {
          type: "key",
          id: "kb",
          actions: [
            ...keys.map((k) => ({ type: "keyDown", value: k })),
            ...keys.reverse().map((k) => ({ type: "keyUp", value: k })),
          ],
        },
      ],
    });
    await this.cmd("DELETE", "/actions");
  }

  /**
   * Pointer gestures on `css`, with offsets in CSS pixels from the
   * element's centre: `[["move", x, y], ["down"], ["move", x, y, ms],
   * ["up"]]`. `button` 2 is a right click.
   */
  async pointer(css, steps, { button = 0 } = {}) {
    const el = await this.waitFor(css);
    const origin = { [ELEMENT]: el };
    const actions = steps.map(([kind, x = 0, y = 0, ms = 50]) =>
      kind === "move"
        ? { type: "pointerMove", origin, x: Math.round(x), y: Math.round(y), duration: ms }
        : kind === "pause"
          ? { type: "pause", duration: x }
          : { type: kind === "down" ? "pointerDown" : "pointerUp", button },
    );
    await this.cmd("POST", "/actions", {
      actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" }, actions }],
    });
    await this.cmd("DELETE", "/actions");
  }

  async drag(css, fromX, fromY, toX, toY) {
    await this.pointer(css, [["move", fromX, fromY, 0], ["down"], ["move", (fromX + toX) / 2, (fromY + toY) / 2, 120], ["move", toX, toY, 120], ["up"]]);
  }

  async dblclick(css, x = 0, y = 0) {
    await this.pointer(css, [["move", x, y, 0], ["down"], ["up"], ["pause", 40], ["down"], ["up"]]);
  }

  async rightClick(css, x = 0, y = 0) {
    await this.pointer(css, [["move", x, y, 0], ["down"], ["up"]], { button: 2 });
  }

  /** The element's size, for offsets that depend on it. */
  async rect(css) {
    const el = await this.waitFor(css);
    return this.cmd("GET", `/element/${el}/rect`);
  }

  async attr(css, name) {
    const el = await this.waitFor(css);
    return this.cmd("GET", `/element/${el}/attribute/${name}`);
  }

  async count(css) {
    return (await this.findAll(css)).length;
  }

  /** Poll until `css` exists (and, if given, `pred(text)` holds). */
  async waitFor(css, { timeout = 15000, pred } = {}) {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try {
        const el = await this.find(css);
        if (!pred) return el;
        last = await this.cmd("GET", `/element/${el}/text`);
        if (pred(last)) return el;
      } catch (e) {
        last = e.message;
      }
      await new Promise((r) => setTimeout(r, 250));
    }
    throw new Error(`timed out waiting for ${css}${last ? ` (last: ${String(last).slice(0, 200)})` : ""}`);
  }

  async until(fn, { timeout = 15000, label = "condition" } = {}) {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try {
        last = await fn();
        if (last) return last;
      } catch (e) {
        last = e.message;
      }
      await new Promise((r) => setTimeout(r, 250));
    }
    throw new Error(`timed out waiting for ${label} (last: ${String(last).slice(0, 200)})`);
  }

  async screenshot(file) {
    const b64 = await this.cmd("GET", "/screenshot");
    writeFileSync(file, Buffer.from(b64, "base64"));
  }

  /** Call a Tauri command the way the frontend does. */
  invoke(cmd, args = {}) {
    return this.exec((c, a) => window.__TAURI_INTERNALS__.invoke(c, a), cmd, args);
  }
}
