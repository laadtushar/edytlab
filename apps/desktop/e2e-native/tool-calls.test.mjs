// The tool-call record a story leaves behind, against stand-in conversations
// (no display, driver, app or model needed):
//
//   node --test apps/desktop/e2e-native/tool-calls.test.mjs
//
// A story that fails on a model's answer ("second turn: +6 dB in total (9)",
// #508) used to leave a screenshot and nothing about what the model had
// called or been told back. The record is built from the conversation the
// model was sent, because the chat shows neither a call's arguments nor
// its result: the badge holds the tool's name and a status.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createHttp } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fakeSource, isAgentLoopRequest, markSources, recordStory, toolCallsFile, turnsFromMessages, writeToolCalls } from "./tool-calls.mjs";

const scratch = mkdtempSync(join(tmpdir(), "tool-calls-test-"));
after(() => rmSync(scratch, { recursive: true, force: true }));

const text = (t) => ({ type: "text", text: t });
const use = (id, name, input) => ({ type: "tool_use", id, name, input });
const result = (id, content, is_error) => ({ type: "tool_result", tool_use_id: id, content, ...(is_error ? { is_error } : {}) });

/** The shape of #508: "Make track 0 louder by 3 dB.", then "Do that again.". */
const followUp = [
  { role: "user", content: [text("Make track 0 louder by 3 dB.")] },
  {
    role: "assistant",
    content: [
      { type: "thinking", thinking: "THOUGHT-NOT-RECORDED", signature: "SIGNATURE-NOT-RECORDED" },
      text("Raising track 0 by 3 dB."),
      use("toolu_1", "set_track_gain", { track: 0, db: 3 }),
    ],
  },
  { role: "user", content: [result("toolu_1", '{"node_id":"ab12","prior_gain_db":0.0,"track_gain_db":3.0,"summary":"Set track 0 gain to +3 dB (was +0 dB)"}')] },
  { role: "assistant", content: [text("Done: track 0 is now +3 dB.")] },
  { role: "user", content: [text("Do that again.")] },
  { role: "assistant", content: [use("toolu_2", "set_track_gain", { track: 0, db: 6 })] },
  // Anthropic allows a result's content to be a list of text blocks.
  { role: "user", content: [result("toolu_2", [text('{"node_id":"cd34","prior_gain_db":3.0,"track_gain_db":6.0}')])] },
];

test("a conversation is split into turns at each thing the person said, with every call and its result", () => {
  const turns = turnsFromMessages(followUp);
  assert.deepEqual(turns, [
    {
      turn: 1,
      prompt: "Make track 0 louder by 3 dB.",
      calls: [
        {
          id: "toolu_1",
          name: "set_track_gain",
          arguments: { track: 0, db: 3 },
          result: '{"node_id":"ab12","prior_gain_db":0.0,"track_gain_db":3.0,"summary":"Set track 0 gain to +3 dB (was +0 dB)"}',
          error: false,
        },
      ],
    },
    {
      turn: 2,
      prompt: "Do that again.",
      calls: [{ id: "toolu_2", name: "set_track_gain", arguments: { track: 0, db: 6 }, result: '{"node_id":"cd34","prior_gain_db":3.0,"track_gain_db":6.0}', error: false }],
    },
  ]);
});

test("the model's thinking is not recorded", () => {
  const json = JSON.stringify(turnsFromMessages(followUp));
  assert.ok(!json.includes("THOUGHT-NOT-RECORDED") && !json.includes("SIGNATURE-NOT-RECORDED"));
});

test("a failed call keeps its error flag, and a call nothing answered yet has a null result", () => {
  const turns = turnsFromMessages([
    { role: "user", content: "Load /nope.wav" },
    { role: "assistant", content: [use("t1", "load", { path: "/nope.wav" }), use("t2", "analyze_track", { track: 0 })] },
    { role: "user", content: [result("t1", "no such file: /nope.wav", true)] },
  ]);
  assert.equal(turns.length, 1);
  assert.equal(turns[0].prompt, "Load /nope.wav", "a plain string is a turn too");
  assert.deepEqual(
    turns[0].calls.map((c) => [c.name, c.result, c.error]),
    [
      ["load", "no such file: /nope.wav", true],
      ["analyze_track", null, false],
    ],
  );
});

