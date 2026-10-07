// A scripted, OpenAI-compatible model server standing in for Ollama on
// :11434, so the agent's stories run through the real app and real tools
// with only the model's replies scripted.
//
//   POST /__script   [{ text?, tool_calls?: [{ name, arguments }] }, ...]
//                    queued replies, one per chat request, in order
//   GET  /__requests the chat requests received, newest last
//   GET  /v1/models  one model, so Settings lists it
//   POST /v1/chat/completions  streams the next queued reply as SSE
import { createServer } from "node:http";

const port = Number(process.env.FAKE_LLM_PORT ?? 11434);
let queue = [];
const requests = [];

function readBody(req) {
  return new Promise((resolve) => {
    let data = "";
    req.on("data", (c) => (data += c));
    req.on("end", () => resolve(data));
  });
}

function sse(res, chunks) {
  res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
  for (const c of chunks) res.write(`data: ${JSON.stringify(c)}\n\n`);
  res.write("data: [DONE]\n\n");
  res.end();
}

function chunk(delta, finish = null) {
  return {
    id: "fake-1",
    model: "fake-editor",
    choices: [{ index: 0, delta, finish_reason: finish }],
  };
}

createServer(async (req, res) => {
  const body = await readBody(req);
  if (req.method === "POST" && req.url === "/__script") {
    queue = JSON.parse(body);
    res.writeHead(200).end("ok");
    return;
  }
  if (req.url === "/__requests") {
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(requests));
    return;
  }
  if (req.url === "/v1/models") {
    res
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify({ object: "list", data: [{ id: "fake-editor", object: "model" }] }));
    return;
  }
  if (req.method === "POST" && req.url === "/v1/chat/completions") {
    const parsed = JSON.parse(body || "{}");
    requests.push(parsed);
    const turn = queue.shift() ?? { text: "Done." };
    if (!parsed.stream) {
      // Non-streaming callers (a classifier, a connection test).
      const message = { role: "assistant", content: turn.text ?? "ok" };
      res
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify({ id: "fake-1", model: "fake-editor", choices: [{ index: 0, message, finish_reason: "stop" }] }));
      return;
    }
    const chunks = [chunk({ role: "assistant" })];
    if (turn.text) chunks.push(chunk({ content: turn.text }));
    (turn.tool_calls ?? []).forEach((call, i) => {
      chunks.push(
        chunk({
          tool_calls: [
            {
              index: i,
              id: `call_${requests.length}_${i}`,
              type: "function",
              function: { name: call.name, arguments: JSON.stringify(call.arguments ?? {}) },
            },
          ],
        }),
      );
    });
    chunks.push(chunk({}, turn.tool_calls?.length ? "tool_calls" : "stop"));
    sse(res, chunks);
    return;
  }
  res.writeHead(404).end();
}).listen(port, "127.0.0.1", () => console.log(`fake llm on :${port}`));
