// A passthrough between the app and api.anthropic.com that counts what a run
// spends and stops it at a budget.
//
//   BUDGET_USD=12 PORT=8788 node claude-proxy.mjs
//   GET /__usage  the totals so far
//
// The app is pointed at it through Settings' base URL, and sends its own
// x-api-key. Nothing here reads, stores or logs a header or a body: only
// token counts, the model and the status are kept, in $OUT/claude-usage.jsonl.
//
// Cost is an upper bound. Every input token is priced as fresh input, at
// the highest rate of the models the app uses, so the real bill is lower.
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { createServer } from "node:http";

const PORT = Number(process.env.PORT ?? 8788);
const BUDGET = Number(process.env.BUDGET_USD ?? 12);
const OUT = process.env.OUT ?? "/tmp/edytlab-native";
const IN_PER_M = 5;
const OUT_PER_M = 25;

const totals = { requests: 0, input: 0, output: 0, cache_read: 0, cache_create: 0, refused: 0 };
// The budget is for the whole run, across restarts of this proxy: start
// from what earlier processes recorded.
if (existsSync(`${OUT}/claude-usage.jsonl`)) {
  for (const line of readFileSync(`${OUT}/claude-usage.jsonl`, "utf8").split("\n")) {
    if (!line.trim()) continue;
    const e = JSON.parse(line);
    totals.requests += 1;
    totals.input += e.input;
    totals.output += e.output;
    totals.cache_read += e.cache_read;
    totals.cache_create += e.cache_create;
  }
}
const cost = () => (totals.input + totals.cache_read + totals.cache_create) * (IN_PER_M / 1e6) + totals.output * (OUT_PER_M / 1e6);

function record(entry) {
  appendFileSync(`${OUT}/claude-usage.jsonl`, `${JSON.stringify(entry)}\n`);
}

function addUsage(u, into) {
  if (!u) return;
  into.input += u.input_tokens ?? 0;
  into.cache_read += u.cache_read_input_tokens ?? 0;
  into.cache_create += u.cache_creation_input_tokens ?? 0;
  into.output = Math.max(into.output, u.output_tokens ?? 0);
}

const server = createServer(async (req, res) => {
  if (req.url === "/__usage") {
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ ...totals, usd_upper_bound: Number(cost().toFixed(4)), budget: BUDGET }));
    return;
  }
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = Buffer.concat(chunks);

  if (cost() >= BUDGET) {
    totals.refused += 1;
    res.writeHead(429, { "content-type": "application/json" }).end(
      JSON.stringify({ type: "error", error: { type: "rate_limit_error", message: `the end-to-end budget of $${BUDGET} is spent` } }),
    );
    return;
  }

  let model = "";
  let stream = false;
  try {
    const parsed = JSON.parse(body.toString() || "{}");
    model = parsed.model ?? "";
    stream = parsed.stream === true;
  } catch {}

  const headers = {};
  for (const [k, v] of Object.entries(req.headers)) {
    if (["host", "content-length", "connection", "accept-encoding", "transfer-encoding"].includes(k)) continue;
    headers[k] = v;
  }
  headers["accept-encoding"] = "identity";

  const used = { input: 0, output: 0, cache_read: 0, cache_create: 0 };
  let status = 0;
  try {
    const up = await fetch(`https://api.anthropic.com${req.url}`, {
      method: req.method,
      headers,
      body: ["GET", "HEAD"].includes(req.method) ? undefined : body,
    });
    status = up.status;
    const out = {};
    up.headers.forEach((v, k) => {
      if (!["content-encoding", "content-length", "transfer-encoding", "connection"].includes(k)) out[k] = v;
    });
    res.writeHead(up.status, out);
    let tail = "";
    let whole = "";
    for await (const chunk of up.body) {
      res.write(chunk);
      const text = Buffer.from(chunk).toString();
      if (stream) {
        tail += text;
        const lines = tail.split("\n");
        tail = lines.pop() ?? "";
        for (const line of lines) {
          if (!line.startsWith("data:")) continue;
          try {
            const ev = JSON.parse(line.slice(5));
            if (ev.type === "message_start") addUsage(ev.message?.usage, used);
            if (ev.type === "message_delta") addUsage(ev.usage ? { output_tokens: ev.usage.output_tokens } : null, used);
          } catch {}
        }
      } else {
        whole += text;
      }
    }
    if (!stream) {
      try {
        addUsage(JSON.parse(whole).usage, used);
      } catch {}
    }
    res.end();
  } catch (e) {
    if (!res.headersSent) res.writeHead(502, { "content-type": "application/json" });
    res.end(JSON.stringify({ type: "error", error: { type: "api_error", message: `proxy: ${String(e.message ?? e).slice(0, 120)}` } }));
  }
  totals.requests += 1;
  totals.input += used.input;
  totals.output += used.output;
  totals.cache_read += used.cache_read;
  totals.cache_create += used.cache_create;
  record({ t: new Date().toISOString(), path: req.url, model, stream, status, ...used });
});
// The app keeps connections in a pool and reuses them after a model turn
// that can take minutes. Node's default 5 s keep-alive closed them under
// it ("error sending request"), which is this proxy's fault, not the
// app's: the real API keeps them open far longer.
server.keepAliveTimeout = 10 * 60 * 1000;
server.headersTimeout = server.keepAliveTimeout + 1000;
server.requestTimeout = 0;
server.listen(PORT, "127.0.0.1", () => console.log(`claude proxy on :${PORT}, budget $${BUDGET}, spent so far ≤ $${cost().toFixed(2)}`));