test("a turn with no tool calls is still a turn", () => {
  const turns = turnsFromMessages([{ role: "user", content: [text("What can you do?")] }]);
  assert.deepEqual(turns, [{ turn: 1, prompt: "What can you do?", calls: [] }]);
});

test("chat-completions messages (the scripted model's shape) give the same record", () => {
  const turns = turnsFromMessages([
    { role: "system", content: "You edit audio." },
    { role: "user", content: "Make track 0 louder by 6 dB." },
    { role: "assistant", content: null, tool_calls: [{ id: "call_1_0", type: "function", function: { name: "gain", arguments: '{"track":0,"db":6}' } }] },
    { role: "tool", tool_call_id: "call_1_0", content: '{"gain_db":6.0}' },
  ]);
  assert.deepEqual(turns, [
    { turn: 1, prompt: "Make track 0 louder by 6 dB.", calls: [{ id: "call_1_0", name: "gain", arguments: { track: 0, db: 6 }, result: '{"gain_db":6.0}', error: false }] },
  ]);
});

test("arguments that are not JSON are kept as the text the model wrote", () => {
  const [turn] = turnsFromMessages([
    { role: "user", content: "x" },
    { role: "assistant", tool_calls: [{ id: "c", function: { name: "gain", arguments: '{"track":' } }] },
  ]);
  assert.equal(turn.calls[0].arguments, '{"track":');
});

test("a long result is cut and says by how much; short ones are left alone", () => {
  const [turn] = turnsFromMessages([
    { role: "user", content: "x" },
    { role: "assistant", content: [use("t", "analyze_track", {})] },
    { role: "user", content: [result("t", "y".repeat(5000))] },
  ]);
  const r = turn.calls[0].result;
  assert.ok(r.length < 1500, `${r.length} characters kept`);
  assert.match(r, /… \(4000 more characters\)$/);
});

test("a key in a prompt, an argument or a result is masked, and so is the one the run was given", () => {
  const was = process.env.ANTHROPIC_E2E_KEY;
  process.env.ANTHROPIC_E2E_KEY = "run-key-with-no-known-prefix-123456";
  try {
    const json = JSON.stringify(
      turnsFromMessages([
        { role: "user", content: "use sk-ant-api03-PROMPTSECRET0000000000 please" },
        { role: "assistant", content: [use("t", "load", { path: "/a.wav", note: "Bearer abcdefghijklmnopqrstuvwxyz0123", other: "run-key-with-no-known-prefix-123456" })] },
        { role: "user", content: [result("t", 'headers: {"x-api-key":"sk-ant-api03-RESULTSECRET0000000000"}')] },
      ]),
    );
    for (const secret of ["PROMPTSECRET", "RESULTSECRET", "abcdefghijklmnopqrstuvwxyz0123", "run-key-with-no-known-prefix-123456"]) {
      assert.ok(!json.includes(secret), `${secret} is in the record`);
    }
    assert.match(json, /\[redacted\]/);
    assert.ok(json.includes("/a.wav"), "ordinary arguments survive");
  } finally {
    if (was === undefined) delete process.env.ANTHROPIC_E2E_KEY;
    else process.env.ANTHROPIC_E2E_KEY = was;
  }
});

test("isAgentLoopRequest: the streaming request that carries tools, not a classifier or a plan call", () => {
  assert.ok(isAgentLoopRequest({ stream: true, tools: [{ name: "gain" }], messages: [] }));
  assert.ok(!isAgentLoopRequest({ stream: false, tools: [{ name: "gain" }], messages: [] }));
  assert.ok(!isAgentLoopRequest({ stream: true, messages: [] }));
  assert.ok(!isAgentLoopRequest({ stream: true, tools: [], messages: [] }));
  assert.ok(!isAgentLoopRequest({ stream: true, tools: [{ name: "gain" }] }));
  assert.ok(!isAgentLoopRequest(null));
});

