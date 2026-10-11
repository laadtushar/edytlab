// What claude-proxy.mjs keeps of a run, against a stand-in for the API (no
// key, no spend, no display needed):
//
//   node --test apps/desktop/e2e-native/claude-proxy.test.mjs
//
// The proxy sits between the app and the Messages API, so it is the one
// place a run can see the tool calls Claude made and the results it was
// told, which the app's chat does not show (#508). It also sees the app's
// x-api-key on every request, and none of that may be kept or served.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer as createHttp } from "node:http";
import { createServer as createNet } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const KEY = "sk-ant-api03-PROXYTESTSECRET0000000000000000";
const scratch = mkdtempSync(join(tmpdir(), "claude-proxy-test-"));
let upstream;
let proxy;
let base;
const seen = []; // what the stand-in API received: { url, key, body }

const freePort = () =>
  new Promise((resolve) => {
    const s = createNet().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

before(async () => {
  upstream = createHttp(async (req, res) => {
    let data = "";
    for await (const c of req) data += c;
    seen.push({ url: req.url, key: req.headers["x-api-key"], body: JSON.parse(data || "{}") });
    const stream = JSON.parse(data || "{}").stream === true;
    if (stream) {
      res.writeHead(200, { "content-type": "text/event-stream" });
      res.write(`data: ${JSON.stringify({ type: "message_start", message: { usage: { input_tokens: 10 } } })}\n\n`);
      res.write(`data: ${JSON.stringify({ type: "message_delta", usage: { output_tokens: 3 } })}\n\n`);
      res.end();
    } else {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ usage: { input_tokens: 5, output_tokens: 1 } }));
    }
  });
  await new Promise((r) => upstream.listen(0, "127.0.0.1", r));
  const port = await freePort();
  base = `http://127.0.0.1:${port}`;
  proxy = spawn(process.execPath, [new URL("./claude-proxy.mjs", import.meta.url).pathname], {
    // Not the test's own environment: the proxy gets what it needs and nothing else.
    env: { PATH: process.env.PATH, PORT: String(port), OUT: scratch, BUDGET_USD: "100", ANTHROPIC_UPSTREAM: `http://127.0.0.1:${upstream.address().port}` },
    stdio: "ignore",
  });
  for (let i = 0; i < 50; i++) {
    if (await fetch(`${base}/__usage`).then((r) => r.ok, () => false)) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("the proxy did not start");
});

after(() => {
  proxy?.kill();
  upstream?.close();
  rmSync(scratch, { recursive: true, force: true });
});

const post = (body) =>
  fetch(`${base}/v1/messages`, { method: "POST", headers: { "x-api-key": KEY, "anthropic-version": "2023-06-01", "content-type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.text());

const tools = [{ name: "set_track_gain" }];
const first = [{ role: "user", content: [{ type: "text", text: "Make track 0 louder by 3 dB." }] }];
const toolUse = { role: "assistant", content: [{ type: "tool_use", id: "toolu_1", name: "set_track_gain", input: { track: 0, db: 3 } }] };
const toolResult = { role: "user", content: [{ type: "tool_result", tool_use_id: "toolu_1", content: '{"prior_gain_db":0.0,"track_gain_db":3.0}' }] };

test("before any chat request there is nothing to read, and the key still reaches the API", async () => {
  const empty = await fetch(`${base}/__toolcalls`);
  assert.equal(empty.status, 200, "the proxy serves /__toolcalls");
  assert.deepEqual(await empty.json(), { seen: 0, turns: [] });

  await post({ model: "m", stream: true, tools, messages: first });
  assert.equal(seen.at(-1).key, KEY, "the proxy is still a passthrough");
});

test("the latest conversation's tool calls and results are served, turn by turn", async () => {
  await post({ model: "m", stream: true, tools, messages: [...first, toolUse, toolResult] });
  const got = await fetch(`${base}/__toolcalls`).then((r) => r.json());
  assert.equal(got.seen, 2, "two agent-loop requests so far");
  assert.deepEqual(got.turns, [
    {
      turn: 1,
      prompt: "Make track 0 louder by 3 dB.",
      calls: [{ id: "toolu_1", name: "set_track_gain", arguments: { track: 0, db: 3 }, result: '{"prior_gain_db":0.0,"track_gain_db":3.0}', error: false }],
    },
  ]);
});

test("a request with no tools (a classifier, a plan) leaves the conversation as it was", async () => {
  await post({ model: "m", stream: false, messages: [{ role: "user", content: "classify: louder" }] });
  const got = await fetch(`${base}/__toolcalls`).then((r) => r.json());
  assert.equal(got.seen, 2);
  assert.equal(got.turns.length, 1);
});

test("no header, key or body is kept or served", async () => {
  const served = await fetch(`${base}/__toolcalls`).then((r) => r.text());
  const usage = await fetch(`${base}/__usage`).then((r) => r.text());
  const log = readFileSync(join(scratch, "claude-usage.jsonl"), "utf8");
  for (const [where, text] of [["/__toolcalls", served], ["/__usage", usage], ["claude-usage.jsonl", log]]) {
    assert.ok(!text.includes("PROXYTESTSECRET"), `${where} holds the key`);
    assert.ok(!/x-api-key|anthropic-version/i.test(text), `${where} holds a header`);
  }
  assert.deepEqual(Object.keys(JSON.parse(served)), ["seen", "turns"]);
  // The usage log is what it was: counts, the model and the status.
  assert.deepEqual(Object.keys(JSON.parse(log.split("\n")[0])).sort(), ["cache_create", "cache_read", "input", "model", "output", "path", "status", "stream", "t"]);
});
