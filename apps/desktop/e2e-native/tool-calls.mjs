// A record of what the model called and was told back, turn by turn, kept
// with a story's screenshots so a failure that is the model's can be
// explained (#508).
//
// The record comes from the conversation the model was sent, because that
// is the one place a call's arguments and result are both on view. The chat
// shows neither: a tool badge holds the tool's name and a status (the app
// puts no result on it), `agent://tool-call` carries a name and an id, and
// the session graph keeps the arguments of the calls that made a node but
// not what any call answered, nor a call that failed. The Messages API is
// stateless, so each request carries the whole conversation so far, and the
// last one the agent loop sent holds every call of every turn with the
// result the model read for it. What it cannot hold is a call the model made
// after that request (a turn that hung mid-step), and a result not sent yet
// (a turn that ended in an error right after a step): that call has a null
// result.
//
// Two sources, one per model the stories can run against:
//   claude-proxy.mjs  keeps the latest conversation and serves it on
//                     /__toolcalls (real Claude, through the app's own
//                     Anthropic provider);
//   fake-llm.mjs      already logs every request on /__requests (the
//                     scripted model).
//
// Nothing here reads a header, and the environment is read only to mask the
// run's own key. A record holds a story id, a source name, and per turn the
// person's words and each call's id, name, arguments and result text, with
// anything shaped like a key masked.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const PROMPT_LIMIT = 300; // characters kept of what the person said
const STRING_LIMIT = 300; // ... of each string in a call's arguments
const RESULT_LIMIT = 1000; // ... of a call's result

// What a key looks like, for the one that might turn up in a tool's result
// or a prompt; the run's own key is masked by value as well.
const KEY_SHAPES = [
  [/sk-ant-[A-Za-z0-9_-]{8,}/g, "[redacted]"],
  [/\bsk-[A-Za-z0-9_-]{20,}/g, "[redacted]"],
  [/\bBearer\s+[A-Za-z0-9._~+/=-]{16,}/gi, "Bearer [redacted]"],
  [/("?(?:x-api-key|api[_-]?key|authorization)"?\s*[:=]\s*)"?[^\s",}]+"?/gi, "$1[redacted]"],
];

/** `text` with keys masked. Reads the environment only to mask the run's own. */
export function redact(text) {
  let out = String(text);
  const own = process.env.ANTHROPIC_E2E_KEY;
  if (own && own.length >= 8) out = out.split(own).join("[redacted]");
  for (const [shape, to] of KEY_SHAPES) out = out.replace(shape, to);
  return out;
}

function clip(text, limit) {
  const s = redact(text);
  return s.length > limit ? `${s.slice(0, limit)}… (${s.length - limit} more characters)` : s;
}

/** A call's arguments with every string clipped and masked, structure kept. */
function scrub(value) {
  if (typeof value === "string") return clip(value, STRING_LIMIT);
  if (Array.isArray(value)) return value.map(scrub);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, scrub(v)]));
  return value;
}

const blocks = (content) => (typeof content === "string" ? [{ type: "text", text: content }] : Array.isArray(content) ? content : []);
const textOf = (content) =>
  blocks(content)
    .filter((b) => b?.type === "text")
    .map((b) => b.text ?? "")
    .join("\n");

/** Chat-completions carry a call's arguments as JSON text; keep the text if it is not. */
function parseArguments(text) {
  if (typeof text !== "string") return scrub(text ?? {});
  try {
    return scrub(JSON.parse(text));
  } catch {
    return clip(text, RESULT_LIMIT);
  }
}

/**
 * The turns of a conversation as the model was sent it, each with the calls
 * it made and the result it read for each.
 *
 * A turn starts at a user message that says something; one that only
 * carries tool results answers the calls before it. Reads both shapes the
 * app sends: Anthropic's content blocks (`tool_use` / `tool_result`) and
 * chat-completions (`tool_calls` / role `tool`). A call nothing has
 * answered yet has a null result: the conversation ended, or was last sent,
 * before its answer was.
 *
 * Thinking, the assistant's own words and system text are not recorded.
 */
export function turnsFromMessages(messages) {
  const turns = [];
  const calls = new Map();
  let turn = null;
  const begin = (prompt) => {
    turn = { turn: turns.length + 1, prompt: clip(prompt, PROMPT_LIMIT), calls: [] };
    turns.push(turn);
  };
  const call = (id, name, args) => {
    if (!turn) begin("");
    const c = { id: id ?? null, name: String(name ?? ""), arguments: args, result: null, error: false };
    turn.calls.push(c);
    if (id != null) calls.set(id, c);
  };
  const answer = (id, content, error) => {
    const c = calls.get(id);
    if (c) {
      c.result = clip(content, RESULT_LIMIT);
      c.error = error;
    }
  };

  for (const m of Array.isArray(messages) ? messages : []) {
    if (!m || typeof m !== "object") continue;
    if (m.role === "user") {
      const answers = blocks(m.content).filter((b) => b?.type === "tool_result");
      for (const b of answers) answer(b.tool_use_id, textOf(b.content), b.is_error === true);
      const said = textOf(m.content).trim();
      if (!answers.length && said) begin(said);
    } else if (m.role === "assistant") {
      for (const b of blocks(m.content)) if (b?.type === "tool_use") call(b.id, b.name, scrub(b.input ?? {}));
      for (const t of Array.isArray(m.tool_calls) ? m.tool_calls : []) call(t?.id, t?.function?.name, parseArguments(t?.function?.arguments));
    } else if (m.role === "tool") {
      answer(m.tool_call_id, textOf(m.content), false);
    }
  }
  return turns;
}

