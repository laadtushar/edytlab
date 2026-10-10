// The WebDriver client's contract with tauri-driver, against a local
// stand-in server (no display, driver or app needed):
//
//   node --test apps/desktop/e2e-native/webdriver.test.mjs
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Driver } from "./webdriver.mjs";

/** A stand-in for tauri-driver that records what reaches it. `routes`
 * maps "METHOD /path" to (req, res, body) => void; anything else answers
 * `{ value: null }`. */
function standIn(routes = {}, port = 0) {
  const seen = { requests: [], connections: 0, open: 0 };
  const server = createServer((req, res) => {
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => {
      seen.requests.push({ method: req.method, url: req.url, connection: req.headers.connection, body: raw, socket: req.socket });
      const route = routes[`${req.method} ${req.url}`];
      if (route) return route(req, res, raw);
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ value: null }));
    });
  });
  server.on("connection", (s) => {
    seen.connections++;
    seen.open++;
    s.on("close", () => seen.open--);
  });
  return new Promise((resolve) =>
    server.listen(port, "127.0.0.1", () => resolve({ server, seen, base: `http://127.0.0.1:${server.address().port}` })),
  );
}

const close = (server) =>
  new Promise((resolve) => {
    server.closeAllConnections();
    server.close(resolve);
  });

const until = async (cond, ms = 2000) => {
  const end = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > end) throw new Error("timed out");
    await new Promise((r) => setTimeout(r, 10));
  }
};

let s;
before(async () => {
  s = await standIn({
    "POST /session": (req, res) => res.end(JSON.stringify({ value: { sessionId: "s1", capabilities: {} } })),
    "GET /session/s1/element/e1/text": (req, res) => res.end(JSON.stringify({ value: "hello" })),
    "POST /session/s1/element": (req, res) => {
      res.statusCode = 404;
      res.end(JSON.stringify({ value: { error: "no such element", message: "Unable to locate .nope" } }));
    },
    // What tauri-driver does when its request to WebKitWebDriver fails:
    // the connection is dropped without an answer.
    "GET /session/s1/screenshot": (req) => req.socket.destroy(),
    "POST /session/s1/execute/async": (req, res, body) => res.end(JSON.stringify({ value: { ok: JSON.parse(body).args } })),
  });
});
after(() => close(s.server));

test("every request says keep-alive, so tauri-driver never forwards a close to WebKitWebDriver", async () => {
  const from = s.seen.requests.length;
  const d = await Driver.start({ application: "/bin/app", base: s.base });
  assert.equal(d.id, "s1");
  assert.equal(await d.cmd("GET", "/element/e1/text"), "hello");
  assert.deepEqual(await d.exec((a, b) => a + b, 1, 2), [1, 2]);
  await d.quit();
  const reqs = s.seen.requests.slice(from);
  assert.equal(reqs.length, 4);
  for (const r of reqs) assert.equal(r.connection?.toLowerCase(), "keep-alive", `${r.method} ${r.url} said ${r.connection}`);
});

test("every request has a connection of its own, which this side closes once answered", async () => {
  const d = new Driver(s.base, "s1");
  const conns = s.seen.connections;
  const from = s.seen.requests.length;
  for (let i = 0; i < 5; i++) assert.equal(await d.cmd("GET", "/element/e1/text"), "hello");
  assert.equal(s.seen.connections - conns, 5, "one connection per request");
  assert.equal(new Set(s.seen.requests.slice(from).map((r) => r.socket)).size, 5);
  await until(() => s.seen.open === 0);
});

test("a driver restarted on the same port is reached by the very next request", async () => {
  const one = await standIn({ "GET /session/s1/url": (req, res) => res.end(JSON.stringify({ value: "one" })) });
  const port = one.server.address().port;
  const d = new Driver(one.base, "s1");
  assert.equal(await d.cmd("GET", "/url"), "one");
  await close(one.server);
  // The suite restarts the driver through execFileSync, which blocks the
  // event loop: nothing here can see the old socket go away meanwhile.
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 200);
  const two = await standIn({ "GET /session/s1/url": (req, res) => res.end(JSON.stringify({ value: "two" })) }, port);
  try {
    assert.equal(await d.cmd("GET", "/url"), "two");
  } finally {
    await close(two.server);
  }
});

test("a connection dropped without an answer names the command and the cause", async () => {
  const d = new Driver(s.base, "s1");
  await assert.rejects(d.cmd("GET", "/screenshot"), /^Error: GET \/screenshot: request failed \(ECONNRESET socket hang up\)$/);
});

test("an error answer carries the driver's message", async () => {
  const d = new Driver(s.base, "s1");
  await assert.rejects(d.cmd("POST", "/element", { using: "css selector", value: ".nope" }), /POST \/element: .*no such element/);
});

test("a large answer is read whole", async () => {
  const png = Buffer.alloc(3 * 1024 * 1024, 7);
  const big = await standIn({ "GET /session/s1/screenshot": (req, res) => res.end(JSON.stringify({ value: png.toString("base64") })) });
  try {
    const file = join(mkdtempSync(join(tmpdir(), "wd-test-")), "shot.png");
    await new Driver(big.base, "s1").screenshot(file);
    assert.ok(readFileSync(file).equals(png));
  } finally {
    await close(big.server);
  }
});