test("the file is written next to the screenshots, named for its story, with only the record in it", () => {
  const rel = writeToolCalls(scratch, "7-claude-follow-up", {
    story: "7-claude-follow-up",
    source: "claude-proxy",
    turns: turnsFromMessages(followUp),
    // Nothing outside the record's own fields may reach the file.
    headers: { "x-api-key": "sk-ant-api03-HEADERSECRET00000000" },
    env: { HOME: "/home/someone" },
  });
  assert.equal(rel, "shots/7-claude-follow-up-tool-calls.json");
  assert.equal(rel, toolCallsFile("7-claude-follow-up"));
  const raw = readFileSync(join(scratch, rel), "utf8");
  const written = JSON.parse(raw);
  assert.deepEqual(Object.keys(written), ["story", "source", "turns"]);
  assert.equal(written.turns[1].calls[0].arguments.db, 6);
  assert.ok(!raw.includes("HEADERSECRET") && !raw.includes("/home/someone"));
});

test("a story id cannot write outside the shots folder", () => {
  assert.equal(toolCallsFile("../../etc/passwd"), "shots/.._.._etc_passwd-tool-calls.json");
});

/** A source that has seen `turns` since its mark, like the fake model or the proxy. */
const source = (name, { turns = null, mark = 0, fail, expected = name === "claude-proxy" } = {}) => ({
  name,
  expected,
  mark: async () => {
    if (fail) throw new Error(fail);
    return mark;
  },
  read: async () => {
    if (fail) throw new Error(fail);
    return turns;
  },
});

test("recordStory: a failing story's note names the file, and the file holds every turn", async () => {
  const sources = [source("fake-llm"), source("claude-proxy", { turns: turnsFromMessages(followUp) })];
  const marks = await markSources(sources);
  const r = await recordStory({ out: scratch, id: "7-claude-follow-up", sources, marks, failed: true });
  assert.equal(r.file, "shots/7-claude-follow-up-tool-calls.json");
  assert.match(r.note, /shots\/7-claude-follow-up-tool-calls\.json/);
  assert.match(r.note, /2 turns, 2 calls/);
  assert.match(r.note, /set_track_gain/);
  const written = JSON.parse(readFileSync(join(scratch, r.file), "utf8"));
  assert.equal(written.source, "claude-proxy");
  assert.equal(written.story, "7-claude-follow-up");
  assert.equal(written.turns.length, 2);
});

test("recordStory: a passing story gets the file and no note", async () => {
  const sources = [source("fake-llm", { turns: turnsFromMessages(followUp) })];
  const r = await recordStory({ out: scratch, id: "5-agent-gain", sources, marks: await markSources(sources), failed: false });
  assert.equal(r.file, "shots/5-agent-gain-tool-calls.json");
  assert.equal(r.note, undefined);
  assert.ok(existsSync(join(scratch, r.file)));
});

test("recordStory: a story the model was never asked in leaves no file, and no note", async () => {
  const sources = [source("fake-llm"), source("claude-proxy")];
  for (const failed of [false, true]) {
    const r = await recordStory({ out: scratch, id: "1-onboarding", sources, marks: await markSources(sources), failed });
    assert.equal(r, null);
  }
  assert.ok(!existsSync(join(scratch, "shots/1-onboarding-tool-calls.json")));
});

test("recordStory: a source that cannot be read is skipped quietly, but a failing story is told so for the Claude proxy", async () => {
  const quiet = [source("fake-llm", { fail: "ECONNREFUSED" })];
  assert.equal(await recordStory({ out: scratch, id: "a", sources: quiet, marks: await markSources(quiet), failed: true }), null);

  const proxy = [source("claude-proxy", { fail: "the proxy has no /__toolcalls; restart it" })];
  const r = await recordStory({ out: scratch, id: "b", sources: proxy, marks: await markSources(proxy), failed: true });
  assert.equal(r.file, undefined);
  assert.match(r.note, /no tool-call record/);
  assert.match(r.note, /restart it/);
  assert.equal(await recordStory({ out: scratch, id: "c", sources: proxy, marks: await markSources(proxy), failed: false }), null, "a passing story is not nagged");
});