/** Is `body` a request of the agent loop, the one that carries the
 * conversation and the tools? The classifier and plan calls have no tools
 * and are not streamed, and one of them is not the conversation. */
export function isAgentLoopRequest(body) {
  return !!body && body.stream === true && Array.isArray(body.tools) && body.tools.length > 0 && Array.isArray(body.messages);
}

// --- the file -------------------------------------------------------------

/** Where a story's record goes, relative to the output folder: with its screenshots. */
export const toolCallsFile = (id) => `shots/${String(id).replace(/[^\w.-]/g, "_")}-tool-calls.json`;

/** Write the record; returns its path relative to `out`. Only the record's own fields are written. */
export function writeToolCalls(out, id, { story, source, turns }) {
  const file = toolCallsFile(id);
  mkdirSync(join(out, "shots"), { recursive: true });
  writeFileSync(join(out, file), `${JSON.stringify({ story, source, turns }, null, 2)}\n`);
  return file;
}

// --- sources ----------------------------------------------------------------

async function getJson(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(5000) });
  if (!res.ok) throw new Error(`${url} answered ${res.status}`);
  return res.json();
}

/** The scripted model: every chat request it has received. `expected` is
 * false because it is simply not there when the stories run against a real
 * local model. */
export function fakeSource(base = "http://127.0.0.1:11434") {
  const requests = async () => {
    const all = await getJson(`${base}/__requests`);
    if (!Array.isArray(all)) throw new Error(`${base}/__requests is not the scripted model's log`);
    return all;
  };
  return {
    name: "fake-llm",
    expected: false,
    mark: async () => (await requests()).length,
    async read(mark) {
      const all = await requests();
      // run-env.sh starts the scripted model afresh with every boot, so a
      // log shorter than the mark is a new server's: all of it is since.
      const last = all.slice(all.length < mark ? 0 : mark).findLast(isAgentLoopRequest);
      return last ? turnsFromMessages(last.messages) : null;
    },
  };
}

/** Real Claude, through claude-proxy.mjs. `expected` is true: the proxy is
 * what the Claude stories talk to, so one that cannot be read is news. */
export function proxySource(base = "http://127.0.0.1:8788") {
  const latest = async () => {
    let got;
    try {
      got = await getJson(`${base}/__toolcalls`);
    } catch (e) {
      throw new Error(`the Claude proxy at ${base} cannot be read for tool calls (${e.message}); start it from this checkout (claude-proxy.mjs)`);
    }
    if (typeof got?.seen !== "number" || !Array.isArray(got.turns)) {
      throw new Error(`the Claude proxy at ${base} does not keep tool calls; restart it from this checkout (claude-proxy.mjs)`);
    }
    return got;
  };
  return {
    name: "claude-proxy",
    expected: true,
    mark: async () => (await latest()).seen,
    async read(mark) {
      const { seen, turns } = await latest();
      return seen > mark ? turns : null;
    },
  };
}

/** Where each source stands as a story begins. Never throws. */
export function markSources(sources) {
  return Promise.all(
    sources.map(async (s) => {
      try {
        return { mark: await s.mark() };
      } catch (e) {
        return { error: String(e?.message ?? e) };
      }
    }),
  );
}

const count = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

/**
 * Write the record of a story that has finished, from the first source that
 * saw the model asked something since `marks`. Returns null when none did (a
 * story that never talks to a model leaves nothing), else
 * `{ file, note }`: `note` is set when the story failed, and names the
 * file. A failing story whose Claude proxy could not be read gets a note
 * saying so, because a missing record there is not "nothing happened".
 * Never throws; a story's own failure is not to be hidden by this one.
 */
export async function recordStory({ out, id, sources, marks, failed }) {
  const trouble = [];
  for (const [i, source] of sources.entries()) {
    const mark = marks[i];
    if (mark?.error !== undefined || mark === undefined) {
      if (source.expected) trouble.push(mark?.error ?? "not marked");
      continue;
    }
    let turns;
    try {
      turns = await source.read(mark.mark);
    } catch (e) {
      if (source.expected) trouble.push(String(e?.message ?? e));
      continue;
    }
    if (!turns) continue;
    try {
      const file = writeToolCalls(out, id, { story: id, source: source.name, turns });
      const all = turns.flatMap((t) => t.calls);
      const names = all.map((c) => c.name);
      const shown = names.slice(0, 8).join(", ") + (names.length > 8 ? ", …" : "");
      const bad = all.filter((c) => c.error).length;
      const summary = [count(turns.length, "turn"), count(all.length, "call"), ...(bad ? [`${bad} failed`] : [])].join(", ");
      return { file, note: failed ? `tool calls of every turn: ${file} (${summary}${names.length ? `: ${shown}` : ""})` : undefined };
    } catch (e) {
      return { note: `tool calls of ${id} could not be written: ${String(e?.message ?? e)}` };
    }
  }
  return failed && trouble.length ? { note: `no tool-call record: ${trouble.join("; ")}` } : null;
}
