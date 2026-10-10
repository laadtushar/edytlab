// A minimal W3C WebDriver client for tauri-driver: just what these
// stories need, over node:http, so the native suite adds no dependencies.
import { writeFileSync } from "node:fs";
import { request as httpRequest } from "node:http";

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
  up: "\uE013",
  down: "\uE015",
  backspace: "\uE003",
  del: "\uE017",
};

// How long a command may take before it is given up on: what fetch (undici)
// allowed before this client used node:http.
const TIMEOUT_MS = 300000;

/**
 * One HTTP exchange with tauri-driver, on a connection of its own.
 *
 * Every request gets a fresh TCP connection (`agent: false`), which Node
 * closes once the answer is in. Nothing is pooled on this side: a story's
 * driver lives on the same port as the last story's dead one, and a pooled
 * socket to the dead one made the next story's first request fail.
 *
 * But the request says `Connection: keep-alive`, never `close`, although
 * this side closes it anyway. tauri-driver forwards every header but Host
 * to WebKitWebDriver through a hyper client that pools its connections
 * there and never retries one (`retry_canceled_requests(false)`, in 2.0.6
 * and 2.1.0). WebKitWebDriver's server (libsoup) honours a request's
 * `Connection: close` by closing the socket after the response, but does
 * not say so in the response; hyper decides from the response alone, so it
 * pooled the socket the server was closing. A command handed that socket in
 * the moment before hyper saw it close never reached WebKitWebDriver:
 * tauri-driver logged "client error (SendRequest) ... Connection reset by
 * peer" or "client error (Canceled) ... connection closed", dropped this
 * side's connection without an answer, and the story failed with "other
 * side closed" on whatever command it was on, two or three times a run.
 * With keep-alive the server keeps its end open, and hyper's pool holds
 * only live sockets.
 */
function exchange(method, url, payload, label = `${method} ${new URL(url).pathname}`) {
  const body = payload === undefined ? undefined : Buffer.from(JSON.stringify(payload));
  const headers = { "content-type": "application/json", connection: "keep-alive" };
  if (body) headers["content-length"] = body.length;
  return new Promise((resolve, reject) => {
    const req = httpRequest(url, { method, agent: false, headers }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("error", reject);
      res.on("end", () => {
        let json = {};
        try {
          json = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        } catch {}
        resolve({ ok: res.statusCode >= 200 && res.statusCode < 300, body: json });
      });
    });
    req.setTimeout(TIMEOUT_MS, () =>
      req.destroy(Object.assign(new Error(`no answer in ${TIMEOUT_MS / 1000} s`), { code: "ETIMEDOUT" })),
    );
    req.on("error", reject);
    req.end(body);
  }).catch((e) => {
    // The code says whether the driver refused, reset, or timed out.
    throw new Error(`${label}: request failed (${`${e.code ?? ""} ${e.message}`.trim()})`);
  });
}

export class Driver {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async start({ application, args = [], base = "http://127.0.0.1:4444" }) {
    const { ok, body } = await exchange("POST", `${base}/session`, {
      capabilities: {
        alwaysMatch: { "tauri:options": { application, args } },
      },
    });
    if (!ok || !body.value?.sessionId) throw new Error(`session: ${JSON.stringify(body)}`);
    return new Driver(base, body.value.sessionId);
  }

  async cmd(method, path, payload) {
    const { ok, body } = await exchange(method, `${this.base}/session/${this.id}${path}`, payload, `${method} ${path}`);
    if (!ok) {
      throw new Error(`${method} ${path}: ${JSON.stringify(body.value ?? body).slice(0, 400)}`);
    }
    return body.value;
  }

  async quit() {
    await exchange("DELETE", `${this.base}/session/${this.id}`).catch(() => {});
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
