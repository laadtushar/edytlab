/**
 * Which failed sends the chat banner routes to Settings, and what it
 * says about them (#414).
 *
 * The strings are what the app really shows: `ai::Error::Api` words a
 * refused request "the model provider returned an error ({status}):
 * {body}", the commands layer prefixes "ai error: ", and the body is the
 * provider's own. Each provider refuses a bad key in its own words —
 * Anthropic "invalid x-api-key", OpenAI "invalid_api_key", Gemini a 400
 * with "API key not valid" — which is how a check for "api key" alone
 * missed every one of them.
 */

import { describe, expect, it } from "vitest";

import {
  describeChatError,
  isCredentialError,
  needsSettings,
} from "../lib/chat-errors";

const api = (status: number, body: string) =>
  `ai error: the model provider returned an error (${status}): ${body}`;

/** The exact text of the native run against Anthropic with a wrong key. */
const ANTHROPIC_WRONG_KEY = api(
  401,
  '{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}',
);

const REFUSED_KEY: Record<string, string> = {
  "Anthropic, wrong key (401)": ANTHROPIC_WRONG_KEY,
  "Anthropic, key without permission (403)": api(
    403,
    '{"type":"error","error":{"type":"permission_error","message":"Your API key does not have permission to use the specified resource."}}',
  ),
  "OpenAI, wrong key (401)": api(
    401,
    '{"error":{"message":"Incorrect API key provided: sk-abc***xyz. You can find your API key at https://platform.openai.com/account/api-keys.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}',
  ),
  "Groq, wrong key (401)": api(
    401,
    '{"error":{"message":"Invalid API Key","type":"invalid_request_error","code":"invalid_api_key"}}',
  ),
  "OpenRouter, no credentials (401, no key wording)": api(
    401,
    '{"error":{"message":"No auth credentials found","code":401}}',
  ),
  "a 403 with an empty body": api(403, ""),
  // Gemini answers a bad key with 400, not 401: only its words say so.
  "Gemini, wrong key (400)": api(
    400,
    '[{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID"}]}}]',
  ),
  // Mid-stream errors carry no HTTP status, only the provider's event.
  "Anthropic authentication_error inside the stream": `ai error: the model provider's stream failed: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`,
  "an api-key spelt with a hyphen": "the server rejected the Api-Key header",
  "an APIKEY in capitals": "missing APIKEY",
  "a bare HTTP 401": "HTTP 401 from the endpoint",
  "a status 403": "request failed with status 403",
  "401 Unauthorized": "401 Unauthorized",
};

const NOT_A_KEY_PROBLEM: Record<string, string> = {
  "a server error (500)": api(
    500,
    '{"type":"error","error":{"type":"api_error","message":"Internal server error"}}',
  ),
  "Anthropic overloaded (529)": api(
    529,
    '{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}',
  ),
  "a rate limit (429)": api(
    429,
    '{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed the rate limit for your organization of 50 requests per minute."}}',
  ),
  // Waiting fixes this; a new key does not, whatever the text mentions.
  "a rate limit that names the key (429)": api(
    429,
    '{"error":{"message":"Rate limit reached for this API key. Please try again in 20s.","code":"rate_limit_exceeded"}}',
  ),
  "a server error that mentions authentication (503)": api(
    503,
    '{"error":{"message":"authentication service unavailable, try again"}}',
  ),
  "a bad request about the model (400)": api(
    400,
    '{"type":"error","error":{"type":"invalid_request_error","message":"model: claude-x not found"}}',
  ),
  // `ai::Error::ContextTooSmall`'s own text (#395). No fix for it is in
  // Settings' key field, so it must not offer Settings, and its words
  // ("context", "Ollama") must not read as a credential.
  "a model context that is too small": `ai error: The model's context window is too small for this request (it needs about 15157 tokens; the model has 8192). Start a new chat to drop the earlier messages, or choose a model with a larger context. For a local model, give it more: with Ollama, raise its context length; with llama.cpp, start the server with a larger --ctx-size.`,
  "a network failure": "ai error: http error: error sending request for url (https://api.anthropic.com/v1/messages)",
  // 401 and 403 here are seconds, not statuses.
  "a tool error with 401 and 403 in it":
    "ai error: tool argument validation failed twice: fade: duration 403 s is longer than the track (401.2 s)",
  "a status-like number inside a longer one":
    "ai error: the model provider's stream failed: upstream timed out after 14013 ms (request 4401)",
  "the stream failing on overload": `ai error: the model provider's stream failed: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`,
  "a silent source": "source is silent",
  // Reaching the tool budget used to be an error here (#439); now the turn
  // ends with the model's summary. This is a failure of the same kind that
  // still is one.
  "a plan approval timing out":
    "ai error: plan approval timed out after 5 minutes; nothing was run",
  "a musical key": "detected key: A minor (confidence 0.82)",
};