test("recordStory never throws: a write that fails is a note", async () => {
  const sources = [source("fake-llm", { turns: turnsFromMessages(followUp) })];
  const marks = await markSources(sources);
  // `out` is a file, so `shots/` cannot be made under it.
  const notAFolder = join(scratch, "not-a-folder");
  writeFileSync(notAFolder, "");
  const r = await recordStory({ out: notAFolder, id: "x", sources, marks, failed: true });
  assert.equal(r.file, undefined);
  assert.match(r.note, /could not be written/);
});

// --- against the real scripted model --------------------------------------

const freePort = () =>
  new Promise((resolve) => {
    const s = createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

test("fakeSource reads what fake-llm.mjs was sent, from the story's mark on", async () => {
  const port = await freePort();
  const child = spawn(process.execPath, [new URL("./fake-llm.mjs", import.meta.url).pathname], { env: { PATH: process.env.PATH, FAKE_LLM_PORT: String(port) }, stdio: "ignore" });
  try {
    const base = `http://127.0.0.1:${port}`;
    for (let i = 0; i < 40; i++) {
      if (await fetch(`${base}/__requests`).then((r) => r.ok, () => false)) break;
      await new Promise((r) => setTimeout(r, 100));
    }
    const chat = (messages, extra = {}) =>
      fetch(`${base}/v1/chat/completions`, { method: "POST", body: JSON.stringify({ model: "fake-editor", stream: true, tools: [{ type: "function", function: { name: "gain" } }], messages, ...extra }) }).then((r) => r.text());

    // An earlier story's traffic.
    await chat([{ role: "user", content: "an earlier story" }]);

    const src = fakeSource(base);
    const mark = await src.mark();
    assert.equal(await src.read(mark), null, "nothing asked since the mark");

    await fetch(`${base}/__script`, { method: "POST", body: JSON.stringify([{ tool_calls: [{ name: "gain", arguments: { track: 0, db: 6 } }] }, { text: "Done." }]) });
    const first = await chat([{ role: "user", content: "Make track 0 louder by 6 dB." }]);
    assert.match(first, /"name":"gain"/, "the scripted call went out");
    await chat([{ role: "user", content: "Make track 0 louder by 6 dB." }, { role: "assistant", content: null, tool_calls: [{ id: "call_2_0", function: { name: "gain", arguments: '{"track":0,"db":6}' } }] }, { role: "tool", tool_call_id: "call_2_0", content: '{"gain_db":6.0}' }]);
    // A classifier call after it, which carries no tools, must not hide the conversation.
    await chat([{ role: "user", content: "classify this" }], { stream: false, tools: undefined });

    const turns = await src.read(mark);
    assert.equal(turns.length, 1);
    assert.equal(turns[0].prompt, "Make track 0 louder by 6 dB.");
    assert.deepEqual(turns[0].calls.map((c) => [c.name, c.arguments, c.result]), [["gain", { track: 0, db: 6 }, '{"gain_db":6.0}']]);
  } finally {
    child.kill();
  }
});

test("fakeSource: a scripted model restarted since the mark (its log is shorter) is read from its start", async () => {
  // run-env.sh starts a new scripted model with every boot of the app.
  let log = [];
  const server = createHttp((req, res) => res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(log)));
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  try {
    const src = fakeSource(`http://127.0.0.1:${server.address().port}`);
    const loop = (prompt) => ({ stream: true, tools: [{ type: "function" }], messages: [{ role: "user", content: prompt }] });
    log = [loop("an earlier story"), loop("an earlier story"), loop("an earlier story")];
    const mark = await src.mark();
    assert.equal(mark, 3);
    log = [loop("this story")]; // the model server started again
    assert.equal((await src.read(mark))[0].prompt, "this story");
  } finally {
    server.close();
  }
});
