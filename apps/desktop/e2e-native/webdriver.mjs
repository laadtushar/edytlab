// A minimal W3C WebDriver client for tauri-driver: just what these
// stories need, over fetch, so the native suite adds no dependencies.
import { writeFileSync } from "node:fs";

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

export class Driver {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async start({ application, args = [], base = "http://127.0.0.1:4444" }) {
    const res = await fetch(`${base}/session`, {
      method: "POST",
      headers: { "content-type": "application/json" },
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
      headers: { "content-type": "application/json" },
      body: payload === undefined ? undefined : JSON.stringify(payload),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      throw new Error(`${method} ${path}: ${JSON.stringify(body.value ?? body).slice(0, 400)}`);
    }
    return body.value;
  }

  async quit() {
    await fetch(`${this.base}/session/${this.id}`, { method: "DELETE" }).catch(() => {});
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

  async click(css) {
    const el = await this.waitFor(css);
    await this.cmd("POST", `/element/${el}/click`, {});
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