describe("isCredentialError", () => {
  for (const [label, message] of Object.entries(REFUSED_KEY)) {
    it(`recognises ${label}`, () => {
      expect(isCredentialError(message), message).toBe(true);
    });
  }

  for (const [label, message] of Object.entries(NOT_A_KEY_PROBLEM)) {
    it(`leaves alone ${label}`, () => {
      expect(isCredentialError(message), message).toBe(false);
    });
  }

  // No key at all is its own case, and the banner must not say a key
  // was rejected when none was sent.
  it("does not call a missing agent a rejected key", () => {
    expect(
      isCredentialError("no agent configured; call set_api_key first"),
    ).toBe(false);
  });
});

describe("needsSettings", () => {
  it("offers Settings when no agent is configured (#250)", () => {
    expect(needsSettings("no agent configured; call set_api_key first")).toBe(
      true,
    );
  });

  for (const [label, message] of Object.entries(REFUSED_KEY)) {
    it(`offers Settings for ${label}`, () => {
      expect(needsSettings(message), message).toBe(true);
    });
  }

  for (const [label, message] of Object.entries(NOT_A_KEY_PROBLEM)) {
    it(`does not offer Settings for ${label}`, () => {
      expect(needsSettings(message), message).toBe(false);
    });
  }
});

describe("describeChatError", () => {
  it("says in words that the key was refused, and keeps the provider's detail", () => {
    const text = describeChatError(ANTHROPIC_WRONG_KEY);
    expect(text).toMatch(/^The provider rejected the API key\./);
    // The status and the provider's own message are what someone
    // reporting the problem, or checking which key, needs.
    expect(text).toContain("(401)");
    expect(text).toContain("invalid x-api-key");
  });

  it("reads an Error's message", () => {
    const text = describeChatError(new Error(ANTHROPIC_WRONG_KEY));
    expect(text).toMatch(/^The provider rejected the API key\./);
    expect(text).toContain("invalid x-api-key");
  });

  it("words every other failure as before", () => {
    const message = api(500, "Internal server error");
    expect(describeChatError(message)).toBe(
      `Could not complete request: ${message}`,
    );
    expect(describeChatError("no agent configured; call set_api_key first")).toBe(
      "Could not complete request: no agent configured; call set_api_key first",
    );
  });

  // The banner decides on its button from the text it shows, so wording
  // that text must not change the answer either way.
  it("is offered Settings exactly when the error it describes is", () => {
    const all = {
      ...REFUSED_KEY,
      ...NOT_A_KEY_PROBLEM,
      "no agent": "no agent configured; call set_api_key first",
    };
    for (const message of Object.values(all)) {
      expect(needsSettings(describeChatError(message)), message).toBe(
        needsSettings(message),
      );
    }
  });

  it("still says something for an empty rejection", () => {
    expect(describeChatError(undefined)).toBe(
      "Could not complete request: unknown error",
    );
  });
});
